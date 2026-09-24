//! Typed client for the Cloudflare API v4.
//!
//! This crate knows HTTP and JSON, and the shapes of the endpoints Teitunnel uses
//! (accounts, zones, DNS records, tunnels and their configurations). It knows nothing
//! about Teitunnel's product model; that lives in `teitunnel-core`.

mod access;
mod client;
mod dns;
mod envelope;
mod error;
mod load_balancing;
mod management;
mod multipart;
mod networks;
mod probe;
mod resources;
mod token;
mod tunnels;
mod workers;

pub use access::{
    AccessApp, AccessOrganization, AccessPolicy, IdentityProvider, NewAccessApp, TEITUNNEL_PREFIX,
    email_domain_rule, email_rule, rule_email, rule_email_domain,
};
pub use client::{API_BASE, Client};
pub use dns::{DnsRecord, NewDnsRecord};
pub use envelope::{ApiMessage, Envelope, ResultInfo};
pub use error::{Error, Result};
pub use load_balancing::{LoadBalancer, Monitor, Origin, OriginHealth, Pool, PoolHealth};
pub use management::{
    Connector, ConnectorConnection, LogStream, MANAGEMENT_BASE, RemoteLog, StreamError,
};
pub use networks::{
    DefaultDeviceProfile, DeviceSettings, NetworkRoute, SplitTunnelEntry, VirtualNetwork,
};
pub use probe::{Access, NIL_ID, NIL_UUID};
pub use resources::{Account, AccountRef, Plan, TokenStatus, Zone, ZoneStatus};
pub use token::ApiToken;
pub use tunnels::{Connection, IngressRule, Tunnel, TunnelConfig, VersionedConfig};
pub use workers::{
    AssetEntry, AssetFile, DeploymentVersion, MODULE_TYPE, UploadSession, WorkerDeployment,
    WorkerDomain, WorkerModule, WorkerVersion,
};

pub(crate) use resources::encode;

/// Percent-encodes a query value (hostnames, comments), keeping `.` readable.
pub(crate) fn encode_query(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
