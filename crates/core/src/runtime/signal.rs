//! Signalling child process groups. Children are spawned in their own process group
//! (`process_group(0)`), so the group id equals the child's pid.

/// Sends SIGTERM to the process group `pid`.
#[cfg(unix)]
pub(crate) fn terminate_group(pid: u32) {
    send(pid, nix::sys::signal::Signal::SIGTERM, true);
}

/// Sends SIGKILL to the process group `pid`.
#[cfg(unix)]
pub(crate) fn kill_group(pid: u32) {
    send(pid, nix::sys::signal::Signal::SIGKILL, true);
}

/// Sends `SIGTERM` (or `SIGKILL` when `force`) to a single process.
#[cfg(unix)]
pub(crate) fn signal_process(pid: u32, force: bool) {
    let signal = if force {
        nix::sys::signal::Signal::SIGKILL
    } else {
        nix::sys::signal::Signal::SIGTERM
    };
    send(pid, signal, false);
}

#[cfg(unix)]
fn send(pid: u32, signal: nix::sys::signal::Signal, group: bool) {
    let Ok(raw) = i32::try_from(pid) else { return };
    let target = nix::unistd::Pid::from_raw(raw);
    let result = if group {
        nix::sys::signal::killpg(target, signal)
    } else {
        nix::sys::signal::kill(target, signal)
    };
    if let Err(err) = result
        && err != nix::errno::Errno::ESRCH
    {
        tracing::warn!(pid, ?signal, error = %err, "failed to signal process");
    }
}

// Windows: graceful console signals need a console attached to the child; until the
// Windows milestone (M7) connectors are stopped with TerminateProcess via
// `Child::start_kill`, and orphans with `sysinfo`.
#[cfg(not(unix))]
pub(crate) fn signal_process(pid: u32, _force: bool) {
    let mut system = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    if let Some(process) = system.process(pid) {
        process.kill();
    }
}
