//! One-click sharing from outside the window: the menu bar (tray) and the global
//! shortcut (M12-07). The shell draws the menu and registers the shortcut; what to list,
//! what a click or key press does, and the waiting all live here.

use std::time::Duration;

use serde::Serialize;

use crate::{
    control::integrations::ShortcutAction,
    discovery::{LocalService, ServiceKind},
    domain::OriginUrl,
    exposure::{self, ExposureReport},
    inspect::{Inspector, TapPatch, TapScope},
    quick_share::{HostHeaderChoice, QuickShare, QuickShares, ShareStatus},
    store::Store,
    text::{Text, UserText},
};

/// Services the menu offers at most.
pub const MENU_SERVICES: usize = 6;
/// Recent addresses the menu offers at most.
pub const MENU_RECENT: usize = 3;
/// How long a one-click share may take to get its address.
pub const LIVE_TIMEOUT: Duration = Duration::from_secs(45);

/// A local service the menu can share in one click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceItem {
    /// Its port.
    pub port: u16,
    /// What to share, e.g. `http://localhost:5173`.
    pub origin: String,
    /// `localhost:5173 — Vite · my-app` (framework names aren't translated).
    pub label: String,
}

/// Whether `share` serves the local `port`.
fn serves_port(share: &QuickShare, port: u16) -> bool {
    let origin = share.origin.as_str();
    let host = origin
        .split("://")
        .nth(1)
        .unwrap_or(origin)
        .rsplit_once(':')
        .map_or("", |(host, _)| host);
    share.origin.port() == port && matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Whether a service is something people share over HTTP (not a database or a system
/// daemon).
fn is_web(kind: ServiceKind) -> bool {
    !matches!(kind, ServiceKind::Database | ServiceKind::System)
}

fn label(service: &LocalService) -> String {
    let name = service
        .kind
        .product_name()
        .map_or_else(|| service.process.clone(), str::to_owned);
    let name = match &service.project {
        Some(project) if !name.is_empty() => format!("{name} · {project}"),
        Some(project) => project.clone(),
        None => name,
    };
    if name.is_empty() {
        format!("localhost:{}", service.port)
    } else {
        format!("localhost:{} — {name}", service.port)
    }
}

/// Running services not shared yet, dev servers first (discovery's order), at most
/// [`MENU_SERVICES`].
pub fn menu_services(services: &[LocalService], shares: &[QuickShare]) -> Vec<ServiceItem> {
    services
        .iter()
        .filter(|s| is_web(s.kind))
        .filter(|s| !shares.iter().any(|share| serves_port(share, s.port)))
        .take(MENU_SERVICES)
        .map(|s| ServiceItem {
            port: s.port,
            origin: s.origin.clone(),
            label: label(s),
        })
        .collect()
}

/// Removes services a share now serves (the menu updates without a new lookup).
pub fn drop_shared(items: &mut Vec<ServiceItem>, shares: &[QuickShare]) {
    items.retain(|item| !shares.iter().any(|share| serves_port(share, item.port)));
}

/// Addresses of live shares, newest first, at most [`MENU_RECENT`]: `(share id, URL)`.
pub fn recent_urls(shares: &[QuickShare]) -> Vec<(String, String)> {
    let mut live: Vec<&QuickShare> = shares
        .iter()
        .filter(|s| s.status == ShareStatus::Live && s.url.is_some())
        .collect();
    live.sort_by_key(|s| std::cmp::Reverse(s.started_at));
    live.into_iter()
        .take(MENU_RECENT)
        .filter_map(|s| Some((s.id.clone(), s.url.clone()?)))
        .collect()
}

/// What the global shortcut does now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutPick {
    /// Share this origin.
    Share {
        /// E.g. `http://localhost:5173`.
        origin: String,
    },
    /// It's shared already: copy its address.
    Copy {
        /// The address.
        url: String,
    },
    /// Nothing obvious (none, several, or the person asked for the sheet): open Quick
    /// Share to choose.
    Choose,
}

/// Picks what the global shortcut shares: the one running dev server (a recognised
/// framework, else a Node/Python/Ruby/PHP/Java/Go server) that isn't shared yet, or the
/// address of the one that is. With none or several, the person chooses.
pub fn pick_for_shortcut(
    action: ShortcutAction,
    services: &[LocalService],
    shares: &[QuickShare],
) -> ShortcutPick {
    if action == ShortcutAction::OpenQuickShare {
        return ShortcutPick::Choose;
    }
    let dev: Vec<&LocalService> = services.iter().filter(|s| s.kind.is_dev_server()).collect();
    let candidates = if dev.is_empty() {
        services
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    ServiceKind::Node
                        | ServiceKind::Python
                        | ServiceKind::Ruby
                        | ServiceKind::Php
                        | ServiceKind::Java
                        | ServiceKind::Go
                )
            })
            .collect()
    } else {
        dev
    };
    let [service] = candidates.as_slice() else {
        return ShortcutPick::Choose;
    };
    match shares.iter().find(|share| serves_port(share, service.port)) {
        Some(QuickShare { url: Some(url), .. }) => ShortcutPick::Copy { url: url.clone() },
        Some(_) => ShortcutPick::Choose,
        None => ShortcutPick::Share {
            origin: service.origin.clone(),
        },
    }
}

