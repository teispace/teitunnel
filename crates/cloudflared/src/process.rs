//! How Teitunnel starts helper programs.

use tokio::process::Command;

/// `CREATE_NO_WINDOW`: a console program started by a GUI app gets no console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Makes `command` start without a console window on Windows. Teitunnel is a GUI app,
/// so without this every connector, version check and `schtasks` call would flash a
/// console window. Nothing changes elsewhere.
pub fn no_console(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}
