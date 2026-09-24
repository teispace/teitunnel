//! Upstreams: origin servers (pooled HTTP client) and folders (static file server).

use std::sync::Arc;

use crate::{LensError, Upstream};

mod folder;
mod origin;
mod tls;

pub(crate) use folder::Folder;
pub(crate) use origin::OriginClient;

/// A ready-to-use upstream.
#[derive(Debug, Clone)]
pub(crate) enum ActiveUpstream {
    Origin(Arc<OriginClient>),
    Folder(Arc<Folder>),
}

impl ActiveUpstream {
    pub(crate) fn build(upstream: &Upstream) -> Result<Self, LensError> {
        Ok(match upstream {
            Upstream::Origin(config) => Self::Origin(Arc::new(OriginClient::new(config.clone())?)),
            Upstream::Folder(config) => Self::Folder(Arc::new(Folder::new(config.clone())?)),
        })
    }
}
