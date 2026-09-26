//! Comments on shares and Snapshots.
//!
//! Reviewers pin comments to a spot on a page with a small overlay script
//! ([`OVERLAY_JS`]), reply and resolve. Where they're kept depends on what's shared:
//!
//! - **Live shares and routes** (anything going through Lens): in this computer's
//!   database. Lens injects the overlay into HTML pages and answers the same-origin API
//!   under `/__teitunnel/comments/` itself ([`serve::CommentsHandler`]), so nothing
//!   leaves the machine, it works on Quick Shares without an account, and it costs no
//!   Worker requests. The share is only reachable while this computer is on anyway.
//! - **Snapshots**: in a D1 database on the user's Cloudflare account, written by the
//!   Snapshot's Worker (the Worker answers the same API) and read here with the
//!   account's token through the D1 query endpoint ([`remote`]).
//!
//! Both speak the same JSON ([`Thread`]) and apply the same limits, so the overlay
//! doesn't know which one it talks to. Text is stored as typed and only ever rendered
//! as text (`textContent` in the overlay, React text in the app).

mod local;
pub mod remote;
pub mod serve;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

pub use local::MAX_COMMENTS_PER_SUBJECT;

use crate::{
    store::{Store, StoreError},
    text::{Text, UserText, english_display, msg},
};

/// The overlay script, served at [`OVERLAY_PATH`].
pub const OVERLAY_JS: &str = include_str!("comments/overlay.js");
/// Where the overlay and its API live on a shared site.
pub const BASE_PATH: &str = "/__teitunnel/comments/";
/// The overlay's address on a shared site.
pub const OVERLAY_PATH: &str = "/__teitunnel/comments/overlay.js";
/// What Lens adds to HTML pages.
pub const SNIPPET: &str = "<script src=\"/__teitunnel/comments/overlay.js\" defer></script>";

/// Longest comment, in characters.
pub const MAX_BODY: usize = 4000;
/// Longest reviewer name, in characters.
pub const MAX_NAME: usize = 80;
/// Longest page path.
pub const MAX_PATH: usize = 1024;
/// Longest element selector.
pub const MAX_SELECTOR: usize = 512;
/// Most comments in one thread (the first one included).
pub const MAX_PER_THREAD: usize = 200;

/// What a set of comments is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum SubjectKind {
    /// A Quick Share (a new address each time it starts).
    QuickShare,
    /// A route or a share on your domain.
    Route,
    /// A Snapshot (kept on Cloudflare).
    Snapshot,
}

impl SubjectKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::QuickShare => "quickShare",
            Self::Route => "route",
            Self::Snapshot => "snapshot",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "quickShare" => Some(Self::QuickShare),
            "route" => Some(Self::Route),
            "snapshot" => Some(Self::Snapshot),
            _ => None,
        }
    }
}

/// A share, route or Snapshot that has comments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    /// Stable key: `share:<id>`, `route:<account>:<hostname>` or `snapshot:<id>`.
    pub key: String,
    /// What it is.
    pub kind: SubjectKind,
    /// The account (none for a Quick Share).
    pub account_id: Option<String>,
    /// What people see, e.g. the hostname.
    pub label: String,
    /// The address to open.
    pub url: Option<String>,
}

impl Subject {
    /// A Quick Share.
    pub fn quick_share(share_id: &str, url: Option<&str>) -> Self {
        Self {
            key: format!("share:{share_id}"),
            kind: SubjectKind::QuickShare,
            account_id: None,
            label: url
                .map(|u| u.trim_start_matches("https://").to_owned())
                .unwrap_or_else(|| share_id.to_owned()),
            url: url.map(str::to_owned),
        }
    }

    /// A route (or a share on your domain).
    pub fn route(account_id: &str, hostname: &str) -> Self {
        let hostname = hostname.to_ascii_lowercase();
        Self {
            key: format!("route:{account_id}:{hostname}"),
            kind: SubjectKind::Route,
            account_id: Some(account_id.to_owned()),
            url: Some(format!("https://{hostname}")),
            label: hostname,
        }
    }

    /// A Snapshot.
    pub fn snapshot(snapshot_id: &str, account_id: &str, name: &str, url: &str) -> Self {
        Self {
            key: format!("snapshot:{snapshot_id}"),
            kind: SubjectKind::Snapshot,
            account_id: Some(account_id.to_owned()),
            label: name.to_owned(),
            url: Some(url.to_owned()),
        }
    }
}

