//! Image download, verification, and caching.

use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zipsign_api::{
    PUBLIC_KEY_LENGTH, VerifyingKey, unsign::copy_and_unsign_tar, verify::verify_tar,
};

use crate::commands::update::{POLIS_PUBLIC_KEY_B64, base64_decode, hex_encode};

const IMAGE_FILENAME: &str = "polis.qcow2";
const SIDECAR_FILENAME: &str = "polis.qcow2.sha256";
const METADATA_FILENAME: &str = "image.json";

/// Image source for workspace creation.
#[derive(Debug, Clone)]
pub enum ImageSource {
    /// Use cached image or download from GitHub.
    Default,
    /// Local file path.
    LocalFile(PathBuf),
    /// HTTP(S) URL.
    HttpUrl(String),
}

/// Metadata for a cached image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadata {
    pub version: String,
    pub sha256: String,
    pub arch: String,
    pub downloaded_at: DateTime<Utc>,
    pub source: String,
}

/// Returns the image cache directory.
///
/// Linux: `~/polis/images/` (snap `AppArmor` requires non-hidden)
/// Windows: `%PROGRAMDATA%\Polis\images\` (accessible to multipassd SYSTEM service)
/// macOS: `~/.polis/images/`
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub fn images_dir() -> Result<PathBuf> {
    // All platforms use ~/.polis/images/ so the image is always owned by the
    // current user and readable by the process that calls `multipass launch`.
    //
    // On Windows, ProgramData was previously used but multipassd runs as
    // NT AUTHORITY\SYSTEM and may not have read access to files created there
    // by the logged-in user. Using the home directory avoids that ACL mismatch.
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

/// Check if a valid image exists in cache.
///
/// # Errors
/// Load existing metadata from cache.
///
/// # Errors
///
/// Returns an error if the metadata file cannot be read or parsed.
pub fn load_metadata(images_dir: &Path) -> Result<Option<ImageMetadata>> {
    let path = images_dir.join(METADATA_FILENAME);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let meta =
        serde_json::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(meta))
}

/// Ensure image is available (download if needed).
///
/// Returns path to verified image.
///
/// # Errors
///
/// Returns an error if download fails, verification fails, or disk is full.
pub fn ensure_available(source: ImageSource, quiet: bool) -> Result<PathBuf> {
    let dir = images_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let dest = dir.join(IMAGE_FILENAME);
    let sidecar = dir.join(SIDECAR_FILENAME);

    match source {
        ImageSource::Default => ensure_default(&dir, &dest, &sidecar, quiet),
        ImageSource::LocalFile(path) => ensure_local(&dir, &dest, &sidecar, &path, quiet),
        ImageSource::HttpUrl(url) => ensure_http(&dir, &dest, &url, quiet),
    }
}

fn ensure_default(dir: &Path, dest: &Path, sidecar: &Path, quiet: bool) -> Result<PathBuf> {
    if dest.exists() && load_metadata(dir)?.is_some() {
        return Ok(dest.to_path_buf());
    }
    if !quiet {
        println!("Downloading workspace...");
    }
    let resolved = resolve_latest_image_url()?;
    download_with_resume(&resolved.image_url, dest, quiet)?;
    download_with_resume(&resolved.checksum_url, sidecar, quiet)?;
    if !quiet {
        print!("Verifying integrity...");
    }
    let sha256 = verify_image_integrity(dest, sidecar)?;
    if !quiet {
        println!(" ✓");
    }
    let meta = ImageMetadata {
        version: resolved.tag,
        sha256,
        arch: current_arch()?.to_string(),
        downloaded_at: Utc::now(),
        source: resolved.image_url,
    };
    write_metadata(dir, &meta)?;
    Ok(dest.to_path_buf())
}

fn version_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_prefix("polis-")?.strip_suffix(".qcow2")?;
    for arch in &["amd64", "arm64"] {
        if let Some(version) = stem.strip_suffix(&format!("-{arch}")) {
            return Some(version.to_string());
        }
    }
    None
}

fn ensure_local(
    dir: &Path,
    dest: &Path,
    sidecar: &Path,
    path: &Path,
    quiet: bool,
) -> Result<PathBuf> {
    if !quiet {
        println!("Using: {}", path.display());
    }
    std::fs::copy(path, dest).with_context(|| format!("copying {}", path.display()))?;
    let source_sidecar = path.with_extension("qcow2.sha256");
    if source_sidecar.exists() {
        std::fs::copy(&source_sidecar, sidecar)?;
    }
    let sha256 = if sidecar.exists() {
        verify_image_integrity(dest, sidecar)?
    } else {
        sha256_file(dest)?
    };
    let meta = ImageMetadata {
        version: version_from_path(path).unwrap_or_else(|| "local".to_string()),
        sha256,
        arch: current_arch()?.to_string(),
        downloaded_at: Utc::now(),
        source: path.to_string_lossy().into_owned(),
    };
    write_metadata(dir, &meta)?;
    Ok(dest.to_path_buf())
}

