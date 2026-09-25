//! `teitunnel://` links, for launchers (Raycast, Alfred), scripts and web pages:
//!
//! - `teitunnel://share?port=3000`: share `localhost:3000` (always asks first: any web
//!   page can open a link).
//! - `teitunnel://open`, `teitunnel://open?route=app.example.com`,
//!   `teitunnel://open?share=<id>`: bring the window to a view (nothing changes).
//! - `teitunnel://inspect?share=<id>`: a share's request inspector.
//!
//! Parsing is strict: unknown actions or parameters, repeated parameters and values
//! that aren't a port, hostname or id are refused.

use std::{
    borrow::Cow,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{
    host::{Action, ConfirmRequest, Decision, Host, Requester},
    protocol::{HostHeader, RpcError, ShareInfo, StartShare, View, code},
};

/// The URL scheme.
pub const SCHEME: &str = "teitunnel";

/// The longest link accepted.
const MAX_LINK: usize = 2048;

/// What a link asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    /// Share a port on this machine.
    Share {
        /// The port.
        port: u16,
    },
    /// Show a view.
    Open(View),
    /// Show a share's request inspector.
    Inspect {
        /// The share's id.
        share: String,
    },
}

/// Why a link was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// Not a `teitunnel://` link, or too long.
    #[error("Not a Teitunnel link.")]
    NotOurs,
    /// An action Teitunnel doesn't know.
    #[error("Teitunnel doesn't know the link action {0}.")]
    UnknownAction(String),
    /// A parameter that's missing, repeated, unknown or invalid.
    #[error("The link's {0} parameter isn't valid.")]
    BadParameter(String),
}

impl DeepLink {
    /// Parses a link.
    ///
    /// # Errors
    /// See [`LinkError`].
    pub fn parse(link: &str) -> Result<Self, LinkError> {
        let link = link.trim();
        if link.len() > MAX_LINK {
            return Err(LinkError::NotOurs);
        }
        let (scheme, rest) = link.split_once("://").ok_or(LinkError::NotOurs)?;
        if !scheme.eq_ignore_ascii_case(SCHEME) {
            return Err(LinkError::NotOurs);
        }
        let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
        let (action, query) = rest.split_once('?').unwrap_or((rest, ""));
        let action = action.trim_end_matches('/').to_ascii_lowercase();
        let params = Params::parse(query)?;
        match action.as_str() {
            "share" => {
                params.only(&["port"])?;
                let port = params
                    .get("port")
                    .and_then(|p| p.parse::<u16>().ok())
                    .filter(|p| *p > 0)
                    .ok_or_else(|| LinkError::BadParameter("port".into()))?;
                Ok(Self::Share { port })
            }
            "open" => {
                params.only(&["route", "share"])?;
                match (params.get("route"), params.get("share")) {
                    (None, None) => Ok(Self::Open(View::Overview)),
                    (Some(route), None) => Ok(Self::Open(View::Route {
                        hostname: hostname(route)?,
                    })),
                    (None, Some(share)) => Ok(Self::Open(View::Share {
                        id: Some(share_id(share)?),
                    })),
                    (Some(_), Some(_)) => Err(LinkError::BadParameter("route".into())),
                }
            }
            "inspect" => {
                params.only(&["share"])?;
                let share = params
                    .get("share")
                    .ok_or_else(|| LinkError::BadParameter("share".into()))?;
                Ok(Self::Inspect {
                    share: share_id(share)?,
                })
            }
            other => Err(LinkError::UnknownAction(other.chars().take(32).collect())),
        }
    }

    /// Whether the person must confirm it: anything that shares or changes. Links only
    /// navigate otherwise.
    pub fn requires_confirmation(&self) -> bool {
        matches!(self, Self::Share { .. })
    }
}

/// A link's query parameters (each at most once).
struct Params(Vec<(String, String)>);

impl Params {
    fn parse(query: &str) -> Result<Self, LinkError> {
        let mut params: Vec<(String, String)> = Vec::new();
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = decode(key).to_ascii_lowercase();
            if params.iter().any(|(k, _)| *k == key) {
                return Err(LinkError::BadParameter(key));
            }
            params.push((key, decode(value).into_owned()));
        }
        Ok(Self(params))
    }

    fn only(&self, allowed: &[&str]) -> Result<(), LinkError> {
        match self.0.iter().find(|(k, _)| !allowed.contains(&k.as_str())) {
            Some((key, _)) => Err(LinkError::BadParameter(key.chars().take(32).collect())),
            None => Ok(()),
        }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

fn decode(text: &str) -> Cow<'_, str> {
    if text.contains(['%', '+']) {
        let spaced = text.replace('+', " ");
        Cow::Owned(
            percent_encoding::percent_decode_str(&spaced)
                .decode_utf8_lossy()
                .into_owned(),
        )
    } else {
        Cow::Borrowed(text)
    }
}

/// A public hostname: letters, digits and hyphens in dot-separated labels.
fn hostname(value: &str) -> Result<String, LinkError> {
    let host = value.trim().trim_end_matches('.').to_ascii_lowercase();
    let valid = host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if valid {
        Ok(host)
    } else {
        Err(LinkError::BadParameter("route".into()))
    }
}

