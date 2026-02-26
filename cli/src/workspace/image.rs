//! Image utilities — SHA256 hashing and GitHub release resolution.
//!
//! The old image download and caching logic (qcow2 download, verification,
//! `ensure_available()`) has been removed as part of the cloud-init migration.
//! VM provisioning now uses `multipass launch 24.04 --cloud-init` instead of
//! downloading baked images.
//!
//! Retained functions:
//! - `sha256_file()` — used by `vm::sha256_file()` for config hash computation
//! - `images_dir()` — used by `polis delete` to clean up any legacy image cache
//! - `resolve_latest_image_url()` — used by `polis doctor` for version drift checks

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::commands::update::hex_encode;

/// Returns the image cache directory (legacy — used by `polis delete --all`).
///
/// Linux: `~/polis/images/`
/// Windows/macOS: `~/.polis/images/`
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub fn images_dir() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    return Ok(dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join("polis")
        .join("images"));
    #[cfg(not(target_os = "linux"))]
    Ok(dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".polis")
        .join("images"))
}

/// Compute the SHA256 hex digest of a file.
///
/// Reads the file in 64 KB chunks to avoid loading large files into memory.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or read.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = file.read(&mut buf).context("reading file")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

// ── GitHub Release Resolution ────────────────────────────────────────────────

/// Resolved release information from GitHub.
#[derive(Debug)]
pub struct ResolvedRelease {
    /// The release tag (e.g. `v0.3.0`).
    pub tag: String,
}

/// GitHub releases API URL.
pub const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/OdraLabsHQ/polis/releases?per_page=10";

/// Resolve the latest release tag from GitHub releases.
///
/// Used by `polis doctor` for version drift checks.
///
/// # Errors
///
/// Returns an error if the network is unavailable or no release is found.
pub fn resolve_latest_image_url() -> Result<ResolvedRelease> {
    let url =
        std::env::var("POLIS_GITHUB_API_URL").unwrap_or_else(|_| GITHUB_RELEASES_URL.to_string());
    let token = std::env::var("GITHUB_TOKEN").unwrap_or_default();

    let req = ureq::get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "polis-cli");
    let req = if token.is_empty() {
        req
    } else {
        req.set("Authorization", &format!("Bearer {token}"))
    };

    let body: serde_json::Value = match req.call() {
        Ok(resp) => serde_json::from_str(&resp.into_string().context("reading response")?)
            .context("parsing response")?,
        Err(ureq::Error::Status(403, _)) => anyhow::bail!(
            "Cannot check for updates: rate limited.\n\nTry again in a few minutes, or set GITHUB_TOKEN."
        ),
        Err(ureq::Error::Status(code, _)) => {
            anyhow::bail!("Cannot check for updates: HTTP {code}")
        }
        Err(_) => anyhow::bail!(
            "Cannot check for updates: no network connection.\n\nFor offline setup: https://polis.dev/docs/offline"
        ),
    };

    let releases = body.as_array().context("invalid response")?;

    for release in releases {
        if let Some(tag) = release["tag_name"].as_str()
            && !tag.is_empty()
        {
            return Ok(ResolvedRelease {
                tag: tag.to_string(),
            });
        }
    }
    anyhow::bail!("No releases found in recent GitHub releases.")
}