fn ensure_http(dir: &Path, dest: &Path, url: &str, quiet: bool) -> Result<PathBuf> {
    if !quiet {
        println!("Downloading from {url}...");
    }
    download_with_resume(url, dest, quiet)?;
    let sha256 = sha256_file(dest)?;
    let meta = ImageMetadata {
        version: "unknown".to_string(),
        sha256,
        arch: current_arch()?.to_string(),
        downloaded_at: Utc::now(),
        source: url.to_string(),
    };
    write_metadata(dir, &meta)?;
    Ok(dest.to_path_buf())
}

fn write_metadata(images_dir: &Path, meta: &ImageMetadata) -> Result<()> {
    let path = images_dir.join(METADATA_FILENAME);
    let content = serde_json::to_string_pretty(meta).context("serializing image metadata")?;
    std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))
}

fn current_arch() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        other => anyhow::bail!("unsupported architecture: {other}"),
    }
}

// ── Download ─────────────────────────────────────────────────────────────────

fn download_with_resume(url: &str, dest: &Path, quiet: bool) -> Result<()> {
    let partial = {
        let mut s = dest.as_os_str().to_owned();
        s.push(".partial");
        PathBuf::from(s)
    };
    let existing = partial.metadata().map(|m| m.len()).unwrap_or(0);
    do_download(url, dest, &partial, existing, quiet, true)
}

fn do_download(
    url: &str,
    dest: &Path,
    partial: &Path,
    existing: u64,
    quiet: bool,
    allow_retry: bool,
) -> Result<()> {
    let req = ureq::get(url);
    let req = if existing > 0 {
        req.set("Range", &format!("bytes={existing}-"))
    } else {
        req
    };

    let response = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(416, _)) if allow_retry => {
            std::fs::remove_file(partial).ok();
            return do_download(url, dest, partial, 0, quiet, false);
        }
        Err(ureq::Error::Status(code, _)) => anyhow::bail!("Download failed: HTTP {code}"),
        Err(_) => anyhow::bail!("Download interrupted.\n\nResume with: polis start"),
    };

    let status = response.status();
    let (mut file, start_pos) = open_partial_file(status, partial, existing)?;

    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .map(|len| if status == 206 { start_pos + len } else { len });

    if start_pos > 0 && total.is_some_and(|t| start_pos >= t) {
        drop(file);
        std::fs::remove_file(partial).ok();
        return do_download(url, dest, partial, 0, quiet, false);
    }

    let pb = make_progress_bar(quiet, total, start_pos);

    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).context("Download interrupted")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("Download interrupted")?;
        pb.inc(n as u64);
    }
    pb.finish_and_clear();
    drop(file);
    std::fs::rename(partial, dest).context("failed to finalize downloaded image")?;
    Ok(())
}

fn open_partial_file(status: u16, partial: &Path, existing: u64) -> Result<(File, u64)> {
    if status == 206 {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(partial)
            .context("opening partial file")?;
        Ok((file, existing))
    } else if status == 200 {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(partial)
            .context("opening partial file")?;
        Ok((file, 0))
    } else {
        anyhow::bail!("Download failed: HTTP {status}");
    }
}

fn make_progress_bar(quiet: bool, total: Option<u64>, start_pos: u64) -> indicatif::ProgressBar {
    if quiet {
        return indicatif::ProgressBar::hidden();
    }
    if let Some(t) = total {
        let pb = indicatif::ProgressBar::new(t);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("[{bar:40}] {percent}%")
                .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar())
                .progress_chars("█▓░"),
        );
        pb.set_position(start_pos);
        pb
    } else {
        indicatif::ProgressBar::new_spinner()
    }
}

// ── Verification ─────────────────────────────────────────────────────────────

