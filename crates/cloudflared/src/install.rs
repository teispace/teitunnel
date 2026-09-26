//! Installing the managed cloudflared from Cloudflare's GitHub releases.
//!
//! Verification chain (all must pass before the binary is used):
//! 1. the downloaded asset matches the `sha256` digest GitHub reports for it;
//! 2. the extracted binary matches the checksum in the release notes (for `.tgz`
//!    assets Cloudflare lists the hash of the binary inside, not of the archive);
//! 3. on macOS, `codesign --verify --strict` passes and the Team ID is Cloudflare's.
//!
//! Installation is atomic: the new binary is staged, then renamed into place, and the
//! previous one is kept as `cloudflared.prev` for rollback. A failure at any step
//! leaves the current binary untouched.

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{Error, Result, Version, locate::read_version};

/// Cloudflare's Apple Developer Team ID, which signs the macOS `cloudflared` release.
pub const CLOUDFLARE_TEAM_ID: &str = "68WVV388M8";

const GITHUB_API: &str = "https://api.github.com/repos/cloudflare/cloudflared/releases/latest";
/// Upper bound for the extracted binary (decompression-bomb guard).
const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;

/// How an asset is packaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    /// A gzipped tarball containing `cloudflared` (macOS).
    Tgz,
    /// The executable itself (Linux, Windows).
    Raw,
}

/// The release asset for a platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSpec {
    /// Asset file name, e.g. `cloudflared-darwin-arm64.tgz`.
    pub name: &'static str,
    /// Packaging.
    pub kind: ArchiveKind,
}

impl AssetSpec {
    /// The asset for this machine, if Cloudflare publishes one.
    pub fn current() -> Option<Self> {
        Self::for_platform(std::env::consts::OS, std::env::consts::ARCH)
    }

    /// The asset for `os`/`arch` (Rust's `std::env::consts` names).
    pub fn for_platform(os: &str, arch: &str) -> Option<Self> {
        let (name, kind) = match (os, arch) {
            ("macos", "aarch64") => ("cloudflared-darwin-arm64.tgz", ArchiveKind::Tgz),
            ("macos", "x86_64") => ("cloudflared-darwin-amd64.tgz", ArchiveKind::Tgz),
            ("linux", "x86_64") => ("cloudflared-linux-amd64", ArchiveKind::Raw),
            ("linux", "aarch64") => ("cloudflared-linux-arm64", ArchiveKind::Raw),
            ("linux", "arm") => ("cloudflared-linux-arm", ArchiveKind::Raw),
            ("windows", "x86_64") => ("cloudflared-windows-amd64.exe", ArchiveKind::Raw),
            _ => return None,
        };
        Some(Self { name, kind })
    }
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// A downloadable asset of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// Download URL.
    pub url: String,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256 of the asset file, from GitHub, if reported.
    pub digest: Option<String>,
}

/// The latest cloudflared release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Its version.
    pub version: Version,
    /// Assets by file name.
    pub assets: HashMap<String, ReleaseAsset>,
    /// SHA-256 checksums from the release notes, by asset name.
    pub checksums: HashMap<String, String>,
}

/// Parses `name: <64 hex>` lines from release notes.
pub fn parse_checksums(body: &str) -> HashMap<String, String> {
    body.lines()
        .filter_map(|line| {
            let (name, hash) = line.trim().split_once(':')?;
            let hash = hash.trim().to_ascii_lowercase();
            let valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
            (valid && !name.contains(' ')).then(|| (name.trim().to_owned(), hash))
        })
        .collect()
}

/// Download progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Bytes downloaded so far, and the total.
    Downloading {
        /// Bytes received.
        received: u64,
        /// Expected size.
        total: u64,
    },
    /// Checking hashes and signature.
    Verifying,
    /// Moving the binary into place.
    Installing,
}

/// Fetches releases and installs the managed binary.
#[derive(Debug, Clone)]
pub struct Installer {
    http: reqwest::Client,
    api_url: String,
    dest_dir: PathBuf,
    asset: AssetSpec,
    verify_signature: bool,
}

impl Installer {
    /// An installer for this machine writing into `dest_dir`.
    ///
    /// # Errors
    /// [`Error::UnsupportedPlatform`] if Cloudflare publishes no binary for this machine.
    pub fn new(dest_dir: PathBuf) -> Result<Self> {
        let asset = AssetSpec::current().ok_or(Error::UnsupportedPlatform)?;
        Self::with_options(
            dest_dir,
            asset,
            GITHUB_API.to_owned(),
            cfg!(target_os = "macos"),
        )
    }

