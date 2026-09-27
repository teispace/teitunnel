//! Approvals for clients on MCP 2026-07-28 and later, which ask the person through
//! Multi Round-Trip Requests (SEP-2322) instead of server-initiated `elicitation/create`.
//!
//! A change that needs approval answers `tools/call` with `resultType: "input_required"`:
//! an elicitation (`inputRequests.approve`) and an opaque `requestState`. The client asks
//! the person, then calls the tool again with their answer (`inputResponses.approve`) and
//! the same `requestState`, and the tool runs again from the start.
//!
//! `requestState` passes through the client, so it's sealed here: an HMAC-SHA256 with a
//! key made from the OS's random generator when the process starts (it never leaves the
//! process, so states don't survive a restart). It binds the answer to one call (the
//! tool, a digest of its arguments, who called), to what the person was shown (a digest
//! of the approval's title and details: for `apply_plan`, the plan id and its steps), and
//! to ten minutes. A state that fails any check is refused; an approval is used once.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rmcp::model::{
    ElicitRequest, ElicitRequestParams, ElicitResult, ElicitationAction, ElicitationSchema,
    InputRequest, InputRequests, InputRequiredResult, InputResponses, JsonObject,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// How long the person has to answer.
pub(crate) const STATE_TTL: Duration = Duration::from_secs(10 * 60);
/// The key of the approval question in `inputRequests` and `inputResponses`.
pub(crate) const APPROVE: &str = "approve";

/// Seals and opens request states, and remembers which approvals were used.
pub(crate) struct Sealer {
    key: [u8; 32],
    used: Mutex<HashMap<String, Instant>>,
}

impl std::fmt::Debug for Sealer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sealer").finish_non_exhaustive()
    }
}

/// What a sealed state says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    /// The tool.
    pub(crate) tool: String,
    /// A digest of its arguments (without `confirmed`).
    pub(crate) args: String,
    /// Who called: the client and how it reached the server.
    pub(crate) principal: String,
    /// A digest of what the person is asked.
    pub(crate) approval: String,
    /// Makes each question unique, so an answer is used once.
    pub(crate) nonce: String,
    /// When it lapses (milliseconds since the epoch).
    pub(crate) expires: u64,
}

/// Why a state was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Not a state this process sealed, or changed since.
    Invalid,
    /// Older than [`STATE_TTL`].
    Expired,
    /// Sealed for another call.
    OtherCall,
}

impl Refused {
    /// For the protocol error.
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Invalid => {
                "requestState isn't valid (this server may have restarted). Call the tool again without it."
            }
            Self::Expired => {
                "requestState expired: the person has ten minutes to answer. Call the tool again without it."
            }
            Self::OtherCall => {
                "requestState belongs to another call. Retry the same tool with the same arguments, or call again without it."
            }
        }
    }
}

/// One call, as a state is bound to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Call {
    pub(crate) tool: String,
    pub(crate) args: String,
    pub(crate) principal: String,
}