/// A share id: up to 128 of letters, digits, `.`, `_` and `-`.
fn share_id(value: &str) -> Result<String, LinkError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if valid {
        Ok(value.to_owned())
    } else {
        Err(LinkError::BadParameter("share".into()))
    }
}

/// What handling a link did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handled {
    /// A share started.
    Shared(ShareInfo),
    /// The window shows the view.
    Opened,
    /// The person said no.
    Declined,
    /// Another link is waiting for the person's answer; this one was dropped.
    Busy,
}

/// Handles links for the app: asks before sharing (never "always": a link isn't a
/// client the person can vouch for), one question at a time so a page can't queue up
/// dialogs, then does it through the same host as the control connection.
#[derive(Debug, Default)]
pub struct LinkHandler {
    asking: AtomicBool,
}

impl LinkHandler {
    /// Handles one link.
    ///
    /// # Errors
    /// What the host reported (e.g. the port has nothing listening).
    pub async fn handle(&self, host: &dyn Host, link: DeepLink) -> Result<Handled, RpcError> {
        match link {
            DeepLink::Share { port } => {
                if self.asking.swap(true, Ordering::SeqCst) {
                    return Ok(Handled::Busy);
                }
                let request = StartShare {
                    origin: port.to_string(),
                    stop_after_seconds: None,
                    host_header: HostHeader::Auto,
                    folder: None,
                };
                let decision = host
                    .confirm(ConfirmRequest {
                        requester: Requester::Link,
                        action: Action::StartShare(request.clone()),
                        offer_always: false,
                    })
                    .await;
                self.asking.store(false, Ordering::SeqCst);
                match decision {
                    Decision::Once | Decision::Always => {
                        let share = host.start_share(request).await?;
                        let _ = host
                            .open(View::Share {
                                id: Some(share.id.clone()),
                            })
                            .await;
                        Ok(Handled::Shared(share))
                    }
                    Decision::Deny => Ok(Handled::Declined),
                }
            }
            DeepLink::Open(view) => {
                host.open(view).await?;
                Ok(Handled::Opened)
            }
            DeepLink::Inspect { share } => {
                host.open(View::Inspector { share }).await?;
                Ok(Handled::Opened)
            }
        }
    }
}

/// Refuses a link when links are turned off in Settings.
pub fn disabled() -> RpcError {
    RpcError::new(
        code::DISABLED,
        "Teitunnel links are turned off in Settings.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_links() {
        assert_eq!(
            DeepLink::parse("teitunnel://share?port=3000"),
            Ok(DeepLink::Share { port: 3000 })
        );
        assert_eq!(
            DeepLink::parse("TEITUNNEL://share/?port=8080#x"),
            Ok(DeepLink::Share { port: 8080 })
        );
        assert_eq!(
            DeepLink::parse("teitunnel://open?route=App.Example.com"),
            Ok(DeepLink::Open(View::Route {
                hostname: "app.example.com".into()
            }))
        );
        assert_eq!(
            DeepLink::parse("teitunnel://open"),
            Ok(DeepLink::Open(View::Overview))
        );
        assert_eq!(
            DeepLink::parse("teitunnel://open?share=qs-1"),
            Ok(DeepLink::Open(View::Share {
                id: Some("qs-1".into())
            }))
        );
        assert_eq!(
            DeepLink::parse("teitunnel://inspect?share=3f2a-9"),
            Ok(DeepLink::Inspect {
                share: "3f2a-9".into()
            })
        );
        assert_eq!(
            DeepLink::parse("teitunnel://open?route=a%2Eexample.com"),
            Ok(DeepLink::Open(View::Route {
                hostname: "a.example.com".into()
            }))
        );
    }

    #[test]
    fn refuses_anything_else() {
        for bad in [
            "https://share?port=3000",
            "teitunnel:share?port=3000",
            "teitunnel://share",
            "teitunnel://share?port=0",
            "teitunnel://share?port=70000",
            "teitunnel://share?port=3000&port=4000",
            "teitunnel://share?port=3000&origin=evil.com",
            "teitunnel://share?port=3000&host=10.0.0.1",
            "teitunnel://open?route=not a host",
            "teitunnel://open?route=localhost",
            "teitunnel://open?route=a.com&share=x",
            "teitunnel://inspect",
            "teitunnel://inspect?share=../../etc",
            "teitunnel://delete?route=a.com",
        ] {
            assert!(DeepLink::parse(bad).is_err(), "{bad}");
        }
        let long = format!("teitunnel://open?route={}.com", "a".repeat(3000));
        assert_eq!(DeepLink::parse(&long), Err(LinkError::NotOurs));
    }

    #[test]
    fn only_sharing_needs_a_confirmation() {
        assert!(DeepLink::Share { port: 1 }.requires_confirmation());
        assert!(!DeepLink::Open(View::Overview).requires_confirmation());
        assert!(!DeepLink::Inspect { share: "x".into() }.requires_confirmation());
    }
}
