//! Comments reviewers pinned to shares, routes and Snapshots, as a
//! [`ToolProvider`]: `comments_list`, `comments_reply` and `comments_resolve`, so an
//! agent can read feedback and close the loop. Replies and resolutions are written as
//! the owner and need the person's approval like any change; reviewers' email addresses
//! are never given to agents.

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::comments::{SubjectView, Thread};

use crate::{
    backend::{BoxFuture, SharedBackend},
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolProvider,
        ToolResult, ToolSpec, arguments,
    },
    tools::{DEFAULT_TIMEOUT, Hints, spec},
};

/// The comments tools.
pub struct CommentsTools {
    backend: SharedBackend,
}

impl std::fmt::Debug for CommentsTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommentsTools").finish_non_exhaustive()
    }
}

impl CommentsTools {
    /// The tools over `backend`.
    pub fn new(backend: SharedBackend) -> Self {
        Self { backend }
    }
}

/// What to list.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListArgs {
    /// A subject's key from a previous call (`share:…`, `route:…`, `snapshot:…`), or its
    /// label (hostname or Snapshot name). Leave out to list what has comments.
    #[serde(default)]
    subject: Option<String>,
    /// Include resolved threads (default: only open ones).
    #[serde(default)]
    include_resolved: bool,
}

/// A reply.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplyArgs {
    /// The subject's key or label.
    subject: String,
    /// The thread's id (from comments_list).
    thread: String,
    /// The reply, plain text (at most 4,000 characters).
    body: String,
    /// The person reviewed the reply and agreed (needed when this server can't ask).
    #[serde(default)]
    confirmed: bool,
}

/// A resolution.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolveArgs {
    /// The subject's key or label.
    subject: String,
    /// The thread's id.
    thread: String,
    /// `true` resolves (default), `false` reopens.
    #[serde(default = "yes")]
    resolved: bool,
    /// The person agreed (needed when this server can't ask).
    #[serde(default)]
    confirmed: bool,
}

fn yes() -> bool {
    true
}

/// A subject with its counts.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SubjectOut {
    /// Pass this as `subject`.
    key: String,
    /// `quickShare`, `route` or `snapshot`.
    kind: String,
    /// Hostname or Snapshot name.
    label: String,
    /// Its address.
    url: Option<String>,
    /// Unresolved threads.
    open: u32,
    /// Comments in all.
    comments: u32,
    /// Written since the person last looked.
    unread: u32,
}

/// One comment, without the reviewer's email address.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CommentOut {
    id: String,
    author: String,
    /// Signed in with Cloudflare Access.
    verified: bool,
    /// Written by the owner (the person, or an agent for them).
    by_owner: bool,
    body: String,
    /// When (milliseconds since the epoch).
    created_at: u64,
}

/// A thread.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ThreadOut {
    id: String,
    /// The page, e.g. `/pricing`.
    path: String,
    /// The element the reviewer pinned it to (CSS selector), if any.
    element: Option<String>,
    resolved: bool,
    /// Where it is on the page, to open in a browser.
    url: Option<String>,
    comments: Vec<CommentOut>,
}

/// What comments_list found.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ListOut {
    /// Subjects with comments (when no subject was given).
    subjects: Vec<SubjectOut>,
    /// The subject's threads (when one was given).
    threads: Vec<ThreadOut>,
}

/// What a reply or resolution did.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ChangeOut {
    /// `done`, `needsApproval` or `declined`.
    outcome: String,
    /// What happened, for the person.
    message: String,
    /// The thread afterwards (when done).
    thread: Option<ThreadOut>,
}

fn thread_out(subject: &SubjectView, thread: Thread) -> ThreadOut {
    let url = subject.subject.url.as_ref().map(|base| {
        format!(
            "{}{}#__teitunnel-comment={}",
            base.trim_end_matches('/'),
            thread.path,
            thread.id
        )
    });
    ThreadOut {
        element: thread.anchor.map(|a| a.selector),
        url,
        id: thread.id,
        path: thread.path,
        resolved: thread.resolved,
        comments: thread
            .comments
            .into_iter()
            .map(|c| CommentOut {
                id: c.id,
                author: c.author,
                verified: c.verified,
                by_owner: c.by_owner,
                body: c.body,
                created_at: c.created_at,
            })
            .collect(),
    }
}