/// How waiting for a share's address ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Live {
    /// It's live, with its address.
    Ready(Box<QuickShare>),
    /// It failed.
    Failed(Text),
    /// It was stopped meanwhile.
    Gone,
    /// It took longer than allowed (it was stopped).
    TimedOut,
}

/// Waits (bounded) until a Quick Share has its address; stops it on timeout.
pub async fn wait_live(shares: &QuickShares, id: &str, timeout: Duration) -> Live {
    let mut changes = shares.subscribe();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match shares.list().into_iter().find(|s| s.id == id) {
            None => return Live::Gone,
            Some(QuickShare {
                status: ShareStatus::Failed { message },
                ..
            }) => return Live::Failed(message),
            Some(share) if share.status == ShareStatus::Live && share.url.is_some() => {
                return Live::Ready(Box::new(share));
            }
            Some(_) => {}
        }
        tokio::select! {
            _ = changes.recv() => {}
            () = tokio::time::sleep(Duration::from_millis(500)) => {}
            () = tokio::time::sleep_until(deadline) => {
                let _ = shares.stop(id).await;
                return Live::TimedOut;
            }
        }
    }
}

/// How a one-click share ended.
#[derive(Debug, Clone, PartialEq)]
pub enum ShareNow {
    /// Live: the share, with its address.
    Live(Box<QuickShare>),
    /// The exposure check found something: nothing was shared; open the sheet.
    NeedsReview(ExposureReport),
    /// It couldn't start or failed (in the person's language).
    Failed(Text),
}

/// Shares `origin` from the menu or the shortcut: runs the exposure check first when it's
/// on (D-108; findings leave the choice to the person in the window), then waits for
/// the address.
pub async fn share_now(shares: &QuickShares, store: &Store, origin: &str) -> ShareNow {
    let origin = match OriginUrl::parse(origin) {
        Ok(origin) => origin,
        Err(err) => return ShareNow::Failed(err.text()),
    };
    let check = crate::settings::load(store)
        .await
        .map_or(true, |s| s.exposure_check);
    if check {
        let report = exposure::check(origin.as_str()).await;
        if !report.is_clean() {
            return ShareNow::NeedsReview(report);
        }
    }
    let share = match shares.start(origin, None, &HostHeaderChoice::Auto).await {
        Ok(share) => share,
        Err(err) => return ShareNow::Failed(err.text()),
    };
    match wait_live(shares, &share.id, LIVE_TIMEOUT).await {
        Live::Ready(share) => ShareNow::Live(share),
        Live::Failed(message) => ShareNow::Failed(message),
        Live::Gone => ShareNow::Failed(crate::text::msg::control::error::no_url()),
        Live::TimedOut => ShareNow::Failed(crate::text::msg::control::error::url_timeout()),
    }
}

/// Whether the menu offers to pause or resume every inspected share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseAll {
    /// No share can be paused (none is inspected).
    Unavailable,
    /// Shares can be paused.
    Pause,
    /// At least one is paused.
    Resume,
}

fn quick_taps(inspector: &Inspector, shares: &[QuickShare]) -> Vec<crate::inspect::lens::TapId> {
    shares
        .iter()
        .filter(|s| s.inspected)
        .filter_map(|s| {
            inspector.tap_for(&TapScope::QuickShare {
                share_id: s.id.clone(),
            })
        })
        .collect()
}

/// What "Pause All" can do for these shares.
pub fn pause_state(inspector: &Inspector, shares: &[QuickShare]) -> PauseAll {
    let taps = quick_taps(inspector, shares);
    if taps.is_empty() {
        return PauseAll::Unavailable;
    }
    let paused = taps
        .iter()
        .any(|tap| inspector.view(tap).is_ok_and(|v| v.paused.is_some()));
    if paused {
        PauseAll::Resume
    } else {
        PauseAll::Pause
    }
}