/// Where on a page a thread is pinned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    /// CSS selector of the element clicked.
    pub selector: String,
    /// Horizontal position inside the element (0–1).
    pub x: f64,
    /// Vertical position inside the element (0–1).
    pub y: f64,
    /// Page coordinates, used when the element can't be found.
    pub left: f64,
    /// Page coordinates, used when the element can't be found.
    pub top: f64,
    /// The reviewer's viewport width.
    pub vw: u32,
    /// The reviewer's viewport height.
    pub vh: u32,
}

/// One comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    /// Id.
    pub id: String,
    /// Who wrote it.
    pub author: String,
    /// Their email, when Cloudflare Access vouched for it (only shown to the owner).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Signed in with Cloudflare Access.
    pub verified: bool,
    /// Written by the owner (from the app, the CLI or an agent).
    pub by_owner: bool,
    /// The text, as typed.
    pub body: String,
    /// When (ms since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: u64,
}

/// A thread: the first comment, its replies and whether it's resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// Id (the first comment's).
    pub id: String,
    /// The page, e.g. `/pricing`.
    pub path: String,
    /// Where it's pinned (none: the whole page).
    pub anchor: Option<Anchor>,
    /// Resolved.
    pub resolved: bool,
    /// Who resolved it.
    pub resolved_by: Option<String>,
    /// When.
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub resolved_at: Option<u64>,
    /// When it started.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: u64,
    /// Comments, oldest first.
    pub comments: Vec<Comment>,
}

impl Thread {
    /// The same thread without email addresses (what reviewers get).
    #[must_use]
    pub fn public(mut self) -> Self {
        for comment in &mut self.comments {
            comment.email = None;
        }
        self
    }

    /// When the newest comment was written.
    pub fn latest(&self) -> u64 {
        self.comments
            .iter()
            .map(|c| c.created_at)
            .max()
            .unwrap_or(self.created_at)
    }
}

/// A subject with its counts, for the app's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SubjectView {
    /// The subject.
    #[serde(flatten)]
    pub subject: Subject,
    /// Unresolved threads.
    pub open: u32,
    /// Comments.
    pub comments: u32,
    /// Comments written after the owner last looked.
    pub unread: u32,
    /// Newest comment (ms).
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub latest_at: Option<u64>,
}

/// Who writes a comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    /// Display name.
    pub name: String,
    /// Access-verified email.
    pub email: Option<String>,
    /// Access vouched for them.
    pub verified: bool,
    /// The owner.
    pub by_owner: bool,
}

impl Author {
    /// A reviewer who typed their name.
    ///
    /// # Errors
    /// [`CommentsError::InvalidName`].
    pub fn reviewer(name: &str) -> Result<Self, CommentsError> {
        Ok(Self {
            name: clean_name(name)?,
            email: None,
            verified: false,
            by_owner: false,
        })
    }

    /// A reviewer Cloudflare Access signed in (their typed name wins over the address).
    pub fn verified(email: &str, name: Option<&str>) -> Self {
        let email: String = email.trim().chars().take(254).collect();
        let name = name
            .and_then(|n| clean_name(n).ok())
            .unwrap_or_else(|| email.chars().take(MAX_NAME).collect());
        Self {
            name,
            email: Some(email),
            verified: true,
            by_owner: false,
        }
    }

    /// The owner (the app, the CLI, an agent acting for them).
    pub fn owner(name: &str) -> Self {
        Self {
            name: clean_name(name).unwrap_or_else(|_| "Owner".to_owned()),
            email: None,
            verified: false,
            by_owner: true,
        }
    }
}

