//! Test double for `cloudflared`.
//!
//! Integration tests spawn this binary in place of the real one. It gains the local
//! endpoints, JSON log output and scripted failure modes in M1 (see
//! `docs/plans/M1-binary-quick-share.md`). For now it only answers `--version`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "version") => {
            #[allow(clippy::print_stdout)]
            {
                println!("cloudflared version 2026.9.0 (built 2026-09-02-1200 UTC) [fake]");
            }
            ExitCode::SUCCESS
        }
        _ => ExitCode::from(2),
    }
}