/// Pauses (visitors see the paused page, the address stays) or resumes every inspected
/// share. Returns how many changed.
pub fn set_all_paused(inspector: &Inspector, shares: &[QuickShare], paused: bool) -> usize {
    let patch = TapPatch {
        paused: Some(paused),
        ..TapPatch::default()
    };
    quick_taps(inspector, shares)
        .iter()
        .filter(|tap| inspector.configure(tap, &patch).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(port: u16, kind: ServiceKind, project: Option<&str>) -> LocalService {
        LocalService {
            port,
            all_interfaces: false,
            pid: 1,
            process: "node".into(),
            kind,
            project: project.map(str::to_owned),
            origin: format!("http://localhost:{port}"),
        }
    }

    fn share(id: &str, origin: &str, url: Option<&str>, started_at: u64) -> QuickShare {
        QuickShare {
            id: id.into(),
            origin: OriginUrl::parse(origin).unwrap(),
            url: url.map(str::to_owned),
            status: if url.is_some() {
                ShareStatus::Live
            } else {
                ShareStatus::Starting
            },
            started_at,
            stop_at: None,
            host_header: None,
            check: None,
            inspected: false,
        }
    }

    #[test]
    fn lists_unshared_web_services_with_their_names() {
        let services = [
            service(5173, ServiceKind::Vite, Some("shop")),
            service(3000, ServiceKind::Next, None),
            service(5432, ServiceKind::Database, None),
            service(7000, ServiceKind::System, None),
            service(8080, ServiceKind::Other, None),
        ];
        let shares = [share(
            "qs-1",
            "3000",
            Some("https://a.trycloudflare.com"),
            1,
        )];
        let items = menu_services(&services, &shares);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            ["localhost:5173 — Vite · shop", "localhost:8080 — node"]
        );
        assert_eq!(items[0].origin, "http://localhost:5173");
        // A share that starts later takes its service off the menu.
        let mut before = menu_services(&services, &[]);
        assert_eq!(before.len(), 3);
        drop_shared(&mut before, &shares);
        assert_eq!(before, items);
        let many: Vec<_> = (0..10)
            .map(|i| service(4000 + i, ServiceKind::Node, None))
            .collect();
        assert_eq!(menu_services(&many, &[]).len(), MENU_SERVICES);
    }

    #[test]
    fn recent_addresses_are_live_newest_first() {
        let shares = [
            share("old", "3000", Some("https://old.trycloudflare.com"), 1),
            share("new", "3001", Some("https://new.trycloudflare.com"), 3),
            share("starting", "3002", None, 4),
            share("mid", "3003", Some("https://mid.trycloudflare.com"), 2),
            share("oldest", "3004", Some("https://x.trycloudflare.com"), 0),
        ];
        let ids: Vec<_> = recent_urls(&shares).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["new", "mid", "old"]);
    }

    #[test]
    fn the_shortcut_shares_the_one_dev_server() {
        let action = ShortcutAction::ShareDevServer;
        let vite = service(5173, ServiceKind::Vite, None);
        let node = service(4000, ServiceKind::Node, None);
        let docker = service(8080, ServiceKind::Docker, None);
        // A framework dev server wins over a plain runtime.
        assert_eq!(
            pick_for_shortcut(action, &[vite.clone(), node.clone(), docker.clone()], &[]),
            ShortcutPick::Share {
                origin: "http://localhost:5173".into()
            }
        );
        // Without one, a single runtime server.
        assert_eq!(
            pick_for_shortcut(action, &[node.clone(), docker.clone()], &[]),
            ShortcutPick::Share {
                origin: "http://localhost:4000".into()
            }
        );
        // Already shared: copy its address.
        let shared = [share(
            "qs",
            "http://127.0.0.1:5173",
            Some("https://v.trycloudflare.com"),
            1,
        )];
        assert_eq!(
            pick_for_shortcut(action, std::slice::from_ref(&vite), &shared),
            ShortcutPick::Copy {
                url: "https://v.trycloudflare.com".into()
            }
        );
        // Several, none, still starting, or asked for the sheet: choose.
        let next = service(3000, ServiceKind::Next, None);
        assert_eq!(
            pick_for_shortcut(action, &[vite.clone(), next], &[]),
            ShortcutPick::Choose
        );
        assert_eq!(
            pick_for_shortcut(action, &[docker], &[]),
            ShortcutPick::Choose
        );
        let starting = [share("qs", "5173", None, 1)];
        assert_eq!(
            pick_for_shortcut(action, std::slice::from_ref(&vite), &starting),
            ShortcutPick::Choose
        );
        assert_eq!(
            pick_for_shortcut(ShortcutAction::OpenQuickShare, &[vite], &[]),
            ShortcutPick::Choose
        );
    }

    #[test]
    fn a_share_of_another_host_is_not_this_port() {
        let remote = share(
            "qs",
            "http://192.168.1.5:5173",
            Some("https://r.trycloudflare.com"),
            1,
        );
        assert!(!serves_port(&remote, 5173));
        assert!(serves_port(&share("qs", "5173", None, 1), 5173));
    }
}