/// Why comments couldn't be read or written. Messages are shown to the person (and,
/// through the overlay, to reviewers: they never contain internals).
#[derive(Debug, thiserror::Error)]
pub enum CommentsError {
    /// Empty or too long.
    InvalidBody,
    /// Missing or too long.
    InvalidName,
    /// Not a path on the site.
    InvalidPath,
    /// A malformed spot.
    InvalidAnchor,
    /// The subject or thread is full.
    TooMany,
    /// No such thread (or subject).
    NotFound,
    /// Too many comments from one visitor.
    RateLimited,
    /// A Snapshot published without comments.
    NoDatabase,
    /// Comments are off for this share.
    Disabled,
    /// Snapshot comments need the account connected.
    NoAccount,
    /// The database failed.
    Store(#[from] StoreError),
    /// Cloudflare refused or couldn't be reached.
    Api(#[from] cf_api::Error),
}

impl UserText for CommentsError {
    fn text(&self) -> Text {
        use msg::comments::error as m;
        match self {
            Self::InvalidBody => m::invalid_body(MAX_BODY as u64),
            Self::InvalidName => m::invalid_name(MAX_NAME as u64),
            Self::InvalidPath => m::invalid_path(),
            Self::InvalidAnchor => m::invalid_anchor(),
            Self::TooMany => m::too_many(),
            Self::NotFound => m::not_found(),
            Self::RateLimited => m::rate_limited(),
            Self::NoDatabase => m::no_database(),
            Self::Disabled => m::disabled(),
            Self::NoAccount => m::no_account(),
            Self::Store(err) => err.text(),
            Self::Api(err) => m::cloudflare(err.detail()),
        }
    }
}

english_display!(CommentsError);

fn has_control(text: &str, allow_newlines: bool) -> bool {
    text.chars().any(|c| {
        (c.is_control() && !(allow_newlines && matches!(c, '\n' | '\t')))
            || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    })
}

/// A comment as stored: trimmed, 1–[`MAX_BODY`] characters, no control characters
/// except newlines and tabs (`\r\n` becomes `\n`), no bidirectional overrides.
///
/// # Errors
/// [`CommentsError::InvalidBody`].
/// Turns comments on or off for a tap's share or route. Commenters' emails come from
/// the Access login header only on a route whose login Teitunnel made (so the header is
/// Cloudflare's, not a visitor's).
///
/// # Errors
/// Unknown tap; the inspector refused.
pub async fn set_on_tap(
    inspector: &crate::inspect::Inspector,
    local: &crate::engine::Local,
    tap: &crate::inspect::lens::TapId,
    on: bool,
) -> Result<crate::inspect::TapView, crate::inspect::InspectError> {
    use crate::inspect::TapScope;
    let view = inspector.view(tap)?;
    let trust = match &view.scope {
        TapScope::Route {
            account_id,
            hostname,
            ..
        } => local
            .owned_access_apps(account_id)
            .await
            .unwrap_or_default()
            .iter()
            .any(|(_, domain)| domain.split('/').next() == Some(hostname.as_str())),
        TapScope::QuickShare { .. } | TapScope::LocalDomain { .. } => false,
    };
    inspector.set_comments(tap, on, trust).await
}

pub fn clean_body(body: &str) -> Result<String, CommentsError> {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    let body = body.trim();
    if body.is_empty() || body.chars().count() > MAX_BODY || has_control(body, true) {
        return Err(CommentsError::InvalidBody);
    }
    Ok(body.to_owned())
}

/// A name as stored: trimmed, 1–[`MAX_NAME`] characters, one line.
///
/// # Errors
/// [`CommentsError::InvalidName`].
pub fn clean_name(name: &str) -> Result<String, CommentsError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || has_control(name, false) {
        return Err(CommentsError::InvalidName);
    }
    Ok(name.to_owned())
}

/// A page path: starts with `/`, at most [`MAX_PATH`] bytes, no query or fragment
/// (they're dropped), no control characters.
///
/// # Errors
/// [`CommentsError::InvalidPath`].
pub fn clean_path(path: &str) -> Result<String, CommentsError> {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.len() > MAX_PATH
        || has_control(path, false)
        || path.contains('\\')
    {
        return Err(CommentsError::InvalidPath);
    }
    Ok(path.to_owned())
}

/// A spot as stored: a selector of at most [`MAX_SELECTOR`] characters, positions
/// clamped into range, viewport sizes capped.
///
/// # Errors
/// [`CommentsError::InvalidAnchor`] for non-finite numbers or a bad selector.
pub fn clean_anchor(anchor: &Anchor) -> Result<Anchor, CommentsError> {
    let selector = anchor.selector.trim();
    let numbers = [anchor.x, anchor.y, anchor.left, anchor.top];
    if selector.chars().count() > MAX_SELECTOR
        || has_control(selector, false)
        || numbers.iter().any(|n| !n.is_finite())
    {
        return Err(CommentsError::InvalidAnchor);
    }
    Ok(Anchor {
        selector: selector.to_owned(),
        x: anchor.x.clamp(0.0, 1.0),
        y: anchor.y.clamp(0.0, 1.0),
        left: anchor.left.clamp(0.0, 10_000_000.0).round(),
        top: anchor.top.clamp(0.0, 10_000_000.0).round(),
        vw: anchor.vw.min(100_000),
        vh: anchor.vh.min(100_000),
    })
}

/// A new id: 16 hex characters from the OS generator (`c` + 15).
pub(crate) fn new_id() -> String {
    let mut bytes = [0u8; 8];
    // The OS generator doesn't fail in practice; a time-based id is only a fallback.
    if getrandom::fill(&mut bytes).is_err() {
        bytes = crate::domain_shares::now_ms().to_le_bytes();
    }
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("c{}", &hex[1..])
}

/// One stored comment row (the same columns locally and in D1).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub id: String,
    pub thread: String,
    pub path: String,
    pub anchor: Option<String>,
    pub author: String,
    pub email: Option<String>,
    pub verified: bool,
    pub by_owner: bool,
    pub body: String,
    pub created_at: u64,
    pub resolved_at: Option<u64>,
    pub resolved_by: Option<String>,
}

