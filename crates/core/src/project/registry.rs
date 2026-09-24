//! The projects this computer knows (opened in the app or applied from a terminal), kept
//! in the settings table (`projects`), with the routes each one created, so `project
//! down --remove-routes` removes only those.

use serde::{Deserialize, Serialize};

use crate::{
    settings,
    store::{Store, StoreError},
};

const KEY: &str = "projects";

/// A route a project created (it wasn't there before the project was applied).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CreatedRoute {
    /// Account id.
    pub account_id: String,
    /// Hostname.
    pub hostname: String,
    /// Path rule.
    pub path: Option<String>,
}

/// A known project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    /// The project file's path.
    pub path: String,
    /// The project's name.
    pub name: String,
    /// When it was added (ms since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub added_at: u64,
    /// When it was last applied.
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub applied_at: Option<u64>,
    /// Routes it created.
    #[serde(default)]
    pub created_routes: Vec<CreatedRoute>,
}

async fn change<T: Send + 'static>(
    store: &Store,
    f: impl FnOnce(&mut Vec<ProjectEntry>) -> T + Send + 'static,
) -> Result<T, StoreError> {
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            let mut list: Vec<ProjectEntry> = settings::read(&tx, KEY)?.unwrap_or_default();
            let out = f(&mut list);
            list.sort_by_key(|a| a.name.to_lowercase());
            settings::write(&tx, KEY, &list)?;
            tx.commit()?;
            Ok(out)
        })
        .await
}

/// Every known project, by name.
///
/// # Errors
/// Database errors.
pub async fn list(store: &Store) -> Result<Vec<ProjectEntry>, StoreError> {
    store
        .call(|conn| Ok(settings::read::<Vec<ProjectEntry>>(conn, KEY)?.unwrap_or_default()))
        .await
}

/// Remembers a project (no change if known; its name is updated).
///
/// # Errors
/// Database errors.
pub async fn remember(store: &Store, path: &str, name: &str) -> Result<(), StoreError> {
    let (path, name) = (path.to_owned(), name.to_owned());
    change(store, move |list| {
        match list.iter_mut().find(|p| p.path == path) {
            Some(entry) => entry.name = name,
            None => list.push(ProjectEntry {
                path,
                name,
                added_at: crate::domain_shares::now_ms(),
                applied_at: None,
                created_routes: Vec::new(),
            }),
        }
    })
    .await
}

/// Forgets a project (nothing it made is changed).
///
/// # Errors
/// Database errors.
pub async fn forget(store: &Store, path: &str) -> Result<(), StoreError> {
    let path = path.to_owned();
    change(store, move |list| list.retain(|p| p.path != path)).await
}

/// Records an apply and the routes it created.
///
/// # Errors
/// Database errors.
pub async fn applied(
    store: &Store,
    path: &str,
    name: &str,
    created: Vec<CreatedRoute>,
) -> Result<(), StoreError> {
    remember(store, path, name).await?;
    let path = path.to_owned();
    change(store, move |list| {
        if let Some(entry) = list.iter_mut().find(|p| p.path == path) {
            entry.applied_at = Some(crate::domain_shares::now_ms());
            for route in created {
                if !entry.created_routes.contains(&route) {
                    entry.created_routes.push(route);
                }
            }
        }
    })
    .await
}

/// Forgets routes a project created (after removing them).
///
/// # Errors
/// Database errors.
pub async fn removed(store: &Store, path: &str, gone: Vec<CreatedRoute>) -> Result<(), StoreError> {
    let path = path.to_owned();
    change(store, move |list| {
        if let Some(entry) = list.iter_mut().find(|p| p.path == path) {
            entry.created_routes.retain(|r| !gone.contains(r));
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remembers_projects_and_what_they_created() {
        let store = Store::open_in_memory().unwrap();
        remember(&store, "/w/shop/teitunnel.yml", "shop")
            .await
            .unwrap();
        remember(&store, "/w/api/teitunnel.yml", "api")
            .await
            .unwrap();
        remember(&store, "/w/shop/teitunnel.yml", "shop")
            .await
            .unwrap();
        let route = CreatedRoute {
            account_id: "a".into(),
            hostname: "shop.example.com".into(),
            path: None,
        };
        applied(&store, "/w/shop/teitunnel.yml", "shop", vec![route.clone()])
            .await
            .unwrap();
        applied(&store, "/w/shop/teitunnel.yml", "shop", vec![route.clone()])
            .await
            .unwrap();
        let all = list(&store).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "api", "sorted by name");
        assert_eq!(all[1].created_routes, std::slice::from_ref(&route));
        assert!(all[1].applied_at.is_some());
        removed(&store, "/w/shop/teitunnel.yml", vec![route])
            .await
            .unwrap();
        forget(&store, "/w/api/teitunnel.yml").await.unwrap();
        let all = list(&store).await.unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].created_routes.is_empty());
    }
}
