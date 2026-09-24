//! The live state of a tap: its validated configuration (swapped atomically on update),
//! sequence numbers, metrics and sign-in limiter.

use std::sync::{
    Arc, PoisonError, RwLock,
    atomic::{AtomicU64, Ordering::Relaxed},
};

use serde::Serialize;

use crate::{
    LensError, TapConfig, TapId,
    gate::LoginLimiter,
    metrics::TapMetrics,
    rules::{self, CompiledOp},
    upstream::ActiveUpstream,
};

/// A validated configuration, ready for the request path.
#[derive(Debug)]
pub(crate) struct Active {
    pub(crate) config: TapConfig,
    pub(crate) upstream: ActiveUpstream,
    pub(crate) fingerprint: [u8; 32],
    pub(crate) request_ops: Vec<CompiledOp>,
    pub(crate) response_ops: Vec<CompiledOp>,
}

impl Active {
    /// Validates `config` and builds its upstream. `previous` is reused when the
    /// upstream didn't change, so pooled connections survive unrelated updates.
    pub(crate) fn build(config: TapConfig, previous: Option<&Self>) -> Result<Self, LensError> {
        for stub in &config.stubs {
            stub.validate()?;
        }
        if let crate::HostHeader::Custom(host) = &config.host_header {
            http::HeaderValue::from_str(host)
                .map_err(|_| LensError::InvalidConfig(format!("invalid Host header {host:?}")))?;
        }
        if config.capture.max_body_bytes > 64 * 1024 * 1024 {
            return Err(LensError::InvalidConfig(
                "the capture cap can't exceed 64 MiB per body".into(),
            ));
        }
        let request_ops = rules::compile(&config.headers.request)?;
        let response_ops = rules::compile(&config.headers.response)?;
        let upstream = match previous {
            Some(previous) if previous.config.upstream == config.upstream => {
                previous.upstream.clone()
            }
            _ => ActiveUpstream::build(&config.upstream)?,
        };
        Ok(Self {
            fingerprint: config.gates.fingerprint(),
            config,
            upstream,
            request_ops,
            response_ops,
        })
    }
}

/// A tap as the runtime sees it.
#[derive(Debug)]
pub(crate) struct TapRuntime {
    pub(crate) id: TapId,
    active: RwLock<Arc<Active>>,
    seq: AtomicU64,
    pub(crate) metrics: Arc<TapMetrics>,
    pub(crate) limiter: LoginLimiter,
}

impl TapRuntime {
    pub(crate) fn new(id: TapId, active: Active) -> Self {
        Self {
            id,
            active: RwLock::new(Arc::new(active)),
            seq: AtomicU64::new(0),
            metrics: Arc::new(TapMetrics::new()),
            limiter: LoginLimiter::default(),
        }
    }

    /// The current configuration (cheap: one `Arc` clone).
    pub(crate) fn active(&self) -> Arc<Active> {
        Arc::clone(&self.active.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub(crate) fn replace(&self, active: Active) {
        *self.active.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(active);
    }

    pub(crate) fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Relaxed) + 1
    }

    pub(crate) fn info(&self) -> TapInfo {
        let active = self.active();
        TapInfo {
            id: self.id.clone(),
            name: active.config.name.clone(),
            upstream: active.config.upstream.describe(),
            paused: active.config.paused.is_some(),
            protected: active.config.gates.is_active(),
            capturing: active.config.capture.enabled,
        }
    }
}

/// A summary of a tap, for lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TapInfo {
    /// Id.
    pub id: TapId,
    /// Display name.
    pub name: String,
    /// Where it forwards, e.g. `http://localhost:3000`.
    pub upstream: String,
    /// Whether the paused page is on.
    pub paused: bool,
    /// Whether any gate is active.
    pub protected: bool,
    /// Whether exchanges are recorded.
    pub capturing: bool,
}