/// Groups rows (any order) into threads, oldest thread first. Replies whose first
/// comment is missing are dropped.
pub(crate) fn threads_from(mut rows: Vec<Row>) -> Vec<Thread> {
    rows.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    let mut threads: BTreeMap<String, Thread> = BTreeMap::new();
    let mut order = Vec::new();
    let mut replies = Vec::new();
    for row in rows {
        let comment = Comment {
            id: row.id.clone(),
            author: row.author.clone(),
            email: row.email.clone(),
            verified: row.verified,
            by_owner: row.by_owner,
            body: row.body.clone(),
            created_at: row.created_at,
        };
        if row.id == row.thread {
            order.push(row.id.clone());
            threads.insert(
                row.id.clone(),
                Thread {
                    id: row.id,
                    path: row.path,
                    anchor: row.anchor.and_then(|a| serde_json::from_str(&a).ok()),
                    resolved: row.resolved_at.is_some(),
                    resolved_by: row.resolved_by,
                    resolved_at: row.resolved_at,
                    created_at: row.created_at,
                    comments: vec![comment],
                },
            );
        } else {
            replies.push((row.thread, comment));
        }
    }
    for (thread, comment) in replies {
        if let Some(thread) = threads.get_mut(&thread) {
            thread.comments.push(comment);
        }
    }
    order
        .into_iter()
        .filter_map(|id| threads.remove(&id))
        .collect()
}

/// Something that happened to comments this process keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentsEvent {
    /// A reviewer wrote a comment (a new thread or a reply).
    New {
        /// The subject.
        subject: Subject,
        /// The thread.
        thread: String,
        /// Who.
        author: String,
        /// The start of what they wrote.
        excerpt: String,
    },
    /// Threads changed (resolved, answered by the owner, new comments on Cloudflare).
    Changed {
        /// The subject's key.
        subject: String,
    },
}

/// A comment's first line, shortened for notifications.
pub fn excerpt(body: &str) -> String {
    let line = body.lines().next().unwrap_or_default();
    let mut out: String = line.chars().take(120).collect();
    if line.chars().count() > 120 || body.lines().nth(1).is_some() {
        out.push('…');
    }
    out
}

/// Remembers a subject in `store` (so the app lists it before its first comment).
///
/// # Errors
/// Database errors.
pub async fn register_subject(store: &Store, subject: &Subject) -> Result<(), CommentsError> {
    local::register(store, subject).await
}

/// Forgets a subject and the comments kept here for it.
///
/// # Errors
/// Database errors.
pub async fn forget_subject(store: &Store, key: &str) -> Result<(), CommentsError> {
    local::forget(store, key).await
}

/// Comments this process keeps (live shares) and the notifications about them. Cheap
/// to clone.
#[derive(Debug, Clone)]
pub struct Comments {
    store: Store,
    events: broadcast::Sender<CommentsEvent>,
    limiter: Arc<serve::Limiter>,
}

