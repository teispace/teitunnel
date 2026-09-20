use std::path::{Path, PathBuf};
use std::process::Command;
use crate::error::AppError;
use crate::models::{BinaryStatus, DownloadProgress};

pub struct BinaryManager;

impl BinaryManager {
    /// Returns the managed binary directory: ~/.teitunnel/bin
    pub fn get_managed_bin_dir() -> Result<PathBuf, AppError> {
        let home = dirs::home_dir().ok_or_else(|| AppError::IoError("Unable to locate home directory".into()))?;
        let bin_dir = home.join(".teitunnel").join("bin");
        if !bin_dir.exists() {
            std::fs::create_dir_all(&bin_dir)?;
        }
        Ok(bin_dir)
    }

    /// Returns the path to the managed binary: ~/.teitunnel/bin/cloudflared
    pub fn get_managed_bin_path() -> Result<PathBuf, AppError> {
        let bin_dir = Self::get_managed_bin_dir()?;
        let bin_name = if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" };
        Ok(bin_dir.join(bin_name))
    }

    /// Finds cloudflared on the system: checks managed bin first, then $PATH, then standard paths
    pub fn find_binary() -> Option<PathBuf> {
        // 1. Check managed binary
        if let Ok(managed_path) = Self::get_managed_bin_path() {
            if managed_path.exists() {
                return Some(managed_path);
            }
        }

        // 2. Check if cloudflared is on PATH
        if let Ok(output) = Command::new(if cfg!(windows) { "where" } else { "which" })
            .arg("cloudflared")
            .output()
        {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().lines().next().unwrap_or("").to_string();
                let path = PathBuf::from(&path_str);
                if path.exists() {
                    return Some(path);
                }
            }
        }

        // 3. Check well-known common locations
        let common_paths = if cfg!(target_os = "macos") {
            vec![
                "/opt/homebrew/bin/cloudflared",
                "/usr/local/bin/cloudflared",
            ]
        } else if cfg!(target_os = "linux") {
            vec![
                "/usr/local/bin/cloudflared",
                "/usr/bin/cloudflared",
            ]
        } else if cfg!(target_os = "windows") {
            vec![
                "C:\\Program Files\\cloudflared\\cloudflared.exe",
                "C:\\Program Files (x86)\\cloudflared\\cloudflared.exe",
            ]
        } else {
            vec![]
        };

        for p in common_paths {
            let path = Path::new(p);
            if path.exists() {
                return Some(path.to_path_buf());
            }
        }

        None
    }

    /// Queries the version of a cloudflared executable
    pub fn get_version(bin_path: &Path) -> Option<String> {
        let output = Command::new(bin_path).arg("--version").output().ok()?;
        if output.status.success() {
            let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // Typically: "cloudflared version 2026.9.1 (built ...)"
            Some(out)
        } else {
            None
        }
    }

    /// Returns full status of the binary installation
    pub fn check_status() -> BinaryStatus {
        let arch = std::env::consts::ARCH.to_string();
        let os = std::env::consts::OS.to_string();

        if let Some(path) = Self::find_binary() {
            let version = Self::get_version(&path);
            let is_managed = if let Ok(managed) = Self::get_managed_bin_path() {
                path == managed
            } else {
                false
            };

            BinaryStatus {
                is_installed: true,
                path: Some(path.to_string_lossy().to_string()),
                version,
                is_managed,
                architecture: arch,
                os,
            }
        } else {
            BinaryStatus {
                is_installed: false,
                path: None,
                version: None,
                is_managed: false,
                architecture: arch,
                os,
            }
        }
    }

    /// Determines the official Cloudflare GitHub download URL for this platform
    pub fn get_download_url() -> Result<String, AppError> {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        let filename = match (os, arch) {
            ("macos", "aarch64") => "cloudflared-darwin-arm64.tgz",
            ("macos", "x86_64") => "cloudflared-darwin-amd64.tgz",
            ("linux", "x86_64") => "cloudflared-linux-amd64",
            ("linux", "aarch64") => "cloudflared-linux-arm64",
            ("windows", "x86_64") => "cloudflared-windows-amd64.exe",
            _ => {
                return Err(AppError::BinaryNotFound(format!(
                    "Unsupported platform: {}-{}",
                    os, arch
                )));
            }
        };

        Ok(format!(
            "https://github.com/cloudflare/cloudflared/releases/latest/download/{}",
            filename
        ))
    }

    /// Downloads and installs the managed cloudflared binary to ~/.teitunnel/bin/cloudflared
    pub async fn download_managed_binary<F>(progress_callback: F) -> Result<BinaryStatus, AppError>
    where
        F: Fn(DownloadProgress) + Send + 'static,
    {
        let url = Self::get_download_url()?;
        let target_path = Self::get_managed_bin_path()?;

        progress_callback(DownloadProgress {
            bytes_downloaded: 0,
            total_bytes: None,
            percentage: Some(0.0),
            status: "Initiating download from Cloudflare...".into(),
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let response = client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(AppError::NetworkError(format!(
                "Failed to download cloudflared: HTTP {}",
                response.status()
            )));
        }

        let total_size = response.content_length();
        let bytes = response.bytes().await?;

        progress_callback(DownloadProgress {
            bytes_downloaded: bytes.len() as u64,
            total_bytes: total_size,
            percentage: Some(100.0),
            status: "Unpacking binary...".into(),
        });

        if url.ends_with(".tgz") {
            // macOS tar.gz unpack
            use flate2::read::GzDecoder;
            use tar::Archive;

            let tar = GzDecoder::new(&bytes[..]);
            let mut archive = Archive::new(tar);
            let bin_dir = Self::get_managed_bin_dir()?;
            archive.unpack(&bin_dir)?;
        } else {
            // Direct executable (Linux/Windows)
            std::fs::write(&target_path, &bytes)?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&target_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&target_path, perms)?;
        }

        progress_callback(DownloadProgress {
            bytes_downloaded: bytes.len() as u64,
            total_bytes: total_size,
            percentage: Some(100.0),
            status: "Installation complete!".into(),
        });

        Ok(Self::check_status())
    }
}
