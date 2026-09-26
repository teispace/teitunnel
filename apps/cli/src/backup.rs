//! `teitunnel backup create|restore`: moving Teitunnel's setup to another computer in
//! one encrypted file. Tokens and passwords never go in it; accounts are connected
//! again after restoring.

use std::{
    io::{self, BufRead as _, IsTerminal as _, Write as _},
    path::PathBuf,
    process::ExitCode,
};

use clap::Subcommand;
use teitunnel_core::{
    Secret,
    backup::{self, BackupSummary, KdfParams},
    text::UserText as _,
};

use crate::context::App;

/// `teitunnel backup …`.
#[derive(Debug, Subcommand)]
pub(crate) enum BackupCommand {
    /// Write an encrypted backup of this computer's setup (settings, tunnels, routes'
    /// ownership, projects, alert rules; never tokens or passwords).
    Create {
        /// Where to write it (default: teitunnel-setup.teitunnel-backup here).
        #[arg(long, short = 'f', value_name = "PATH")]
        file: Option<PathBuf>,
        /// Read the passphrase from the first line of standard input.
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Show what a backup holds and restore it (asks before replacing anything).
    Restore {
        /// The backup file.
        file: PathBuf,
        /// Read the passphrase from the first line of standard input.
        #[arg(long)]
        passphrase_stdin: bool,
        /// Restore without asking.
        #[arg(long, short)]
        yes: bool,
    },
}

fn read_line() -> Result<String, String> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

fn prompt(question: &str) -> Result<String, String> {
    write!(io::stdout().lock(), "{question}").map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    read_line()
}

/// The passphrase: from standard input, or typed (twice for a new backup).
fn passphrase(from_stdin: bool, confirm: bool) -> Result<Secret<String>, String> {
    if from_stdin {
        return Ok(Secret::new(read_line()?));
    }
    if !io::stdin().is_terminal() {
        return Err("Not a terminal: pass --passphrase-stdin and pipe the passphrase in.".into());
    }
    let first = prompt(&format!(
        "Passphrase ({}+ characters; you'll need it on the other computer): ",
        backup::MIN_PASSPHRASE
    ))?;
    if confirm && prompt("Again: ")? != first {
        return Err("The passphrases don't match.".into());
    }
    Ok(Secret::new(first))
}

fn print_summary(summary: &BackupSummary) -> Result<(), String> {
    out!(
        "Backup of {} made by Teitunnel {}:",
        if summary.machine.is_empty() {
            "a computer"
        } else {
            summary.machine.as_str()
        },
        summary.app_version
    )?;
    for section in &summary.sections {
        let replaces = if section.existing > 0 && section.section != "settings" {
            format!(" (replaces {} here)", section.existing)
        } else {
            String::new()
        };
        out!(
            "  {:>4}  {}{replaces}",
            section.count,
            section.label.english()
        )?;
    }
    if !summary.projects.is_empty() {
        out!("  Projects: {}", summary.projects.join(", "))?;
    }
    if !summary.accounts.is_empty() {
        out!(
            "  Accounts to connect again: {}",
            summary
                .accounts
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    Ok(())
}

pub(crate) async fn run(app: &App, command: BackupCommand) -> Result<ExitCode, String> {
    match command {
        BackupCommand::Create {
            file,
            passphrase_stdin,
        } => {
            let path = file
                .unwrap_or_else(|| PathBuf::from(format!("teitunnel-setup.{}", backup::EXTENSION)));
            let passphrase = passphrase(passphrase_stdin, true)?;
            backup::create_file(
                app.store(),
                &app.machine_name,
                &path,
                passphrase,
                KdfParams::default(),
            )
            .await
            .map_err(|e| e.text().english())?;
            out!(
                "Wrote {}. It holds no tokens or passwords: on the other computer, run `teitunnel backup restore {}` (or use the app), then connect your accounts again.",
                path.display(),
                path.display()
            )?;
            Ok(ExitCode::SUCCESS)
        }
        BackupCommand::Restore {
            file,
            passphrase_stdin,
            yes,
        } => {
            let passphrase = passphrase(passphrase_stdin, false)?;
            let contents = backup::read_file(&file, passphrase)
                .await
                .map_err(|e| e.text().english())?;
            let summary = backup::summarize(app.store(), &contents)
                .await
                .map_err(|e| e.text().english())?;
            print_summary(&summary)?;
            let question = if summary.overwrites {
                "Replace this computer's setup with the backup's?"
            } else {
                "Restore?"
            };
            if !yes && !crate::confirm(question)? {
                out!("Nothing changed.")?;
                return Ok(ExitCode::SUCCESS);
            }
            backup::restore(app.store(), contents)
                .await
                .map_err(|e| e.text().english())?;
            out!("Restored.")?;
            if !summary.accounts.is_empty() {
                out!(
                    "Connect your accounts again (`teitunnel setup` or the app); the tunnels then run here with tokens fetched from Cloudflare."
                )?;
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[cfg(test)]
mod tests {
    use teitunnel_core::backup::SectionCount;

    use super::*;

    #[test]
    fn names_sections_in_words() {
        let summary = BackupSummary {
            created_at: 0,
            app_version: "0.2.0".into(),
            machine: "Teispace MacBook".into(),
            accounts: Vec::new(),
            projects: vec!["shop".into()],
            sections: vec![SectionCount {
                section: "local_tunnels".into(),
                label: teitunnel_core::text::msg::backup::section::local_tunnels(),
                count: 1,
                existing: 2,
            }],
            overwrites: true,
        };
        assert!(print_summary(&summary).is_ok());
    }
}