fn specs() -> Vec<ToolSpec> {
    let write = Hints {
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
    };
    vec![
        spec::<ListArgs, ListOut>(
            "comments_list",
            "List review comments",
            "Comments reviewers pinned to pages of a share, route or Snapshot (the Teitunnel comments overlay). Without `subject`, lists what has comments with open/unread counts; with one, returns its threads (open only unless includeResolved), each with the page path, the element it's pinned to and a URL that opens the page at the spot.\n\
             \n\
             Use it to read feedback on a preview, then fix the code and answer with comments_reply / comments_resolve.\n\
             \n\
             Example: {} or {\"subject\": \"snapshot:3f2a…\"} or {\"subject\": \"preview.teispace.com\", \"includeResolved\": true}",
            ToolClass::Read,
            Hints::READ_CLOUD,
            DEFAULT_TIMEOUT,
        ),
        spec::<ReplyArgs, ChangeOut>(
            "comments_reply",
            "Reply to a review comment",
            "Adds a reply to a comment thread, shown to reviewers as the owner's. Asks the person before posting. Plain text only.\n\
             \n\
             Example: {\"subject\": \"snapshot:3f2a…\", \"thread\": \"c1a2b3\", \"body\": \"Fixed in the latest version.\"}",
            ToolClass::Change,
            write,
            DEFAULT_TIMEOUT,
        ),
        spec::<ResolveArgs, ChangeOut>(
            "comments_resolve",
            "Resolve a review comment",
            "Marks a comment thread resolved (or reopens it with resolved: false). Asks the person first.\n\
             \n\
             Example: {\"subject\": \"route:acc:app.teispace.com\", \"thread\": \"c1a2b3\"}",
            ToolClass::Change,
            Hints {
                idempotent: true,
                ..write
            },
            DEFAULT_TIMEOUT,
        ),
    ]
}

impl ToolProvider for CommentsTools {
    fn tools(&self) -> Vec<ToolSpec> {
        specs()
    }

    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: JsonObject,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            match name {
                "comments_list" => self.list(arguments).await,
                "comments_reply" => {
                    let args: ReplyArgs = self::arguments(arguments)?;
                    let subject = self.subject(&args.subject).await?;
                    let request = ApprovalRequest {
                        title: format!("Reply to a comment on {}", subject.subject.label),
                        details: format!("Reply as you:\n\n{}", args.body),
                        confirmed: args.confirmed,
                    };
                    if let Some(out) = ask(ctx, &request).await {
                        return Ok(ToolOutput::new(&out));
                    }
                    let thread = self
                        .backend
                        .comment_reply(&subject.subject.key, &args.thread, &args.body)
                        .await?;
                    Ok(ToolOutput::new(&ChangeOut {
                        outcome: "done".into(),
                        message: "Replied.".into(),
                        thread: Some(thread_out(&subject, thread)),
                    }))
                }
                "comments_resolve" => {
                    let args: ResolveArgs = self::arguments(arguments)?;
                    let subject = self.subject(&args.subject).await?;
                    let verb = if args.resolved { "Resolve" } else { "Reopen" };
                    let request = ApprovalRequest {
                        title: format!("{verb} a comment on {}", subject.subject.label),
                        details: format!("{verb} thread {}.", args.thread),
                        confirmed: args.confirmed,
                    };
                    if let Some(out) = ask(ctx, &request).await {
                        return Ok(ToolOutput::new(&out));
                    }
                    let thread = self
                        .backend
                        .comment_resolve(&subject.subject.key, &args.thread, args.resolved)
                        .await?;
                    Ok(ToolOutput::new(&ChangeOut {
                        outcome: "done".into(),
                        message: if args.resolved {
                            "Resolved.".into()
                        } else {
                            "Reopened.".into()
                        },
                        thread: Some(thread_out(&subject, thread)),
                    }))
                }
                other => Err(ToolError::new(format!("Unknown tool {other}."))),
            }
        })
    }
}

/// Asks the person; `Some` when the change must not go ahead.
async fn ask(ctx: &ToolContext, request: &ApprovalRequest) -> Option<ChangeOut> {
    match ctx.approve(request).await {
        Approval::Granted { .. } => None,
        Approval::NeedsConfirmation => Some(ChangeOut {
            outcome: "needsApproval".into(),
            message: "Nothing was posted. Show the person what you'd post and call again with \"confirmed\": true only if they agree.".into(),
            thread: None,
        }),
        Approval::Declined(why) => Some(ChangeOut {
            outcome: "declined".into(),
            message: format!("{why} Nothing was posted."),
            thread: None,
        }),
    }
}

impl CommentsTools {
    /// A subject by key or label.
    async fn subject(&self, wanted: &str) -> Result<SubjectView, ToolError> {
        let wanted = wanted.trim();
        let subjects = self.backend.comment_subjects().await?;
        subjects
            .into_iter()
            .find(|s| s.subject.key == wanted || s.subject.label.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| {
                ToolError::new(format!(
                    "Nothing called {wanted} has comments. Call comments_list without a subject to see what does."
                ))
            })
    }