impl Call {
    /// The call of `tool` with `arguments` by `principal`.
    pub(crate) fn new(tool: &str, arguments: &JsonObject, principal: String) -> Self {
        let mut arguments = arguments.clone();
        // The agent's own confirmation isn't part of what's approved.
        arguments.remove("confirmed");
        Self {
            tool: tool.to_owned(),
            args: digest(&canonical(&Value::Object(arguments))),
            principal,
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// JSON with object keys sorted, so equal arguments digest the same.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        Value::String(k.clone()),
                        canonical(map.get(k).unwrap_or(&Value::Null))
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

fn digest(text: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(text.as_bytes()))
}

/// A digest of what the person is asked to approve.
pub(crate) fn approval_digest(title: &str, details: &str) -> String {
    digest(&format!("{title}\n\u{0}\n{details}"))
}

/// HMAC-SHA256 (RFC 2104).
fn hmac(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut padded = [0u8; BLOCK];
    padded[..key.len()].copy_from_slice(key);
    let mut inner = Sha256::new();
    inner.update(padded.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(padded.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

impl Sealer {
    /// A sealer with a new random key.
    ///
    /// # Errors
    /// The OS's random generator failed.
    pub(crate) fn new() -> Result<Self, getrandom::Error> {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key)?;
        Ok(Self {
            key,
            used: Mutex::default(),
        })
    }

    /// A new state for `call`, asking about `approval` (a digest).
    pub(crate) fn seal_new(&self, call: &Call, approval: &str) -> String {
        let mut nonce = [0u8; 16];
        // Without randomness the nonce is only the time; the HMAC still binds it.
        let _ = getrandom::fill(&mut nonce);
        self.seal(&State {
            tool: call.tool.clone(),
            args: call.args.clone(),
            principal: call.principal.clone(),
            approval: approval.to_owned(),
            nonce: format!("{}{}", URL_SAFE_NO_PAD.encode(nonce), now_ms()),
            expires: now_ms() + u64::try_from(STATE_TTL.as_millis()).unwrap_or(u64::MAX),
        })
    }

    fn seal(&self, state: &State) -> String {
        let payload = serde_json::to_vec(state).unwrap_or_default();
        let mac = hmac(&self.key, &payload);
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&payload),
            URL_SAFE_NO_PAD.encode(mac)
        )
    }

    /// Opens a state a client sent back for `call`.
    pub(crate) fn open(&self, sealed: &str, call: &Call) -> Result<State, Refused> {
        use subtle::ConstantTimeEq as _;
        let (payload, mac) = sealed.split_once('.').ok_or(Refused::Invalid)?;
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| Refused::Invalid)?;
        let mac = URL_SAFE_NO_PAD.decode(mac).map_err(|_| Refused::Invalid)?;
        let expected = hmac(&self.key, &payload);
        if mac.len() != expected.len() || !bool::from(mac.as_slice().ct_eq(&expected)) {
            return Err(Refused::Invalid);
        }
        let state: State = serde_json::from_slice(&payload).map_err(|_| Refused::Invalid)?;
        if state.expires <= now_ms() {
            return Err(Refused::Expired);
        }
        if state.tool != call.tool || state.args != call.args || state.principal != call.principal {
            return Err(Refused::OtherCall);
        }
        Ok(state)
    }

    /// Marks an approval used; `false` if it already was (a replay).
    pub(crate) fn consume(&self, nonce: &str) -> bool {
        let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
        used.retain(|_, at| at.elapsed() < STATE_TTL);
        used.insert(nonce.to_owned(), Instant::now()).is_none()
    }
}

/// The person's answer in `inputResponses`: `Some(true)` approve, `Some(false)` no (or
/// dismissed), `None` not answered.
pub(crate) fn answer(responses: Option<&InputResponses>) -> Option<bool> {
    let result: ElicitResult = serde_json::from_value(responses?.get(APPROVE)?.clone()).ok()?;
    Some(match result.action {
        ElicitationAction::Accept => {
            result
                .content
                .as_ref()
                .and_then(|c| c.get(APPROVE))
                .and_then(Value::as_bool)
                == Some(true)
        }
        _ => false,
    })
}

/// The form asking the person to approve (the same as legacy elicitation's).
pub(crate) fn approval_form(message: String) -> Option<ElicitRequestParams> {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "approve": {
                "type": "boolean",
                "title": "Approve this change",
                "description": "Teitunnel applies it only if you approve.",
                "default": false
            }
        },
        "required": ["approve"]
    });
    let Value::Object(schema) = schema else {
        return None;
    };
    Some(ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: crate::limits::truncate(message),
        requested_schema: ElicitationSchema::from_json_schema(schema).ok()?,
    })
}

