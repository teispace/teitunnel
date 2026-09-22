//! Writes the generated IPC bindings without launching the app (`pnpm bindings`).

use std::process::ExitCode;

fn main() -> ExitCode {
    match teitunnel_desktop::export_bindings() {
        Ok(path) => {
            #[allow(clippy::print_stdout)]
            {
                println!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("failed to export bindings: {err}");
            }
            ExitCode::FAILURE
        }
    }
}
