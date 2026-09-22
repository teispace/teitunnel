//! Per-OS integration: data paths, process signals, service managers and binary
//! asset names.

/// The OS and architecture, e.g. `macOS 27.0 (aarch64)`, for diagnostics.
pub fn os_description() -> String {
    format!(
        "{} ({})",
        sysinfo::System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_owned()),
        std::env::consts::ARCH
    )
}