/// The `input_required` result asking `message`, with a new sealed state.
pub(crate) fn input_required(
    sealer: &Sealer,
    call: &Call,
    approval: &str,
    message: String,
) -> Option<InputRequiredResult> {
    let form = approval_form(message)?;
    let mut requests = InputRequests::new();
    requests.insert(
        APPROVE.to_owned(),
        InputRequest::Elicitation(ElicitRequest::new(form)),
    );
    Some(InputRequiredResult::new(
        Some(requests),
        Some(sealer.seal_new(call, approval)),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(args: &Value) -> Call {
        Call::new(
            "apply_plan",
            args.as_object().unwrap(),
            "test-client via mcp".into(),
        )
    }

    #[test]
    fn a_state_opens_only_for_its_own_call() {
        let sealer = Sealer::new().unwrap();
        let args = json!({ "planId": "p1", "fingerprint": "f1" });
        let sealed = sealer.seal_new(&call(&args), "digest");
        let state = sealer.open(&sealed, &call(&args)).unwrap();
        assert_eq!(state.approval, "digest");
        // Key order and `confirmed` don't matter; everything else does.
        let same = json!({ "confirmed": true, "fingerprint": "f1", "planId": "p1" });
        assert!(sealer.open(&sealed, &call(&same)).is_ok());
        let other = json!({ "planId": "p2", "fingerprint": "f1" });
        assert_eq!(sealer.open(&sealed, &call(&other)), Err(Refused::OtherCall));
        let mut someone = call(&args);
        someone.principal = "another-client via mcp".into();
        assert_eq!(sealer.open(&sealed, &someone), Err(Refused::OtherCall));
        // Another process (another key) can't open it.
        assert_eq!(
            Sealer::new().unwrap().open(&sealed, &call(&args)),
            Err(Refused::Invalid)
        );
    }

    #[test]
    fn tampering_is_refused() {
        let sealer = Sealer::new().unwrap();
        let args = json!({ "planId": "p1" });
        let sealed = sealer.seal_new(&call(&args), "digest");
        let (payload, mac) = sealed.split_once('.').unwrap();
        let mut state: State =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
        state.approval = "something else".into();
        let forged = format!(
            "{}.{mac}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&state).unwrap())
        );
        assert_eq!(sealer.open(&forged, &call(&args)), Err(Refused::Invalid));
        assert_eq!(sealer.open("nonsense", &call(&args)), Err(Refused::Invalid));
        assert_eq!(sealer.open("a.b", &call(&args)), Err(Refused::Invalid));
    }

    #[test]
    fn states_expire_and_answers_are_used_once() {
        let sealer = Sealer::new().unwrap();
        let args = json!({});
        let expired = sealer.seal(&State {
            tool: "apply_plan".into(),
            args: call(&args).args,
            principal: "test-client via mcp".into(),
            approval: "d".into(),
            nonce: "n".into(),
            expires: now_ms() - 1,
        });
        assert_eq!(sealer.open(&expired, &call(&args)), Err(Refused::Expired));
        assert!(sealer.consume("n1"));
        assert!(!sealer.consume("n1"), "a replayed approval");
    }

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 1: a 20-byte key of 0x0b. Keys are zero-padded to the block size, so
        // the same key padded to 32 bytes gives the same MAC.
        let mut key = [0u8; 32];
        key[..20].fill(0x0b);
        let mac = hmac(&key, b"Hi There");
        assert_eq!(
            mac.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn reads_the_persons_answer() {
        let mut responses = InputResponses::new();
        assert_eq!(answer(Some(&responses)), None);
        responses.insert(
            APPROVE.into(),
            json!({ "action": "accept", "content": { "approve": true } }),
        );
        assert_eq!(answer(Some(&responses)), Some(true));
        responses.insert(
            APPROVE.into(),
            json!({ "action": "accept", "content": { "approve": false } }),
        );
        assert_eq!(answer(Some(&responses)), Some(false));
        responses.insert(APPROVE.into(), json!({ "action": "cancel" }));
        assert_eq!(answer(Some(&responses)), Some(false));
        assert_eq!(answer(None), None);
    }
}
