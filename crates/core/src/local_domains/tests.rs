//! Local domains end to end: real TLS handshakes through Lens with the generated CA,
//! SNI routing, redirects, inspection, persistence, renewal, trust and the Doctor.
//! Nothing here touches the system's trust stores, resolver or privileged ports.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use localdomains::{Clock, ManualClock};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, RootCertStore, pki_types::ServerName},
};

use super::{
    LocalDomainFix, LocalDomainInput, LocalDomains, LocalDomainsConfig, NameResolution, PortReason,
    TrustOptions,
    acceptor::{CheckedPlain, PeerPolicy},
};
use crate::{
    inspect::{ExchangeQuery, Inspector, TapScope, lens::Acceptor},
    store::Store,
};

/// An origin answering every request with `body` (and the Host it saw).
async fn origin(body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut head = Vec::new();
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    let Ok(n) = stream.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    head.extend_from_slice(&buf[..n]);
                }
                let head = String::from_utf8_lossy(&head).to_lowercase();
                let host = head
                    .lines()
                    .find_map(|l| l.strip_prefix("host: "))
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                let text = format!("{body} {host}");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    port
}

fn input(name: &str, target: &str) -> LocalDomainInput {
    LocalDomainInput {
        name: name.into(),
        target: target.into(),
        wildcard: false,
        https: true,
        inspect: false,
    }
}

struct Setup {
    domains: LocalDomains,
    inspector: Inspector,
    _dir: tempfile::TempDir,
}

fn setup_with(store: Store, config: impl FnOnce(&mut LocalDomainsConfig)) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let inspector = Inspector::new(None, None, "test");
    let mut cfg = LocalDomainsConfig::isolated(dir.path());
    config(&mut cfg);
    Setup {
        domains: LocalDomains::new(store, inspector.clone(), cfg),
        inspector,
        _dir: dir,
    }
}

fn setup() -> Setup {
    setup_with(Store::open_in_memory().unwrap(), |_| {})
}

