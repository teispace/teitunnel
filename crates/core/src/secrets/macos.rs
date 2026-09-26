//! macOS: new keychain items that every Teitunnel program can read without a prompt.
//!
//! A login-keychain item trusts only the program that created it, so the app, the
//! `teitunnel` command and its MCP server each asked for the keychain password for
//! every item another one had stored (a new tunnel, a refreshed sign-in). Items are
//! therefore created with an access list naming all of them. Trust is recorded by code
//! signature, so it survives updates, and the command installed with Homebrew (the same
//! signed binary) is covered by the one inside the app.
//!
//! The access-list API is only in the file-based keychain's C interface, which is why
//! this is the one module allowed `unsafe`. Reads, updates and deletes go through
//! `keyring` as before.
#![allow(deprecated)] // SecAccess and SecTrustedApplication: no replacement for file-based keychains.

use std::{
    ffi::CString,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{self, NonNull},
};

use objc2_core_foundation::{CFArray, CFData, CFDictionary, CFRetained, CFString, CFType};
use objc2_security::{
    SecAccess, SecItemAdd, SecTrustedApplication, errSecDuplicateItem, errSecSuccess,
    kSecAttrAccess, kSecAttrAccount, kSecAttrLabel, kSecAttrService, kSecClass,
    kSecClassGenericPassword, kSecValueData,
};

/// The result of [`add`].
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Added {
    Yes,
    /// An item with this service and account exists; it was left as it is.
    Exists,
}

/// Adds a generic password readable by every program in `trusted` (and the caller).
///
/// # Errors
/// The keychain's status code.
pub(super) fn add(
    service: &str,
    account: &str,
    secret: &[u8],
    trusted: &[PathBuf],
) -> Result<Added, i32> {
    let apps: Vec<CFRetained<SecTrustedApplication>> = trusted
        .iter()
        .filter_map(|path| trusted_app(path))
        .collect();
    let access = access(service, &apps)?;
    let service = CFString::from_str(service);
    let account = CFString::from_str(account);
    let data = CFData::from_bytes(secret);
    // SAFETY: the keys are Security's own constant strings, valid for the process.
    let keys: [&CFType; 6] = unsafe {
        [
            kSecClass,
            kSecAttrService,
            kSecAttrAccount,
            kSecAttrLabel,
            kSecValueData,
            kSecAttrAccess,
        ]
    };
    // SAFETY: as above.
    let class: &CFType = unsafe { kSecClassGenericPassword };
    let values: [&CFType; 6] = [class, &service, &account, &service, &data, &access];
    let query = CFDictionary::from_slices(&keys, &values);
    // SAFETY: `query` holds CFString keys with values of the types SecItemAdd expects
    // for them; a null result pointer asks for nothing back.
    let status = unsafe { SecItemAdd(query.as_opaque(), ptr::null_mut()) };
    match status {
        _ if status == errSecSuccess => Ok(Added::Yes),
        _ if status == errSecDuplicateItem => Ok(Added::Exists),
        other => Err(other),
    }
}

/// An access list trusting the caller and `apps`.
fn access(
    service: &str,
    apps: &[CFRetained<SecTrustedApplication>],
) -> Result<CFRetained<SecAccess>, i32> {
    let mut list = apps.to_vec();
    // The caller itself (a null path), so the creator never has to ask either.
    list.extend(trusted_app_at(None));
    let list = CFArray::from_retained_objects(&list);
    let descriptor = CFString::from_str(service);
    let mut out: *mut SecAccess = ptr::null_mut();
    // SAFETY: `out` is a valid place for the created reference.
    let status =
        unsafe { SecAccess::create(&descriptor, Some(list.as_opaque()), NonNull::from(&mut out)) };
    match NonNull::new(out) {
        // SAFETY: SecAccessCreate returns a +1 reference we now own.
        Some(access) if status == errSecSuccess => Ok(unsafe { CFRetained::from_raw(access) }),
        _ => Err(status),
    }
}