    async fn list(&self, arguments: JsonObject) -> ToolResult {
        let args: ListArgs = self::arguments(arguments)?;
        let Some(wanted) = args.subject else {
            let subjects = self.backend.comment_subjects().await?;
            let count = subjects.len();
            return Ok(ToolOutput::new(&ListOut {
                subjects: subjects
                    .into_iter()
                    .map(|s| SubjectOut {
                        kind: format!("{:?}", s.subject.kind).to_ascii_lowercase(),
                        key: s.subject.key,
                        label: s.subject.label,
                        url: s.subject.url,
                        open: s.open,
                        comments: s.comments,
                        unread: s.unread,
                    })
                    .collect(),
                threads: Vec::new(),
            })
            .with_summary(format!("{count} share(s) with comments.")));
        };
        let subject = self.subject(&wanted).await?;
        let threads: Vec<ThreadOut> = self
            .backend
            .comment_threads(&subject.subject.key)
            .await?
            .into_iter()
            .filter(|t| args.include_resolved || !t.resolved)
            .map(|t| thread_out(&subject, t))
            .collect();
        let count = threads.len();
        Ok(ToolOutput::new(&ListOut {
            subjects: Vec::new(),
            threads,
        })
        .with_summary(format!("{count} thread(s) on {}.", subject.subject.label)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use teitunnel_core::comments::{Anchor, Comment};

    use super::*;
    use crate::{
        backend::SharedBackend,
        config::{Mode, Settings},
        tools::tests::{FakeBackend, actor},
    };

    fn thread() -> Thread {
        Thread {
            id: "c1".into(),
            path: "/pricing".into(),
            anchor: Some(Anchor {
                selector: "main > h1".into(),
                x: 0.5,
                y: 0.5,
                left: 1.0,
                top: 2.0,
                vw: 1000,
                vh: 800,
            }),
            resolved: false,
            resolved_by: None,
            resolved_at: None,
            created_at: 1,
            comments: vec![Comment {
                id: "c1".into(),
                author: "Ana".into(),
                email: Some("ana@xyz.com".into()),
                verified: true,
                by_owner: false,
                body: "Too small".into(),
                created_at: 1,
            }],
        }
    }

    async fn call(backend: &Arc<FakeBackend>, mode: Mode, name: &str, args: Value) -> Value {
        let shared: SharedBackend = backend.clone();
        let tools = CommentsTools::new(shared);
        let ctx = ToolContext::detached(
            Settings {
                mode,
                allow_secrets: false,
            },
            actor(),
        );
        let Value::Object(args) = args else {
            panic!("arguments must be an object")
        };
        tools
            .call(name, args, &ctx)
            .await
            .map(|out| out.structured)
            .unwrap_or_else(|e| json!({ "error": e.message }))
    }

    #[tokio::test]
    async fn lists_feedback_without_reviewers_addresses() {
        let backend = FakeBackend::new();
        backend.lock().threads = vec![thread()];
        let subjects = call(&backend, Mode::Ask, "comments_list", json!({})).await;
        assert_eq!(subjects["subjects"][0]["key"], "snapshot:s1");
        assert_eq!(subjects["subjects"][0]["open"], 1);
        let threads = call(
            &backend,
            Mode::Ask,
            "comments_list",
            json!({ "subject": "Launch" }),
        )
        .await;
        let first = &threads["threads"][0];
        assert_eq!(first["element"], "main > h1");
        assert_eq!(
            first["url"],
            "https://preview.xyz.com/pricing#__teitunnel-comment=c1"
        );
        assert_eq!(first["comments"][0]["verified"], true);
        assert!(!threads.to_string().contains("ana@xyz.com"));
        let unknown = call(
            &backend,
            Mode::Ask,
            "comments_list",
            json!({ "subject": "nope" }),
        )
        .await;
        assert!(unknown["error"].as_str().unwrap().contains("comments_list"));
    }

    #[tokio::test]
    async fn replies_and_resolutions_need_the_persons_approval() {
        let backend = FakeBackend::new();
        backend.lock().threads = vec![thread()];
        let asked = call(
            &backend,
            Mode::Ask,
            "comments_reply",
            json!({ "subject": "snapshot:s1", "thread": "c1", "body": "Fixed" }),
        )
        .await;
        assert_eq!(asked["outcome"], "needsApproval");
        assert_eq!(
            backend.lock().threads[0].comments.len(),
            1,
            "nothing posted"
        );
        let posted = call(
            &backend,
            Mode::Ask,
            "comments_reply",
            json!({ "subject": "snapshot:s1", "thread": "c1", "body": "Fixed", "confirmed": true }),
        )
        .await;
        assert_eq!(posted["outcome"], "done");
        assert_eq!(posted["thread"]["comments"][1]["byOwner"], true);
        let resolved = call(
            &backend,
            Mode::Full,
            "comments_resolve",
            json!({ "subject": "snapshot:s1", "thread": "c1" }),
        )
        .await;
        assert_eq!(resolved["thread"]["resolved"], true);
        assert!(backend.lock().threads[0].resolved);
    }

    #[test]
    fn lists_three_tools_with_classes() {
        let names: Vec<(String, ToolClass)> = specs()
            .iter()
            .map(|s| (s.tool.name.to_string(), s.class))
            .collect();
        assert_eq!(
            names,
            [
                ("comments_list".to_owned(), ToolClass::Read),
                ("comments_reply".to_owned(), ToolClass::Change),
                ("comments_resolve".to_owned(), ToolClass::Change),
            ]
        );
    }
}
