//! Small file helpers: private (0600) writes and atomic replacement.

use std::{fs, io, path::Path};

/// Writes `bytes` to `path` so that only the current user can read it (0600 on Unix; on
/// Windows the file inherits the per-user ACL of the data directory). Parent directories
/// are created. The write is atomic: a temporary file in the same directory is renamed over
/// the target.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_with_mode(path, bytes, 0o600)
}

/// Like [`write_private`] with an explicit Unix mode (e.g. 0644 for a public certificate).
pub(crate) fn write_with_mode(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let mut tmp_name = file_name.to_os_string();
    tmp_name.push(format!(".tmp{}", std::process::id()));
    let tmp = parent.join(tmp_name);
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        let mut file = options.open(&tmp)?;
        io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
    }
    fs::rename(&tmp, path)
}
