//! LAN access for phones: advertise `name.local` over multicast DNS (RFC 6762) pointing at
//! this machine's LAN addresses, as an `_https._tcp` service, so a phone on the same network
//! can open `https://name.local`.

use std::{collections::HashMap, fmt, net::IpAddr};

use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::name::{LocalName, Suffix};

/// The DNS-SD service type advertised.
pub const SERVICE_TYPE: &str = "_https._tcp.local.";

/// An mDNS failure.
#[derive(Debug, thiserror::Error)]
pub enum MdnsError {
    /// Only `.local` names can be advertised.
    #[error("{0} isn't a .local name")]
    NotLocal(LocalName),
    /// The mDNS daemon failed.
    #[error("mdns: {0}")]
    Daemon(#[from] mdns_sd::Error),
}

/// The service record for `name` on `port`. With no `addrs`, the daemon fills in every
/// interface's address and keeps them current.
///
/// # Errors
/// `name` isn't `.local`, or the record is invalid.
pub fn service_info(
    name: &LocalName,
    addrs: &[IpAddr],
    port: u16,
) -> Result<ServiceInfo, MdnsError> {
    if name.suffix() != Suffix::Local {
        return Err(MdnsError::NotLocal(name.clone()));
    }
    let host = format!("{name}.");
    let instance = name.as_str().trim_end_matches(".local");
    let info = ServiceInfo::new(
        SERVICE_TYPE,
        instance,
        &host,
        addrs,
        port,
        &[("path", "/")][..],
    )?;
    Ok(if addrs.is_empty() {
        info.enable_addr_auto()
    } else {
        info
    })
}

/// Advertises names until withdrawn or dropped.
pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    registered: HashMap<LocalName, String>,
}

impl fmt::Debug for MdnsAdvertiser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsAdvertiser")
            .field("names", &self.registered.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl MdnsAdvertiser {
    /// Starts the mDNS daemon (its own thread; no async runtime needed).
    ///
    /// # Errors
    /// The daemon couldn't start (no multicast socket).
    pub fn new() -> Result<Self, MdnsError> {
        Ok(Self {
            daemon: ServiceDaemon::new()?,
            registered: HashMap::new(),
        })
    }

    /// Advertises `name` at `addrs` (empty: all interfaces) on `port`, replacing an earlier
    /// advertisement of the same name.
    ///
    /// # Errors
    /// See [`service_info`]; or the daemon refused.
    pub fn advertise(
        &mut self,
        name: &LocalName,
        addrs: &[IpAddr],
        port: u16,
    ) -> Result<(), MdnsError> {
        let info = service_info(name, addrs, port)?;
        self.withdraw(name)?;
        let fullname = info.get_fullname().to_owned();
        self.daemon.register(info)?;
        self.registered.insert(name.clone(), fullname);
        Ok(())
    }

    /// Stops advertising `name` (sends goodbye packets).
    ///
    /// # Errors
    /// The daemon refused.
    pub fn withdraw(&mut self, name: &LocalName) -> Result<(), MdnsError> {
        if let Some(fullname) = self.registered.remove(name) {
            self.daemon.unregister(&fullname)?;
        }
        Ok(())
    }

    /// The advertised names.
    pub fn names(&self) -> impl Iterator<Item = &LocalName> {
        self.registered.keys()
    }

    /// Withdraws everything and stops the daemon.
    ///
    /// # Errors
    /// The daemon refused.
    pub fn shutdown(mut self) -> Result<(), MdnsError> {
        let names: Vec<LocalName> = self.registered.keys().cloned().collect();
        for name in &names {
            self.withdraw(name)?;
        }
        self.daemon.shutdown()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_https_service_for_local_names() {
        let name = LocalName::parse_any("myapp.local").unwrap();
        let ip: IpAddr = "192.168.1.20".parse().unwrap();
        let info = service_info(&name, &[ip], 443).unwrap();
        assert_eq!(info.get_hostname(), "myapp.local.");
        assert_eq!(info.get_port(), 443);
        assert_eq!(info.get_fullname(), "myapp._https._tcp.local.");
        assert!(info.get_addresses().contains(&ip));
        assert!(matches!(
            service_info(
                &LocalName::parse_any("myapp.localhost").unwrap(),
                &[ip],
                443
            ),
            Err(MdnsError::NotLocal(_))
        ));
        let auto = service_info(&name, &[], 443).unwrap();
        assert!(auto.is_addr_auto());
    }

    /// Uses the real network (multicast). Run by hand.
    #[test]
    #[ignore = "sends multicast packets on the local network"]
    fn advertises_and_withdraws() {
        let mut adv = MdnsAdvertiser::new().unwrap();
        let name = LocalName::parse_any("teitunnel-test.local").unwrap();
        adv.advertise(&name, &[], 443).unwrap();
        assert_eq!(adv.names().count(), 1);
        adv.shutdown().unwrap();
    }
}
