//! `teitunnel comments`: the comments reviewers pinned to shares, routes and Snapshots.
//! Live shares' comments are in this computer's database; Snapshots' are read from the
//! account's D1 database. Replies are written as the owner (you).

use std::process::ExitCode;

use clap::Subcommand;
use teitunnel_core::comments::{Author, Comments, Subject, SubjectKind, SubjectView, Thread};

use crate::context::App;

#[derive(Debug, Subcommand)]
pub(crate) enum CommentsCommand {
    /// Without a subject, what has comments; with one (a key, hostname or Snapshot
    /// name), its open threads.
    Ls {
        /// The subject: `snapshot:…`, `route:…`, `share:…`, a hostname or a Snapshot name.
        subject: Option<String>,
        /// Include resolved threads.
        #[arg(long)]
        all: bool,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Reply to a thread (as you).
    Reply {
        /// The subject.
        subject: String,
        /// The thread's id (from `teitunnel comments ls`).
        thread: String,
        /// The reply, plain text.
        text: String,
    },
    /// Resolve a thread (or reopen it with --reopen).
    Resolve {
        /// The subject.
        subject: String,
        /// The thread's id.
        thread: String,
        /// Reopen it instead.
        #[arg(long)]
        reopen: bool,
    },
}

fn owner() -> Author {
    let label = teitunnel_core::engine::ownership::owner_label();
    Author::owner(label.split('@').next().unwrap_or(&label))
}

/// A subject by key or label.
async fn find(comments: &Comments, wanted: &str) -> Result<SubjectView, String> {
    let wanted = wanted.trim();
    comments
        .subjects()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|s| s.subject.key == wanted || s.subject.label.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| {
            format!("Nothing called \"{wanted}\" has comments. See `teitunnel comments ls`.")
        })
}

async fn api(app: &App, subject: &Subject) -> Result<Option<cf_api::Client>, String> {
    match (&subject.kind, &subject.account_id) {
        (SubjectKind::Snapshot, Some(account)) => Ok(Some(
            app.accounts
                .client(account)
                .await
                .map_err(|e| e.to_string())?,
        )),
        _ => Ok(None),
    }
}

fn kind(kind: SubjectKind) -> &'static str {
    match kind {
        SubjectKind::QuickShare => "Quick Share",
        SubjectKind::Route => "route",
        SubjectKind::Snapshot => "Snapshot",
    }
}

fn print_thread(subject: &SubjectView, thread: &Thread) -> Result<(), String> {
    let state = if thread.resolved { "resolved" } else { "open" };
    out!("{}  {}  ({state})", thread.id, thread.path)?;
    if let Some(url) = &subject.subject.url {
        out!(
            "    {}{}#__teitunnel-comment={}",
            url.trim_end_matches('/'),
            thread.path,
            thread.id
        )?;
    }
    for comment in &thread.comments {
        let who = if comment.by_owner {
            format!("{} (you)", comment.author)
        } else if comment.verified {
            format!("{} (signed in)", comment.author)
        } else {
            comment.author.clone()
        };
        out!("  {who}:")?;
        for line in comment.body.lines() {
            out!("    {line}")?;
        }
    }
    Ok(())
}

/// `teitunnel comments …`.
pub(crate) async fn run(app: &App, command: CommentsCommand) -> Result<ExitCode, String> {
    let comments = Comments::new(app.store().clone());
    match command {
        CommentsCommand::Ls {
            subject: None,
            json,
            ..
        } => {
            let subjects = comments.subjects().await.map_err(|e| e.to_string())?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string_pretty(&subjects).map_err(|e| e.to_string())?
                )?;
            } else if subjects.is_empty() {
                out!(
                    "No comments yet. Turn them on for a share in the app, or publish a Snapshot with --comments."
                )?;
            } else {
                for s in subjects {
                    let unread = if s.unread > 0 {
                        format!(", {} new", s.unread)
                    } else {
                        String::new()
                    };
                    out!(
                        "{}\t{} {}\t{} open, {} comments{unread}",
                        s.subject.key,
                        kind(s.subject.kind),
                        s.subject.label,
                        s.open,
                        s.comments
                    )?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        CommentsCommand::Ls {
            subject: Some(wanted),
            all,
            json,
        } => {
            let subject = find(&comments, &wanted).await?;
            let api = api(app, &subject.subject).await?;
            let threads: Vec<Thread> = comments
                .threads(api.as_ref(), &subject.subject.key)
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|t| all || !t.resolved)
                .collect();
            comments
                .mark_seen(&subject.subject.key)
                .await
                .map_err(|e| e.to_string())?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string_pretty(&threads).map_err(|e| e.to_string())?
                )?;
            } else if threads.is_empty() {
                out!("No open comments on {}.", subject.subject.label)?;
            } else {
                for thread in &threads {
                    print_thread(&subject, thread)?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        CommentsCommand::Reply {
            subject,
            thread,
            text,
        } => {
            let subject = find(&comments, &subject).await?;
            let api = api(app, &subject.subject).await?;
            let updated = comments
                .reply(api.as_ref(), &subject.subject.key, &thread, &text, &owner())
                .await
                .map_err(|e| e.to_string())?;
            print_thread(&subject, &updated)?;
            Ok(ExitCode::SUCCESS)
        }
        CommentsCommand::Resolve {
            subject,
            thread,
            reopen,
        } => {
            let subject = find(&comments, &subject).await?;
            let api = api(app, &subject.subject).await?;
            let updated = comments
                .resolve(
                    api.as_ref(),
                    &subject.subject.key,
                    &thread,
                    !reopen,
                    &owner().name,
                )
                .await
                .map_err(|e| e.to_string())?;
            out!(
                "{} {}.",
                if updated.resolved {
                    "Resolved"
                } else {
                    "Reopened"
                },
                updated.id
            )?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