    /// An installer with explicit inputs (tests).
    ///
    /// # Errors
    /// Fails if the HTTP client can't be built.
    pub fn with_options(
        dest_dir: PathBuf,
        asset: AssetSpec,
        api_url: String,
        verify_signature: bool,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Teitunnel/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self {
            http,
            api_url,
            dest_dir,
            asset,
            verify_signature,
        })
    }

    /// Path of the managed binary.
    pub fn binary_path(&self) -> PathBuf {
        self.dest_dir.join(binary_name())
    }

    /// Looks up the latest release.
    ///
    /// # Errors
    /// Fails on network errors, rate limiting or an unexpected response.
    pub async fn latest(&self) -> Result<Release> {
        let response = self
            .http
            .get(&self.api_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?;
        let release: GhRelease = response.json().await?;
        let version = release.tag_name.trim_start_matches('v').parse()?;
        Ok(Release {
            version,
            checksums: parse_checksums(&release.body),
            assets: release
                .assets
                .into_iter()
                .map(|a| {
                    let digest = a
                        .digest
                        .and_then(|d| d.strip_prefix("sha256:").map(str::to_ascii_lowercase));
                    (
                        a.name,
                        ReleaseAsset {
                            url: a.browser_download_url,
                            size: a.size,
                            digest,
                        },
                    )
                })
                .collect(),
        })
    }

    /// Downloads, verifies and installs `release`. Returns the installed binary path.
    ///
    /// # Errors
    /// Any verification failure aborts before the current binary is touched.
    pub async fn install(
        &self,
        release: &Release,
        mut progress: impl FnMut(Progress),
    ) -> Result<PathBuf> {
        let asset = release
            .assets
            .get(self.asset.name)
            .ok_or(Error::UnsupportedPlatform)?;
        let staging = self.dest_dir.join(".staging");
        let _ = tokio::fs::remove_dir_all(&staging).await;
        tokio::fs::create_dir_all(&staging).await?;
        let result = self.stage(release, asset, &staging, &mut progress).await;
        let outcome = match result {
            Ok(staged) => {
                progress(Progress::Installing);
                self.promote(&staged).await
            }
            Err(err) => Err(err),
        };
        let _ = tokio::fs::remove_dir_all(&staging).await;
        outcome
    }

    async fn stage(
        &self,
        release: &Release,
        asset: &ReleaseAsset,
        staging: &Path,
        progress: &mut impl FnMut(Progress),
    ) -> Result<PathBuf> {
        let download = staging.join(self.asset.name);
        let archive_hash = self.download(asset, &download, progress).await?;
        progress(Progress::Verifying);
        if let Some(expected) = &asset.digest
            && *expected != archive_hash
        {
            return Err(Error::Verification(format!(
                "{} doesn't match GitHub's digest",
                self.asset.name
            )));
        }

        let binary = staging.join(binary_name());
        match self.asset.kind {
            ArchiveKind::Tgz => extract_tgz(download.clone(), binary.clone()).await?,
            ArchiveKind::Raw => tokio::fs::rename(&download, &binary).await?,
        }
        let binary_hash = sha256_file(&binary).await?;
        match release.checksums.get(self.asset.name) {
            Some(expected) if *expected != binary_hash => {
                return Err(Error::Verification(
                    "the binary doesn't match the published checksum".into(),
                ));
            }
            None if asset.digest.is_none() => {
                return Err(Error::Verification(
                    "the release publishes no checksum for this platform".into(),
                ));
            }
            _ => {}
        }
        make_executable(&binary).await?;
        if self.verify_signature {
            verify_codesign(&binary).await?;
        }
        match read_version(&binary).await? {
            Some(version) if version == release.version => Ok(binary),
            other => Err(Error::Verification(format!(
                "the binary reports version {other:?}, expected {}",
                release.version
            ))),
        }
    }

    async fn download(
        &self,
        asset: &ReleaseAsset,
        path: &Path,
        progress: &mut impl FnMut(Progress),
    ) -> Result<String> {
        let mut response = self.http.get(&asset.url).send().await?.error_for_status()?;
        let total = response.content_length().unwrap_or(asset.size);
        let mut file = tokio::fs::File::create(path).await?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        progress(Progress::Downloading { received, total });
        while let Some(chunk) = response.chunk().await? {
            received += chunk.len() as u64;
            if received > MAX_BINARY_BYTES {
                return Err(Error::Verification(
                    "the download is unexpectedly large".into(),
                ));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            progress(Progress::Downloading { received, total });
        }
        file.flush().await?;
        Ok(hex(&hasher.finalize()))
    }

    /// Moves the staged binary into place, keeping the old one as `.prev`.
    async fn promote(&self, staged: &Path) -> Result<PathBuf> {
        let target = self.binary_path();
        let previous = self.dest_dir.join(format!("{}.prev", binary_name()));
        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&previous).await;
            tokio::fs::rename(&target, &previous).await?;
        }
        tokio::fs::rename(staged, &target).await?;
        Ok(target)
    }
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