async fn connector(domains: &LocalDomains) -> TlsConnector {
    let pem = domains.ca_certificate().await.unwrap();
    let mut roots = RootCertStore::empty();
    for cert in rustls_pemfile_certs(&pem) {
        roots.add(cert).unwrap();
    }
    let config = ClientConfig::builder_with_provider(localdomains::crypto_provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

fn rustls_pemfile_certs(
    pem: &str,
) -> Vec<tokio_rustls::rustls::pki_types::CertificateDer<'static>> {
    use tokio_rustls::rustls::pki_types::{CertificateDer, pem::PemObject};
    CertificateDer::pem_slice_iter(pem.as_bytes())
        .map(Result::unwrap)
        .collect()
}

/// A GET over TLS with `sni` (and the same Host); the response text.
async fn https_get(
    connector: &TlsConnector,
    port: u16,
    sni: &str,
    path: &str,
) -> std::io::Result<String> {
    let tcp = TcpStream::connect(("127.0.0.1", port)).await?;
    let name = ServerName::try_from(sni.to_owned()).unwrap();
    let mut tls = connector.connect(name, tcp).await?;
    tls.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: {sni}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await?;
    let mut out = Vec::new();
    // Servers may close without a TLS close_notify.
    let _ = tls.read_to_end(&mut out).await;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

async fn http_get(port: u16, host: &str, path: &str) -> String {
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tcp.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    let mut out = Vec::new();
    let _ = tcp.read_to_end(&mut out).await;
    String::from_utf8_lossy(&out).into_owned()
}

#[tokio::test]
async fn serves_https_with_the_generated_ca_and_routes_by_sni() {
    let s = setup();
    let shop = origin("shop").await;
    let api = origin("api").await;
    s.domains
        .add(&input("shop.test", &shop.to_string()))
        .await
        .unwrap();
    s.domains
        .add(&LocalDomainInput {
            wildcard: true,
            ..input("api.localhost", &format!("localhost:{api}"))
        })
        .await
        .unwrap();
    let status = s.domains.status().await;
    assert!(status.running, "{:?}", status.error);
    let port = status.https_port.unwrap();
    assert!(status.domains.iter().all(|d| d.serving));
    assert_eq!(
        status.domains[0].url,
        format!("https://api.localhost:{port}")
    );
    assert!(status.resolver.needed && status.resolver.responding);

    let tls = connector(&s.domains).await;
    let reply = https_get(&tls, port, "shop.test", "/").await.unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.ends_with("shop shop.test"), "Host is kept: {reply}");
    let reply = https_get(&tls, port, "v2.api.localhost", "/x")
        .await
        .unwrap();
    assert!(reply.ends_with("api v2.api.localhost"), "{reply}");
    // No certificate for names that aren't local domains (or deeper than a wildcard).
    assert!(https_get(&tls, port, "evil.com", "/").await.is_err());
    assert!(
        https_get(&tls, port, "a.b.api.localhost", "/")
            .await
            .is_err()
    );
    assert!(https_get(&tls, port, "other.test", "/").await.is_err());
    // The certificate is short-lived and was issued for the name.
    let expiry = s.domains.certificate_expiry("shop.test").await.unwrap();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    assert!((expiry - now) > 29 * 86_400 && (expiry - now) <= 30 * 86_400);
}

#[tokio::test]
async fn plain_http_redirects_to_https_and_serves_http_only_domains() {
    let s = setup();
    let a = origin("a").await;
    let b = origin("b").await;
    s.domains
        .add(&input("secure.localhost", &a.to_string()))
        .await
        .unwrap();
    s.domains
        .add(&LocalDomainInput {
            https: false,
            ..input("plain.localhost", &b.to_string())
        })
        .await
        .unwrap();
    let status = s.domains.status().await;
    let (https, http) = (status.https_port.unwrap(), status.http_port.unwrap());
    let moved = http_get(http, "secure.localhost", "/cart?id=1").await;
    assert!(moved.starts_with("HTTP/1.1 308"), "{moved}");
    assert!(
        moved.contains(&format!(
            "location: https://secure.localhost:{https}/cart?id=1"
        )),
        "{moved}"
    );
    let plain = http_get(http, "plain.localhost", "/").await;
    assert!(plain.ends_with("b plain.localhost"), "{plain}");
    let url = &status
        .domains
        .iter()
        .find(|d| d.name == "plain.localhost")
        .unwrap()
        .url;
    assert_eq!(url, &format!("http://plain.localhost:{http}"));
    // Plain HTTP-only names get no certificate.
    let tls = connector(&s.domains).await;
    assert!(
        https_get(&tls, https, "plain.localhost", "/")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn inspection_records_requests_in_the_inspector_when_on() {
    let s = setup();
    let port = origin("hi").await;
    s.domains
        .add(&LocalDomainInput {
            inspect: true,
            ..input("hooks.localhost", &port.to_string())
        })
        .await
        .unwrap();
    let status = s.domains.status().await;
    let tap = status.domains[0].tap_id.clone().unwrap();
    let https = status.https_port.unwrap();
    let tls = connector(&s.domains).await;
    https_get(&tls, https, "hooks.localhost", "/paid")
        .await
        .unwrap();
    let view = s.inspector.view(&tap).unwrap();
    assert_eq!(
        view.scope,
        TapScope::LocalDomain {
            name: "hooks.localhost".into()
        }
    );
    assert_eq!(view.address, format!("https://hooks.localhost:{https}"));
    assert!(view.capturing);
    let captured = || {
        s.inspector
            .list(&ExchangeQuery {
                tap: Some(tap.clone()),
                ..ExchangeQuery::default()
            })
            .items
            .len()
    };
    for _ in 0..50 {
        if captured() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(captured(), 1);

    s.domains
        .set_inspect("hooks.localhost", false)
        .await
        .unwrap();
    assert!(!s.inspector.view(&tap).unwrap().capturing, "same tap");
    https_get(&tls, https, "hooks.localhost", "/again")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(captured(), 1, "nothing recorded while off");
    assert_eq!(s.domains.status().await.domains[0].requests, 2);
}

#[tokio::test]
async fn domains_persist_and_another_process_serves_them() {
    let db = tempfile::tempdir().unwrap();
    let path = db.path().join("teitunnel.db");
    let port = origin("kept").await;
    {
        let s = setup_with(Store::open(&path).unwrap(), |_| {});
        s.domains
            .add(&input("kept.localhost", &port.to_string()))
            .await
            .unwrap();
        s.domains.stop().await;
    }
    let s = setup_with(Store::open(&path).unwrap(), |_| {});
    let before = s.domains.status().await;
    assert!(!before.running);
    assert_eq!(before.domains.len(), 1);
    assert!(!before.domains[0].serving);
    s.domains.sync().await.unwrap();
    let status = s.domains.status().await;
    let tls = connector(&s.domains).await;
    let reply = https_get(&tls, status.https_port.unwrap(), "kept.localhost", "/")
        .await
        .unwrap();
    assert!(reply.ends_with("kept kept.localhost"), "{reply}");
}

#[tokio::test]
async fn adding_validates_and_removing_the_last_stops_serving() {
    let s = setup();
    let port = origin("x").await;
    for (name, target) in [
        ("shop.com", "3000"),
        ("*.shop.test", "3000"),
        ("localhost", "3000"),
        ("shop.test", "nope nope"),
        ("shop.test", "ftp://x"),
    ] {
        assert!(s.domains.add(&input(name, target)).await.is_err(), "{name}");
    }
    s.domains
        .add(&input("Shop.Test", &port.to_string()))
        .await
        .unwrap();
    let err = s
        .domains
        .add(&input("shop.test", "4000"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("already"), "{err}");
    let status = s.domains.status().await;
    assert_eq!(status.domains[0].name, "shop.test");
    let https = status.https_port.unwrap();
    s.domains.remove("shop.test").await.unwrap();
    assert!(s.domains.remove("shop.test").await.is_err());
    let status = s.domains.status().await;
    assert!(!status.running && status.domains.is_empty());
    assert!(TcpStream::connect(("127.0.0.1", https)).await.is_err());
    assert!(!status.resolver.responding, "the name server stopped too");
}

#[tokio::test]
async fn certificates_are_renewed_before_they_expire() {
    let clock = Arc::new(ManualClock::new(time::OffsetDateTime::now_utc()));
    let clock_for_config: Arc<dyn Clock> = clock.clone();
    let s = setup_with(Store::open_in_memory().unwrap(), move |c| {
        c.clock = clock_for_config;
    });
    let port = origin("r").await;
    s.domains
        .add(&input("renew.localhost", &port.to_string()))
        .await
        .unwrap();
    let https = s.domains.status().await.https_port.unwrap();
    let tls = connector(&s.domains).await;
    https_get(&tls, https, "renew.localhost", "/")
        .await
        .unwrap();
    let first = s
        .domains
        .certificate_expiry("renew.localhost")
        .await
        .unwrap();
    assert_eq!(s.domains.renew().await.unwrap(), 0, "not due yet");
    clock.advance(time::Duration::days(21));
    assert_eq!(s.domains.renew().await.unwrap(), 1);
    let second = s
        .domains
        .certificate_expiry("renew.localhost")
        .await
        .unwrap();
    assert!(second - first >= 20 * 86_400, "{first} → {second}");
    // Upkeep after a wake renews too and keeps serving.
    s.domains.upkeep(true, true).await;
    assert!(s.domains.status().await.running);
}

#[tokio::test]
async fn trust_is_recorded_and_forgetting_makes_a_new_authority() {
    let s = setup();
    let before = s.domains.trust_status().await;
    assert!(!before.trusted && before.ca.is_none(), "no CA until needed");
    let trusted = s.domains.trust(TrustOptions::default()).await.unwrap();
    assert!(trusted.trusted);
    let first = trusted.ca.unwrap().sha256;
    assert!(s.domains.trust_status().await.trusted);
    let untrusted = s.domains.untrust(false).await.unwrap();
    assert!(!untrusted.trusted);
    assert_eq!(untrusted.ca.unwrap().sha256, first, "the CA is kept");
    s.domains.untrust(true).await.unwrap();
    let again = s.domains.trust(TrustOptions::default()).await.unwrap();
    assert_ne!(again.ca.unwrap().sha256, first);
}

#[tokio::test]
async fn doctor_flags_untrusted_and_a_taken_port_then_clears() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken_port = taken.local_addr().unwrap().port();
    let s = setup_with(Store::open_in_memory().unwrap(), |c| {
        c.options.https_port = taken_port;
        c.options.fallback_https_port = 0;
    });
    let port = origin("d").await;
    s.domains
        .add(&input("doc.localhost", &port.to_string()))
        .await
        .unwrap();
    let status = s.domains.status().await;
    assert!(status.running);
    let problem = status.port_problems.first().unwrap();
    assert_eq!(problem.port, taken_port);
    assert_eq!(problem.reason, PortReason::InUse);
    assert!(problem.fallback.is_some());
    let issues = s.domains.doctor().await;
    let checks: Vec<&str> = issues.iter().map(|i| i.check.as_str()).collect();
    assert!(checks.contains(&"local.port"), "{checks:?}");
    assert!(checks.contains(&"local.untrusted"), "{checks:?}");
    s.domains.fix(LocalDomainFix::Trust).await.unwrap();
    let checks: Vec<String> = s
        .domains
        .doctor()
        .await
        .into_iter()
        .map(|i| i.check)
        .collect();
    assert!(!checks.iter().any(|c| c == "local.untrusted"), "{checks:?}");
    // Once the other app lets go of the port, Restart moves back to it.
    drop(taken);
    s.domains.fix(LocalDomainFix::Restart).await.unwrap();
    let status = s.domains.status().await;
    assert_eq!(status.https_port, Some(taken_port));
    assert!(status.port_problems.is_empty());
}

#[tokio::test]
async fn connections_from_other_machines_are_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = CheckedPlain {
        policy: PeerPolicy::default(),
    };
    let accept = |peer: SocketAddr| {
        let acceptor = acceptor.clone();
        let listener = &listener;
        async move {
            let _client = TcpStream::connect(addr).await.unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            acceptor.accept(stream, peer).await.map(|_| ())
        }
    };
    assert!(accept("127.0.0.1:5000".parse().unwrap()).await.is_ok());
    assert!(accept("[::1]:5000".parse().unwrap()).await.is_ok());
    let refused = accept("192.168.1.77:5000".parse().unwrap())
        .await
        .unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(accept("203.0.113.5:5000".parse().unwrap()).await.is_err());
}

#[tokio::test]
async fn names_are_not_checked_when_lookups_are_off() {
    let s = setup();
    let port = origin("n").await;
    s.domains
        .add(&input("n.test", &port.to_string()))
        .await
        .unwrap();
    let status = s.domains.status().await;
    assert_eq!(status.domains[0].resolution, NameResolution::Unchecked);
    assert!(
        !s.domains
            .doctor()
            .await
            .iter()
            .any(|i| i.check == "local.resolver")
    );
    assert!(
        !status.resolver.setup.is_empty(),
        "the one-time steps are shown"
    );
}