fn verify_image_integrity(image_path: &Path, sidecar_path: &Path) -> Result<String> {
    let sidecar_bytes = std::fs::read(sidecar_path)
        .with_context(|| format!("reading {}", sidecar_path.display()))?;

    let key_b64 = std::env::var("POLIS_VERIFYING_KEY_B64")
        .unwrap_or_else(|_| POLIS_PUBLIC_KEY_B64.to_string());
    let key_bytes = base64_decode(&key_b64).context("invalid public key")?;
    anyhow::ensure!(
        key_bytes.len() == PUBLIC_KEY_LENGTH,
        "public key length mismatch"
    );
    let key_array: [u8; PUBLIC_KEY_LENGTH] = key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid public key"))?;
    let verifying_key = VerifyingKey::from_bytes(&key_array).context("invalid public key")?;

    verify_tar(&mut Cursor::new(&sidecar_bytes), &[verifying_key], None).context(
        "Workspace image failed integrity check.\n\nThis may indicate a corrupted download or tampering.\nRetry with: polis delete --all && polis start"
    )?;

    let expected = extract_checksum_from_signed_file(sidecar_path)?;
    let actual = sha256_file(image_path)?;
    anyhow::ensure!(
        actual == expected,
        "Workspace image failed integrity check.\n\nThis may indicate a corrupted download or tampering.\nRetry with: polis delete --all && polis start"
    );
    Ok(actual)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = file.read(&mut buf).context("reading image file")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn extract_checksum_from_signed_file(checksum_path: &Path) -> Result<String> {
    let signed_bytes = std::fs::read(checksum_path)
        .with_context(|| format!("reading {}", checksum_path.display()))?;
    let mut unsigned = Cursor::new(Vec::new());
    copy_and_unsign_tar(&mut Cursor::new(&signed_bytes), &mut unsigned)
        .context("failed to unsign checksum")?;
    unsigned.set_position(0);
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(unsigned));
    let mut content = String::new();
    if let Some(entry) = tar.entries().context("reading sidecar")?.next() {
        entry
            .context("reading entry")?
            .read_to_string(&mut content)
            .context("reading content")?;
    }
    let hex = content
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("malformed checksum file"))?;
    anyhow::ensure!(
        hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
        "malformed checksum file"
    );
    Ok(hex.to_string())
}

// ── GitHub Release Resolution ────────────────────────────────────────────────

/// Resolved release information from GitHub.
#[derive(Debug)]
pub struct ResolvedRelease {
    pub tag: String,
    pub image_url: String,
    pub checksum_url: String,
}

/// GitHub releases API URL.
pub const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/OdraLabsHQ/polis/releases?per_page=10";

/// Resolve the latest image URL from GitHub releases.
///
/// # Errors
///
/// Returns an error if the network is unavailable or no suitable release is found.
pub fn resolve_latest_image_url() -> Result<ResolvedRelease> {
    let arch = current_arch()?;
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
            anyhow::bail!("Cannot download workspace: HTTP {code}")
        }
        Err(_) => anyhow::bail!(
            "Cannot download workspace: no network connection.\n\nFor offline setup: https://polis.dev/docs/offline"
        ),
    };

    let qcow2_suffix = format!("-{arch}.qcow2");
    let sha256_suffix = format!("-{arch}.qcow2.sha256");
    let releases = body.as_array().context("invalid response")?;

    for release in releases {
        let tag = release["tag_name"].as_str().unwrap_or_default().to_string();
        let Some(assets) = release["assets"].as_array() else {
            continue;
        };
        let find_url = |suffix: &str| {
            assets
                .iter()
                .find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(suffix)))
                .and_then(|a| a["browser_download_url"].as_str())
                .map(str::to_string)
        };
        if let (Some(image_url), Some(checksum_url)) =
            (find_url(&qcow2_suffix), find_url(&sha256_suffix))
        {
            return Ok(ResolvedRelease {
                tag,
                image_url,
                checksum_url,
            });
        }
    }
    anyhow::bail!("No workspace image found in recent releases.\n\nUse: polis start --image <url>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_metadata_absent_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(load_metadata(dir.path()).expect("load").is_none());
    }

    #[test]
    fn load_metadata_valid_json_returns_some() {
        let dir = tempfile::tempdir().expect("tempdir");
        let meta = ImageMetadata {
            version: "v1.0.0".into(),
            sha256: "abc123".into(),
            arch: "amd64".into(),
            downloaded_at: chrono::Utc::now(),
            source: "https://example.com/polis.qcow2".into(),
        };
        std::fs::write(
            dir.path().join("image.json"),
            serde_json::to_string(&meta).expect("json"),
        )
        .expect("write");
        let loaded = load_metadata(dir.path()).expect("load").expect("some");
        assert_eq!(loaded.version, "v1.0.0");
        assert_eq!(loaded.sha256, "abc123");
    }

    #[test]
    fn version_from_path_parses_amd64() {
        let p = Path::new("/tmp/polis-0.3.0-preview-11-amd64.qcow2");
        assert_eq!(
            super::version_from_path(p).as_deref(),
            Some("0.3.0-preview-11")
        );
    }

    #[test]
    fn version_from_path_parses_arm64() {
        let p = Path::new("/tmp/polis-0.3.0-preview-11-arm64.qcow2");
        assert_eq!(
            super::version_from_path(p).as_deref(),
            Some("0.3.0-preview-11")
        );
    }

    #[test]
    fn version_from_path_returns_none_for_unknown_name() {
        let p = Path::new("/tmp/custom.qcow2");
        assert!(super::version_from_path(p).is_none());
    }

    #[test]
    fn load_metadata_malformed_json_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("image.json"), b"not json").expect("write");
        assert!(load_metadata(dir.path()).is_err());
    }
}