async fn sha256_file(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<String> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(hex(&hasher.finalize()))
    })
    .await
    .map_err(|err| Error::Verification(err.to_string()))?
}

/// Extracts the `cloudflared` entry of a `.tgz` (and nothing else) to `dest`.
async fn extract_tgz(archive: PathBuf, dest: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&archive)?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        for entry in tar.entries()? {
            let entry = entry?;
            let is_binary = entry.header().entry_type().is_file()
                && entry
                    .path()?
                    .file_name()
                    .is_some_and(|name| name == "cloudflared");
            if is_binary {
                let mut out = std::fs::File::create(&dest)?;
                let copied = std::io::copy(&mut entry.take(MAX_BINARY_BYTES + 1), &mut out)?;
                if copied > MAX_BINARY_BYTES {
                    return Err(Error::Verification(
                        "the archive entry is unexpectedly large".into(),
                    ));
                }
                return Ok(());
            }
        }
        Err(Error::Verification(
            "the archive doesn't contain cloudflared".into(),
        ))
    })
    .await
    .map_err(|err| Error::Verification(err.to_string()))?
}

async fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).await?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// `codesign --verify --strict`, then check the Team ID is Cloudflare's.
async fn verify_codesign(path: &Path) -> Result<()> {
    let verify = tokio::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(path)
        .output()
        .await?;
    if !verify.status.success() {
        return Err(Error::Verification(
            "the binary's code signature is invalid".into(),
        ));
    }
    let details = tokio::process::Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=2"])
        .arg(path)
        .output()
        .await?;
    // codesign prints details on stderr.
    let text = String::from_utf8_lossy(&details.stderr);
    let team = text
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="));
    if team == Some(CLOUDFLARE_TEAM_ID) {
        Ok(())
    } else {
        Err(Error::Verification(format!(
            "the binary isn't signed by Cloudflare (team {team:?})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_note_checksums() {
        let body = "### SHA256 Checksums:\n```\ncloudflared-darwin-arm64.tgz: 9A0B19F67DC7A3011BC6B972C7CE06A5FCEA8784AC6BD599FFA382EA4AEB5A6E\ncloudflared-linux-amd64: 03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc\nnot a checksum: abc\nwith space.tgz x: 03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc\n```";
        let sums = parse_checksums(body);
        assert_eq!(sums.len(), 2);
        assert_eq!(
            sums["cloudflared-darwin-arm64.tgz"],
            "9a0b19f67dc7a3011bc6b972c7ce06a5fcea8784ac6bd599ffa382ea4aeb5a6e"
        );
    }

    #[test]
    fn selects_assets_per_platform() {
        assert_eq!(
            AssetSpec::for_platform("macos", "aarch64").unwrap().kind,
            ArchiveKind::Tgz
        );
        assert_eq!(
            AssetSpec::for_platform("linux", "x86_64").unwrap().name,
            "cloudflared-linux-amd64"
        );
        assert_eq!(
            AssetSpec::for_platform("windows", "x86_64").unwrap().name,
            "cloudflared-windows-amd64.exe"
        );
        assert!(AssetSpec::for_platform("freebsd", "x86_64").is_none());
    }

    #[test]
    fn hex_encodes() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[cfg(unix)]
    mod end_to_end {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };

        use super::*;

        const SCRIPT: &[u8] = b"#!/bin/sh\necho 'cloudflared version 2026.9.1 (built x)'\n";
        const ASSET: AssetSpec = AssetSpec {
            name: "cloudflared-darwin-arm64.tgz",
            kind: ArchiveKind::Tgz,
        };

        fn tgz(contents: &[u8]) -> Vec<u8> {
            let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
                Vec::new(),
                flate2::Compression::default(),
            ));
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "cloudflared", contents)
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap()
        }

        fn sha(bytes: &[u8]) -> String {
            hex(&Sha256::digest(bytes))
        }

        async fn server(archive: &[u8], digest: &str, binary_sum: &str, tag: &str) -> MockServer {
            let server = MockServer::start().await;
            let release = serde_json::json!({
                "tag_name": tag,
                "body": format!("### SHA256 Checksums:\n```\ncloudflared-darwin-arm64.tgz: {binary_sum}\n```"),
                "assets": [{
                    "name": "cloudflared-darwin-arm64.tgz",
                    "browser_download_url": format!("{}/download/cloudflared-darwin-arm64.tgz", server.uri()),
                    "size": archive.len(),
                    "digest": format!("sha256:{digest}"),
                }]
            });
            Mock::given(method("GET"))
                .and(path("/release"))
                .respond_with(ResponseTemplate::new(200).set_body_json(release))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/download/cloudflared-darwin-arm64.tgz"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(archive.to_vec()))
                .mount(&server)
                .await;
            server
        }

        fn installer(server: &MockServer, dest: &Path) -> Installer {
            Installer::with_options(
                dest.to_path_buf(),
                ASSET,
                format!("{}/release", server.uri()),
                false,
            )
            .unwrap()
        }

        #[tokio::test]
        async fn installs_verified_binary_and_keeps_the_previous_one() {
            let archive = tgz(SCRIPT);
            let server = server(&archive, &sha(&archive), &sha(SCRIPT), "2026.9.1").await;
            let dest = tempfile::tempdir().unwrap();
            std::fs::write(dest.path().join("cloudflared"), b"old").unwrap();
            let installer = installer(&server, dest.path());

            let release = installer.latest().await.unwrap();
            assert_eq!(release.version.to_string(), "2026.9.1");
            let mut events = Vec::new();
            let path = installer
                .install(&release, |p| events.push(p))
                .await
                .unwrap();

            assert_eq!(
                read_version(&path).await.unwrap().unwrap().to_string(),
                "2026.9.1"
            );
            assert_eq!(
                std::fs::read(dest.path().join("cloudflared.prev")).unwrap(),
                b"old"
            );
            assert!(!dest.path().join(".staging").exists());
            assert!(matches!(
                events.first(),
                Some(Progress::Downloading { received: 0, .. })
            ));
            assert!(events.contains(&Progress::Verifying));
            assert_eq!(events.last(), Some(&Progress::Installing));
        }

        async fn assert_rejected(archive: &[u8], digest: &str, binary_sum: &str, tag: &str) {
            let server = server(archive, digest, binary_sum, tag).await;
            let dest = tempfile::tempdir().unwrap();
            std::fs::write(dest.path().join("cloudflared"), b"current").unwrap();
            let installer = installer(&server, dest.path());
            let release = installer.latest().await.unwrap();
            let err = installer.install(&release, |_| {}).await.unwrap_err();
            assert!(matches!(err, Error::Verification(_)), "{err:?}");
            assert_eq!(
                std::fs::read(dest.path().join("cloudflared")).unwrap(),
                b"current",
                "current binary untouched"
            );
            assert!(!dest.path().join("cloudflared.prev").exists());
            assert!(!dest.path().join(".staging").exists());
        }

        #[tokio::test]
        async fn rejects_a_tampered_archive() {
            let archive = tgz(SCRIPT);
            assert_rejected(&archive, &sha(b"something else"), &sha(SCRIPT), "2026.9.1").await;
        }

        #[tokio::test]
        async fn rejects_a_tampered_binary() {
            let archive = tgz(SCRIPT);
            assert_rejected(&archive, &sha(&archive), &sha(b"other binary"), "2026.9.1").await;
        }

        #[tokio::test]
        async fn rejects_a_version_mismatch() {
            let archive = tgz(SCRIPT);
            assert_rejected(&archive, &sha(&archive), &sha(SCRIPT), "2026.10.0").await;
        }
    }

    /// Real network + real signature check. Run with `cargo nextest run --run-ignored all`.
    #[tokio::test]
    #[ignore = "downloads ~20 MB from GitHub"]
    async fn installs_the_real_latest_release() {
        let dest = tempfile::tempdir().unwrap();
        let installer = Installer::new(dest.path().to_path_buf()).unwrap();
        let release = installer.latest().await.unwrap();
        let path = installer.install(&release, |_| {}).await.unwrap();
        assert_eq!(read_version(&path).await.unwrap(), Some(release.version));
    }
}
