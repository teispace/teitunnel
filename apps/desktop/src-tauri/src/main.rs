// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    match teitunnel_desktop::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("teitunnel failed to start: {err}");
            }
            ExitCode::FAILURE
        }
    }
}