impl Comments {
    /// Comments kept in `store`.
    pub fn new(store: Store) -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            store,
            events,
            limiter: Arc::new(serve::Limiter::default()),
        }
    }

    /// The database.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// New comments and changes, as they happen.
    pub fn subscribe(&self) -> broadcast::Receiver<CommentsEvent> {
        self.events.subscribe()
    }

    pub(crate) fn emit(&self, event: CommentsEvent) {
        let _ = self.events.send(event);
    }

    /// Remembers a subject (so the app lists it before its first comment).
    ///
    /// # Errors
    /// Database errors.
    pub async fn register(&self, subject: &Subject) -> Result<(), CommentsError> {
        local::register(&self.store, subject).await
    }

    /// Every subject with comments or registered, newest activity first.
    ///
    /// # Errors
    /// Database errors.
    pub async fn subjects(&self) -> Result<Vec<SubjectView>, CommentsError> {
        local::subjects(&self.store).await
    }

    /// One subject.
    ///
    /// # Errors
    /// Database errors.
    pub async fn subject(&self, key: &str) -> Result<Option<Subject>, CommentsError> {
        local::subject(&self.store, key).await
    }

    /// Threads of a live share or route kept here (all pages when `path` is `None`).
    ///
    /// # Errors
    /// Database errors.
    pub async fn local_threads(
        &self,
        subject: &str,
        path: Option<&str>,
    ) -> Result<Vec<Thread>, CommentsError> {
        local::threads(&self.store, subject, path).await
    }

    /// Starts a thread on a live share or route.
    ///
    /// # Errors
    /// Invalid input, a full subject, database errors.
    pub async fn local_start(
        &self,
        subject: &Subject,
        path: &str,
        anchor: Option<&Anchor>,
        body: &str,
        author: &Author,
    ) -> Result<Thread, CommentsError> {
        let thread = local::start(&self.store, subject, path, anchor, body, author).await?;
        self.after_write(subject, &thread, author);
        Ok(thread)
    }

    /// Replies on a live share or route.
    ///
    /// # Errors
    /// Invalid input, an unknown or full thread, database errors.
    pub async fn local_reply(
        &self,
        subject: &Subject,
        thread: &str,
        body: &str,
        author: &Author,
    ) -> Result<Thread, CommentsError> {
        let thread = local::reply(&self.store, subject, thread, body, author).await?;
        self.after_write(subject, &thread, author);
        Ok(thread)
    }

    /// Resolves or reopens a thread on a live share or route.
    ///
    /// # Errors
    /// An unknown thread, database errors.
    pub async fn local_resolve(
        &self,
        subject: &Subject,
        thread: &str,
        resolved: bool,
        by: &str,
    ) -> Result<Thread, CommentsError> {
        let thread = local::resolve(&self.store, &subject.key, thread, resolved, by).await?;
        self.emit(CommentsEvent::Changed {
            subject: subject.key.clone(),
        });
        Ok(thread)
    }

    fn after_write(&self, subject: &Subject, thread: &Thread, author: &Author) {
        if author.by_owner {
            self.emit(CommentsEvent::Changed {
                subject: subject.key.clone(),
            });
            return;
        }
        if let Some(comment) = thread.comments.last() {
            self.emit(CommentsEvent::New {
                subject: subject.clone(),
                thread: thread.id.clone(),
                author: comment.author.clone(),
                excerpt: excerpt(&comment.body),
            });
        }
    }

    /// Marks everything in a subject as read.
    ///
    /// # Errors
    /// Database errors.
    pub async fn mark_seen(&self, key: &str) -> Result<(), CommentsError> {
        local::mark_seen(&self.store, key).await
    }

    /// Records a Snapshot's counts as read from Cloudflare; returns whether a reviewer
    /// wrote something since the last notification.
    ///
    /// # Errors
    /// Database errors.
    pub async fn record_remote(
        &self,
        key: &str,
        counts: remote::RemoteCounts,
    ) -> Result<bool, CommentsError> {
        local::record_remote(&self.store, key, counts).await
    }

    /// When the owner last looked at a subject (ms; 0: never).
    ///
    /// # Errors
    /// Database errors.
    pub async fn seen_at(&self, key: &str) -> Result<u64, CommentsError> {
        local::seen_at(&self.store, key).await
    }

    /// Forgets a subject and the comments kept here for it.
    ///
    /// # Errors
    /// Database errors.
    pub async fn forget(&self, key: &str) -> Result<(), CommentsError> {
        local::forget(&self.store, key).await
    }

    pub(crate) fn limiter(&self) -> &serve::Limiter {
        &self.limiter
    }

    /// A Snapshot subject's account, database and Worker (its comments' site).
    async fn remote_site(
        &self,
        subject: &Subject,
    ) -> Result<(String, String, String), CommentsError> {
        let local = crate::engine::Local::new(self.store.clone());
        let id = subject
            .key
            .strip_prefix("snapshot:")
            .ok_or(CommentsError::NotFound)?;
        let row = local.site(id).await?.ok_or(CommentsError::NotFound)?;
        let database = local
            .cloud_database(&row.account_id)
            .await?
            .ok_or(CommentsError::NoDatabase)?;
        Ok((row.account_id, database, row.script))
    }

    async fn known(&self, key: &str) -> Result<Subject, CommentsError> {
        self.subject(key).await?.ok_or(CommentsError::NotFound)
    }

    /// Every thread of a subject, with reviewers' verified addresses (for the owner).
    /// Snapshot comments are read from Cloudflare with `api` (the subject's account).
    ///
    /// # Errors
    /// Unknown subject, API or database errors.
    pub async fn threads<C: crate::engine::CloudApi>(
        &self,
        api: Option<&C>,
        key: &str,
    ) -> Result<Vec<Thread>, CommentsError> {
        let subject = self.known(key).await?;
        if subject.kind != SubjectKind::Snapshot {
            return self.local_threads(key, None).await;
        }
        let api = api.ok_or(CommentsError::NoAccount)?;
        let (account, database, site) = self.remote_site(&subject).await?;
        remote::threads(api, &account, &database, &site).await
    }

    /// The owner's reply.
    ///
    /// # Errors
    /// Invalid text, unknown subject or thread, API or database errors.
    pub async fn reply<C: crate::engine::CloudApi>(
        &self,
        api: Option<&C>,
        key: &str,
        thread: &str,
        body: &str,
        author: &Author,
    ) -> Result<Thread, CommentsError> {
        let subject = self.known(key).await?;
        let thread = if subject.kind == SubjectKind::Snapshot {
            let api = api.ok_or(CommentsError::NoAccount)?;
            let (account, database, site) = self.remote_site(&subject).await?;
            remote::reply(api, &account, &database, &site, thread, body, author).await?
        } else {
            self.local_reply(&subject, thread, body, author).await?
        };
        self.emit(CommentsEvent::Changed {
            subject: subject.key,
        });
        Ok(thread)
    }

    /// Resolves or reopens a thread.
    ///
    /// # Errors
    /// Unknown subject or thread, API or database errors.
    pub async fn resolve<C: crate::engine::CloudApi>(
        &self,
        api: Option<&C>,
        key: &str,
        thread: &str,
        resolved: bool,
        by: &str,
    ) -> Result<Thread, CommentsError> {
        let subject = self.known(key).await?;
        if subject.kind == SubjectKind::Snapshot {
            let api = api.ok_or(CommentsError::NoAccount)?;
            let (account, database, site) = self.remote_site(&subject).await?;
            let thread =
                remote::resolve(api, &account, &database, &site, thread, resolved, by).await?;
            self.emit(CommentsEvent::Changed {
                subject: subject.key,
            });
            return Ok(thread);
        }
        self.local_resolve(&subject, thread, resolved, by).await
    }

    /// Reads the counts of `account`'s Snapshots with comments from Cloudflare (one
    /// query) and records them; returns the subjects with comments a reviewer wrote
    /// since the last time, and how many are unread (for notifications).
    ///
    /// # Errors
    /// API or database errors.
    pub async fn poll_snapshots<C: crate::engine::CloudApi>(
        &self,
        api: &C,
        account: &str,
    ) -> Result<Vec<(Subject, u32)>, CommentsError> {
        let local = crate::engine::Local::new(self.store.clone());
        let Some(database) = local.cloud_database(account).await? else {
            return Ok(Vec::new());
        };
        let subjects: Vec<Subject> = self
            .subjects()
            .await?
            .into_iter()
            .map(|v| v.subject)
            .filter(|s| s.kind == SubjectKind::Snapshot && s.account_id.as_deref() == Some(account))
            .collect();
        let mut sites = BTreeMap::new();
        let mut by_site = BTreeMap::new();
        for subject in subjects {
            let Some(id) = subject.key.strip_prefix("snapshot:") else {
                continue;
            };
            let Some(row) = local.site(id).await? else {
                continue;
            };
            sites.insert(row.script.clone(), self.seen_at(&subject.key).await?);
            by_site.insert(row.script, subject);
        }
        let counts = remote::counts(api, account, &database, &sites).await?;
        let mut news = Vec::new();
        for (site, subject) in by_site {
            let found = counts.get(&site).copied().unwrap_or_default();
            if self.record_remote(&subject.key, found).await? {
                news.push((subject.clone(), found.unread.max(1)));
                self.emit(CommentsEvent::Changed {
                    subject: subject.key,
                });
            }
        }
        Ok(news)
    }
}