fn trusted_app(path: &Path) -> Option<CFRetained<SecTrustedApplication>> {
    trusted_app_at(Some(&CString::new(path.as_os_str().as_bytes()).ok()?))
}

/// The trusted-application reference for the program at `path` (`None`: the caller).
fn trusted_app_at(path: Option<&CString>) -> Option<CFRetained<SecTrustedApplication>> {
    let mut out: *mut SecTrustedApplication = ptr::null_mut();
    let path = path.map_or(ptr::null(), |path| path.as_ptr());
    // SAFETY: `path` is null or a NUL-terminated string that outlives the call, and `out`
    // is a valid place for the created reference.
    let status = unsafe { SecTrustedApplication::create_from_path(path, NonNull::from(&mut out)) };
    let app = NonNull::new(out)?;
    // SAFETY: SecTrustedApplicationCreateFromPath returns a +1 reference we now own.
    let app = unsafe { CFRetained::from_raw(app) };
    (status == errSecSuccess).then_some(app)
}

/// The Teitunnel programs on this Mac that should share its keychain items: the app and
/// its bundled command (next to the running program, or in the Applications folders),
/// and the command installed on the `PATH`.
pub(super) fn programs(current: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut bundles: Vec<PathBuf> = Vec::new();
    if let Some(dir) = current.parent()
        && dir.ends_with("Contents/MacOS")
    {
        bundles.push(dir.to_path_buf());
    }
    bundles.push(PathBuf::from("/Applications/Teitunnel.app/Contents/MacOS"));
    if let Some(home) = home {
        bundles.push(home.join("Applications/Teitunnel.app/Contents/MacOS"));
    }
    let mut candidates: Vec<PathBuf> = bundles
        .iter()
        .flat_map(|dir| [dir.join("Teitunnel"), dir.join("teitunnel-cli")])
        .collect();
    candidates.push(PathBuf::from("/opt/homebrew/bin/teitunnel"));
    candidates.push(PathBuf::from("/usr/local/bin/teitunnel"));
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        // Resolves the links Homebrew and Settings ▸ Command line create.
        if let Ok(path) = candidate.canonicalize()
            && path != current
            && !found.contains(&path)
        {
            found.push(path);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_app_and_its_command_next_to_the_running_program() {
        let dir = tempfile::tempdir().unwrap();
        let macos = dir.path().join("Teitunnel.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        for name in ["Teitunnel", "teitunnel-cli"] {
            std::fs::write(macos.join(name), b"").unwrap();
        }
        let current = macos.join("teitunnel-cli").canonicalize().unwrap();
        let found = programs(&current, None);
        let app = macos.join("Teitunnel").canonicalize().unwrap();
        assert!(found.contains(&app), "{found:?}");
        assert!(
            !found.contains(&current),
            "the caller is trusted separately"
        );
    }

    #[test]
    fn a_program_outside_an_app_finds_no_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("teitunnel");
        std::fs::write(&current, b"").unwrap();
        let found = programs(&current, None);
        assert!(
            found.iter().all(|path| !path.starts_with(dir.path())),
            "{found:?}"
        );
    }

    /// Writes to the login keychain: run by hand (`--ignored`) on a Mac.
    #[test]
    #[ignore = "writes to the login keychain"]
    fn adds_once_and_reports_an_existing_item() {
        let service = format!("com.teispace.teitunnel.test.{}", std::process::id());
        let trusted = [PathBuf::from("/usr/bin/true")];
        assert_eq!(add(&service, "k", b"v1", &trusted), Ok(Added::Yes));
        assert_eq!(add(&service, "k", b"v2", &trusted), Ok(Added::Exists));
        let entry = keyring::Entry::new(&service, "k").unwrap();
        assert_eq!(entry.get_password().unwrap(), "v1");
        entry.delete_credential().unwrap();
    }
}
