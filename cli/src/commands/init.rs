//! Init command — download and verify the workspace VM image.

use std::fs::OpenOptions;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Args;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zipsign_api::{PUBLIC_KEY_LENGTH, VerifyingKey, verify::verify_tar};

use crate::commands::update::{POLIS_PUBLIC_KEY_B64, base64_decode, hex_encode};

/// Fixed image filename in the cache directory.
const IMAGE_FILENAME: &str = "polis-workspace.qcow2";
/// Signed checksum sidecar filename.
const SIDECAR_FILENAME: &str = "polis-workspace.qcow2.sha256";
/// Metadata filename.
const METADATA_FILENAME: &str = "image.json";

/// Arguments for the `polis init` command.
#[derive(Args)]
pub struct InitArgs {
    /// Image source: local file path or HTTP(S) URL.
    /// Defaults to latest GitHub release with a .qcow2 asset.
    #[arg(long)]
    pub image: Option<String>,

    /// Re-download even if cached image passes verification.
    #[arg(long)]
    pub force: bool,

    /// Check for newer image without downloading (dry-run).
    #[arg(long)]
    pub check: bool,
}

/// Resolved image source after parsing the `--image` flag.
#[derive(Debug)]
pub enum ImageSource {
    /// Local file path (copy to cache).
    LocalFile(PathBuf),
    /// HTTP(S) URL (download with resume).
    HttpUrl(String),
    /// No `--image` flag: resolve via GitHub API.
    GitHubLatest,
}

/// Context for download progress display.
pub(crate) struct DownloadContext {
    /// Whether to suppress progress output.
    pub quiet: bool,
}

/// Metadata written after successful image acquisition.
#[derive(Debug, Serialize, Deserialize)]
pub struct ImageMetadata {
    /// Semver tag from the release (e.g., `"v0.3.0"`).
    pub version: String,
    /// Hex-encoded SHA-256 of the `.qcow2` file.
    pub sha256: String,
    /// CPU architecture (e.g., `"amd64"`, `"arm64"`).
    pub arch: String,
    /// ISO-8601 timestamp of when the image was downloaded.
    pub downloaded_at: DateTime<Utc>,
    /// URL or path the image was acquired from.
    pub source: String,
}

/// Entry point for `polis init`.
///
/// # Errors
///
/// Returns an error if argument validation, directory creation, image
/// acquisition, verification, or metadata writing fails.
pub fn run(args: &InitArgs) -> Result<()> {
    anyhow::ensure!(
        !(args.check && args.force),
        "--check and --force are mutually exclusive"
    );

    let images_dir = images_dir()?;
    std::fs::create_dir_all(&images_dir)
        .with_context(|| format!("failed to create image directory: {}", images_dir.display()))?;

    let source = resolve_source(args.image.as_deref())?;

    let cached_image = images_dir.join(IMAGE_FILENAME);

    if !args.force && cached_image.exists()
        && let Some(meta) = load_metadata(&images_dir)?
    {
        println!(
            "Image up to date: {} (sha256: {}...)",
            meta.version,
            &meta.sha256[..12]
        );
        return Ok(());
    }

    if args.check {
        println!("Image not cached. Run `polis init` to download.");
        return Ok(());
    }

    let meta = acquire_image(&source, &images_dir)?;
    write_metadata(&images_dir, &meta)?;
    println!("Run 'polis run' to create a workspace.");
    Ok(())
}

/// Resolve `--image` flag into an [`ImageSource`].
///
/// # Errors
///
/// Returns an error if the path does not exist or is not a regular file.
fn resolve_source(image: Option<&str>) -> Result<ImageSource> {
    match image {
        None => Ok(ImageSource::GitHubLatest),
        Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
            Ok(ImageSource::HttpUrl(s.to_string()))
        }
        Some(s) => {
            let path = PathBuf::from(s);
            anyhow::ensure!(path.exists(), "Image file not found: {}", path.display());
            anyhow::ensure!(path.is_file(), "Not a regular file: {}", path.display());
            Ok(ImageSource::LocalFile(path))
        }
    }
}

/// Acquire the image from the resolved source into `images_dir`.
///
/// Delegates to stub helpers that will be filled by issues 02–04.
///
/// # Errors
///
/// Returns an error if acquisition or verification fails.
fn acquire_image(source: &ImageSource, images_dir: &Path) -> Result<ImageMetadata> {
    let dest = images_dir.join(IMAGE_FILENAME);
    let sidecar = images_dir.join(SIDECAR_FILENAME);

    match source {
        ImageSource::LocalFile(path) => {
            std::fs::copy(path, &dest)
                .with_context(|| format!("copying {} to {}", path.display(), dest.display()))?;
            let source_str = path.to_string_lossy().into_owned();
            let sha256 = verify_image_integrity(&dest, &sidecar)?;
            Ok(ImageMetadata {
                version: "local".to_string(),
                sha256,
                arch: current_arch()?.to_string(),
                downloaded_at: Utc::now(),
                source: source_str,
            })
        }
        ImageSource::HttpUrl(url) => {
            download_with_resume(url, &dest, &DownloadContext { quiet: false })?;
            let sha256 = verify_image_integrity(&dest, &sidecar)?;
            Ok(ImageMetadata {
                version: "unknown".to_string(),
                sha256,
                arch: current_arch()?.to_string(),
                downloaded_at: Utc::now(),
                source: url.clone(),
            })
        }
        ImageSource::GitHubLatest => {
            let resolved = resolve_latest_image_url()?;
            download_with_resume(&resolved.image_url, &dest, &DownloadContext { quiet: false })?;
            download_with_resume(
                &resolved.checksum_url,
                &sidecar,
                &DownloadContext { quiet: false },
            )?;
            let sha256 = verify_image_integrity(&dest, &sidecar)?;
            Ok(ImageMetadata {
                version: resolved.tag,
                sha256,
                arch: current_arch()?.to_string(),
                downloaded_at: Utc::now(),
                source: resolved.image_url,
            })
        }
    }
}

/// Returns `~/.polis/images/`.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
fn images_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    Ok(home.join(".polis").join("images"))
}

/// Load existing [`ImageMetadata`] from `image.json`, if present.
///
/// Returns `None` if the file does not exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
fn load_metadata(images_dir: &Path) -> Result<Option<ImageMetadata>> {
    let path = images_dir.join(METADATA_FILENAME);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let meta = serde_json::from_str(&content)
        .with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(meta))
}

/// Write [`ImageMetadata`] to `image.json`.
///
/// # Errors
///
/// Returns an error if serialization or the write fails.
fn write_metadata(images_dir: &Path, meta: &ImageMetadata) -> Result<()> {
    let path = images_dir.join(METADATA_FILENAME);
    let content = serde_json::to_string_pretty(meta).context("serializing image metadata")?;
    std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))
}

/// Return the architecture suffix used in release asset names.
///
/// # Errors
///
/// Returns an error if the current architecture is not `x86_64` or `aarch64`.
fn current_arch() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        other => anyhow::bail!("unsupported architecture: {other}"),
    }
}

// ── Stubs (filled by issues 02–04) ──────────────────────────────────────────

// ── Download (issue 02) ──────────────────────────────────────────────────────

/// Download `url` to `dest` with HTTP Range resume support.
///
/// Writes to `{dest}.partial` during download, renames to `dest` on success.
/// Resumes an interrupted download if a `.partial` file already exists.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the file cannot be written.
pub(crate) fn download_with_resume(url: &str, dest: &Path, ctx: &DownloadContext) -> Result<()> {
    let partial = partial_path(dest);
    let existing = partial.metadata().map(|m| m.len()).unwrap_or(0);
    do_download(url, dest, &partial, existing, ctx, true)
}

fn partial_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".partial");
    PathBuf::from(s)
}

fn do_download(
    url: &str,
    dest: &Path,
    partial: &Path,
    existing: u64,
    ctx: &DownloadContext,
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
            return do_download(url, dest, partial, 0, ctx, false);
        }
        Err(ureq::Error::Status(code, _)) => anyhow::bail!("download failed: HTTP {code}"),
        Err(_) => anyhow::bail!("download interrupted"),
    };

    let status = response.status();
    let (mut file, start_pos) = if status == 206 {
        let f = OpenOptions::new()
            .append(true)
            .create(true)
            .open(partial)
            .context("opening partial file")?;
        (f, existing)
    } else if status == 200 {
        let f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(partial)
            .context("opening partial file")?;
        (f, 0_u64)
    } else {
        anyhow::bail!("download failed: HTTP {status}");
    };

    let total = parse_content_length(&response, start_pos);

    // Stale partial: existing bytes >= total → restart fresh.
    if start_pos > 0
        && let Some(t) = total
        && start_pos >= t
    {
        drop(file);
        std::fs::remove_file(partial).ok();
        return do_download(url, dest, partial, 0, ctx, false);
    }

    let pb = if ctx.quiet {
        indicatif::ProgressBar::hidden()
    } else if let Some(t) = total {
        let pb = indicatif::ProgressBar::new(t);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("[{bar:40}] {percent}% ({bytes}/{total_bytes})")
                .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar())
                .progress_chars("█▓░"),
        );
        pb.set_position(start_pos);
        pb
    } else {
        indicatif::ProgressBar::new_spinner()
    };

    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        let n = reader.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("download interrupted")?;
        pb.inc(n as u64);
    }

    pb.finish_and_clear();
    drop(file);

    std::fs::rename(partial, dest).context("failed to finalize downloaded image")?;
    Ok(())
}

/// Parse total content length from response headers.
///
/// For 206 Partial Content, returns `existing_bytes + Content-Length`.
/// For 200 OK, returns `Content-Length`.
fn parse_content_length(response: &ureq::Response, existing_bytes: u64) -> Option<u64> {
    let len = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())?;
    if response.status() == 206 {
        Some(existing_bytes + len)
    } else {
        Some(len)
    }
}

/// Verify the image at `image_path` against the signed sidecar at `sidecar_path`.
///
/// Returns the hex-encoded SHA-256 of the image on success.
///
/// # Errors
///
/// Returns an error if the sidecar cannot be read, the signature is invalid,
/// the checksum is malformed, or the image hash does not match.
pub(crate) fn verify_image_integrity(image_path: &Path, sidecar_path: &Path) -> Result<String> {
    let sidecar_bytes = std::fs::read(sidecar_path)
        .with_context(|| format!("failed to read checksum file: {}", sidecar_path.display()))?;

    let key_bytes =
        base64_decode(POLIS_PUBLIC_KEY_B64).context("invalid embedded public key")?;
    anyhow::ensure!(key_bytes.len() == PUBLIC_KEY_LENGTH, "public key length mismatch");
    let key_array: [u8; PUBLIC_KEY_LENGTH] = key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid embedded public key"))?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_array).context("invalid embedded public key")?;

    let mut cursor = Cursor::new(&sidecar_bytes);
    verify_tar(&mut cursor, &[verifying_key], None).context(
        "image checksum signature verification failed \
         — the checksum file may have been tampered with",
    )?;

    let expected = extract_checksum_from_signed_file(sidecar_path)?;

    println!("  Verifying checksum...");
    let actual = sha256_file(image_path)?;

    anyhow::ensure!(
        actual == expected,
        "image SHA256 mismatch\n  Expected: {expected}\n  Actual:   {actual}\n\nRe-download with: polis init --force"
    );

    Ok(actual)
}

/// Compute the full-file SHA-256 hash of a file, reading in 64 KiB chunks.
///
/// Returns the lowercase hex-encoded hash string.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or read.
pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open image file: {}", path.display()))?;
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

/// Extract the 64-character hex SHA-256 from a `sha256sum`-format sidecar file.
///
/// Expected format: `<64-hex>  <filename>\n`
///
/// # Errors
///
/// Returns an error if the file cannot be read or the checksum is malformed.
fn extract_checksum_from_signed_file(checksum_path: &Path) -> Result<String> {
    let content = std::fs::read_to_string(checksum_path)
        .with_context(|| format!("failed to read checksum file: {}", checksum_path.display()))?;
    let hex = content
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("malformed checksum file: expected 64-character hex SHA-256"))?;
    anyhow::ensure!(
        hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
        "malformed checksum file: expected 64-character hex SHA-256"
    );
    Ok(hex.to_string())
}

/// Resolved release information from GitHub.
#[derive(Debug)]
pub struct ResolvedRelease {
    /// Version tag (e.g., `"v0.3.0"`).
    pub tag: String,
    /// Direct download URL for the `.qcow2` image asset.
    pub image_url: String,
    /// Direct download URL for the `.sha256` sidecar asset.
    pub checksum_url: String,
}

/// GitHub Releases API endpoint — up to 10 most recent releases.
const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/OdraLabsHQ/polis/releases?per_page=10";

/// Resolve the latest image URL and version tag from the GitHub API.
///
/// Queries the 10 most recent releases and returns the first one that
/// contains both a `.qcow2` and a `.sha256` asset for the current arch.
///
/// # Errors
///
/// Returns an error if the API is rate-limited, unreachable, returns
/// invalid JSON, or no matching release is found.
pub(crate) fn resolve_latest_image_url() -> Result<ResolvedRelease> {
    let arch = current_arch()?;

    let url = std::env::var("POLIS_GITHUB_API_URL")
        .unwrap_or_else(|_| GITHUB_RELEASES_URL.to_string());

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
        Ok(resp) => {
            let s = resp.into_string().context("failed to read GitHub API response")?;
            serde_json::from_str(&s).context("failed to parse GitHub API response")?
        }
        Err(ureq::Error::Status(403, _)) => anyhow::bail!(
            "GitHub API rate limit exceeded (60 requests/hour unauthenticated).\nSet GITHUB_TOKEN env var or use: polis init --image <direct-url>"
        ),
        Err(ureq::Error::Status(404, _)) => {
            anyhow::bail!("GitHub repository not found: OdraLabsHQ/polis")
        }
        Err(ureq::Error::Status(code, _)) => anyhow::bail!("GitHub API error: HTTP {code}"),
        Err(e) => return Err(anyhow::anyhow!(e)),
    };

    parse_releases(&body, arch)
}

/// Scan a GitHub releases JSON array for the first release containing both
/// a `*-{arch}.qcow2` and a `*-{arch}.qcow2.sha256` asset.
///
/// # Errors
///
/// Returns an error if `releases` is not a JSON array or no matching release
/// is found.
fn parse_releases(releases: &serde_json::Value, arch: &str) -> Result<ResolvedRelease> {
    let qcow2_suffix = format!("-{arch}.qcow2");
    let sha256_suffix = format!("-{arch}.qcow2.sha256");

    let releases = releases
        .as_array()
        .context("failed to parse GitHub API response")?;

    for release in releases {
        let tag = release["tag_name"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let Some(assets) = release["assets"].as_array() else {
            continue;
        };

        let find_url = |suffix: &str| {
            assets
                .iter()
                .find(|a: &&serde_json::Value| {
                    a["name"]
                        .as_str()
                        .is_some_and(|n| n.ends_with(suffix))
                })
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

    anyhow::bail!(
        "No VM image found in recent GitHub releases.\nUse: polis init --image <url>"
    )
}

// ============================================================================
// Unit + Property Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── resolve_source ───────────────────────────────────────────────────────

    #[test]
    fn test_resolve_source_none_returns_github_latest() {
        assert!(matches!(resolve_source(None).unwrap(), ImageSource::GitHubLatest));
    }

    #[test]
    fn test_resolve_source_http_url_returns_http_url_variant() {
        let src = resolve_source(Some("http://example.com/image.qcow2")).unwrap();
        assert!(matches!(src, ImageSource::HttpUrl(_)));
    }

    #[test]
    fn test_resolve_source_https_url_returns_http_url_variant() {
        let src = resolve_source(Some("https://example.com/image.qcow2")).unwrap();
        assert!(matches!(src, ImageSource::HttpUrl(_)));
    }

    #[test]
    fn test_resolve_source_https_url_preserves_url_string() {
        let url = "https://example.com/image.qcow2";
        let src = resolve_source(Some(url)).unwrap();
        let ImageSource::HttpUrl(got) = src else { panic!("expected HttpUrl") };
        assert_eq!(got, url);
    }

    #[test]
    fn test_resolve_source_nonexistent_path_returns_error() {
        let result = resolve_source(Some("/nonexistent/path/image.qcow2"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Image file not found"), "got: {msg}");
    }

    #[test]
    fn test_resolve_source_directory_path_returns_error() {
        let dir = TempDir::new().unwrap();
        let result = resolve_source(Some(dir.path().to_str().unwrap()));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Not a regular file"), "got: {msg}");
    }

    #[test]
    fn test_resolve_source_existing_file_returns_local_file_variant() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("image.qcow2");
        std::fs::write(&path, b"fake").unwrap();
        let src = resolve_source(Some(path.to_str().unwrap())).unwrap();
        assert!(matches!(src, ImageSource::LocalFile(_)));
    }

    // ── load_metadata / write_metadata roundtrip ─────────────────────────────

    #[test]
    fn test_load_metadata_missing_file_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = load_metadata(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_write_then_load_metadata_roundtrip() {
        let dir = TempDir::new().unwrap();
        let meta = ImageMetadata {
            version: "v0.3.0".to_string(),
            sha256: "abc123def456abc123def456abc123def456abc123def456abc123def456abc1".to_string(),
            arch: "amd64".to_string(),
            downloaded_at: Utc::now(),
            source: "https://example.com/image.qcow2".to_string(),
        };
        write_metadata(dir.path(), &meta).unwrap();
        let loaded = load_metadata(dir.path()).unwrap().expect("metadata should exist");
        assert_eq!(loaded.version, meta.version);
        assert_eq!(loaded.sha256, meta.sha256);
        assert_eq!(loaded.arch, meta.arch);
        assert_eq!(loaded.source, meta.source);
    }

    #[test]
    fn test_load_metadata_corrupt_json_returns_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(METADATA_FILENAME), b"not json").unwrap();
        assert!(load_metadata(dir.path()).is_err());
    }

    // ── current_arch ─────────────────────────────────────────────────────────

    #[test]
    fn test_current_arch_returns_known_value() {
        let arch = current_arch().unwrap();
        assert!(arch == "amd64" || arch == "arm64", "unexpected arch: {arch}");
    }

    // ── run — argument validation ─────────────────────────────────────────────

    #[test]
    fn test_run_check_and_force_together_returns_error() {
        // Mutual exclusion check fires before HOME is consulted — no env override needed.
        let args = InitArgs { image: None, force: true, check: true };
        let err = run(&args).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    // ── stubs return errors ───────────────────────────────────────────────────

    #[test]
    fn test_download_with_resume_returns_error_on_bad_url() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("out.qcow2");
        let ctx = DownloadContext { quiet: true };
        // No real server — expect a transport or HTTP error, not a panic.
        assert!(download_with_resume("https://127.0.0.1:1/img", &dest, &ctx).is_err());
    }

    #[test]
    fn test_verify_image_integrity_stub_returns_error() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("img.qcow2");
        let sidecar = dir.path().join("img.qcow2.sha256");
        assert!(verify_image_integrity(&img, &sidecar).is_err());
    }

    #[test]
    fn test_resolve_latest_image_url_stub_returns_error() {
        assert!(resolve_latest_image_url().is_err());
    }

    // ── sha256_file ───────────────────────────────────────────────────────────

    #[test]
    fn test_sha256_file_known_content_returns_correct_hash() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"hello").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_file_empty_file_returns_empty_hash() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_file_missing_file_returns_error() {
        let dir = TempDir::new().unwrap();
        let err = sha256_file(&dir.path().join("missing")).unwrap_err();
        assert!(
            err.to_string().contains("failed to open image file"),
            "got: {err}"
        );
    }

    // ── extract_checksum_from_signed_file ─────────────────────────────────────

    #[test]
    fn test_extract_checksum_valid_sha256sum_format_returns_hex() {
        let dir = TempDir::new().unwrap();
        let hex = "a".repeat(64);
        let path = dir.path().join("img.sha256");
        std::fs::write(&path, format!("{hex}  img.qcow2\n")).unwrap();
        assert_eq!(extract_checksum_from_signed_file(&path).unwrap(), hex);
    }

    #[test]
    fn test_extract_checksum_empty_file_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("img.sha256");
        std::fs::write(&path, b"").unwrap();
        let err = extract_checksum_from_signed_file(&path).unwrap_err();
        assert!(
            err.to_string().contains("malformed checksum file"),
            "got: {err}"
        );
    }

    #[test]
    fn test_extract_checksum_short_hex_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("img.sha256");
        std::fs::write(&path, "abc123  img.qcow2\n").unwrap();
        let err = extract_checksum_from_signed_file(&path).unwrap_err();
        assert!(
            err.to_string().contains("malformed checksum file"),
            "got: {err}"
        );
    }

    #[test]
    fn test_extract_checksum_non_hex_chars_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("img.sha256");
        // 64 chars but contains non-hex 'g'
        std::fs::write(&path, format!("{}  img.qcow2\n", "g".repeat(64))).unwrap();
        let err = extract_checksum_from_signed_file(&path).unwrap_err();
        assert!(
            err.to_string().contains("malformed checksum file"),
            "got: {err}"
        );
    }

    #[test]
    fn test_extract_checksum_missing_file_returns_error() {
        let dir = TempDir::new().unwrap();
        let err =
            extract_checksum_from_signed_file(&dir.path().join("missing")).unwrap_err();
        assert!(
            err.to_string().contains("failed to read checksum file"),
            "got: {err}"
        );
    }

    // ── verify_image_integrity ────────────────────────────────────────────────

    #[test]
    fn test_verify_image_integrity_invalid_signature_returns_error() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("img.qcow2");
        let sidecar = dir.path().join("img.qcow2.sha256");
        std::fs::write(&img, b"fake image").unwrap();
        std::fs::write(&sidecar, b"not a valid zipsign tar").unwrap();
        let err = verify_image_integrity(&img, &sidecar).unwrap_err();
        assert!(
            err.to_string()
                .contains("image checksum signature verification failed"),
            "got: {err}"
        );
    }

    // ── parse_releases ────────────────────────────────────────────────────────

    fn release_json(tag: &str, assets: &[(&str, &str)]) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "assets": assets.iter().map(|(name, url)| serde_json::json!({
                "name": name,
                "browser_download_url": url
            })).collect::<Vec<_>>()
        })
    }

    #[test]
    fn test_parse_releases_matching_pair_returns_resolved_release() {
        let json = serde_json::json!([release_json("v0.3.0", &[
            ("polis-workspace-v0.3.0-amd64.qcow2",        "https://example.com/img.qcow2"),
            ("polis-workspace-v0.3.0-amd64.qcow2.sha256", "https://example.com/img.sha256"),
        ])]);
        let r = parse_releases(&json, "amd64").unwrap();
        assert_eq!(r.tag, "v0.3.0");
        assert_eq!(r.image_url, "https://example.com/img.qcow2");
        assert_eq!(r.checksum_url, "https://example.com/img.sha256");
    }

    #[test]
    fn test_parse_releases_qcow2_without_sha256_skips_release() {
        let json = serde_json::json!([release_json("v0.3.0", &[
            ("polis-workspace-v0.3.0-amd64.qcow2", "https://example.com/img.qcow2"),
        ])]);
        let err = parse_releases(&json, "amd64").unwrap_err();
        assert!(err.to_string().contains("No VM image found"), "got: {err}");
    }

    #[test]
    fn test_parse_releases_empty_array_returns_error() {
        let err = parse_releases(&serde_json::json!([]), "amd64").unwrap_err();
        assert!(err.to_string().contains("No VM image found"), "got: {err}");
    }

    #[test]
    fn test_parse_releases_not_an_array_returns_error() {
        let err = parse_releases(&serde_json::json!({}), "amd64").unwrap_err();
        assert!(
            err.to_string().contains("failed to parse GitHub API response"),
            "got: {err}"
        );
    }

    #[test]
    fn test_parse_releases_first_matching_release_wins() {
        let json = serde_json::json!([
            release_json("v0.3.0", &[
                ("polis-workspace-v0.3.0-amd64.qcow2",        "https://example.com/v0.3.0.qcow2"),
                ("polis-workspace-v0.3.0-amd64.qcow2.sha256", "https://example.com/v0.3.0.sha256"),
            ]),
            release_json("v0.2.0", &[
                ("polis-workspace-v0.2.0-amd64.qcow2",        "https://example.com/v0.2.0.qcow2"),
                ("polis-workspace-v0.2.0-amd64.qcow2.sha256", "https://example.com/v0.2.0.sha256"),
            ]),
        ]);
        let r = parse_releases(&json, "amd64").unwrap();
        assert_eq!(r.tag, "v0.3.0");
    }

    #[test]
    fn test_parse_releases_skips_wrong_arch_assets() {
        let json = serde_json::json!([release_json("v0.3.0", &[
            ("polis-workspace-v0.3.0-arm64.qcow2",        "https://example.com/arm64.qcow2"),
            ("polis-workspace-v0.3.0-arm64.qcow2.sha256", "https://example.com/arm64.sha256"),
        ])]);
        let err = parse_releases(&json, "amd64").unwrap_err();
        assert!(err.to_string().contains("No VM image found"), "got: {err}");
    }

    // ── partial_path ──────────────────────────────────────────────────────────

    #[test]
    fn test_partial_path_appends_dot_partial() {
        let dest = PathBuf::from("/tmp/polis-workspace.qcow2");
        let p = partial_path(&dest);
        assert_eq!(p, PathBuf::from("/tmp/polis-workspace.qcow2.partial"));
    }

    // ── download_with_resume — HTTP behaviour ─────────────────────────────────

    /// Spin up a minimal HTTP/1.1 server that serves `responses` in order,
    /// one per accepted connection. Returns the bound port.
    fn serve_responses(responses: Vec<Vec<u8>>) -> u16 {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for resp in responses {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf);
                    let _ = stream.write_all(&resp);
                }
            }
        });
        port
    }

    fn http_200(body: &[u8]) -> Vec<u8> {
        let mut r = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn http_206(body: &[u8]) -> Vec<u8> {
        let mut r = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn http_status(code: u16, reason: &str) -> Vec<u8> {
        format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .into_bytes()
    }

    #[test]
    fn test_download_200_ok_creates_dest_with_correct_content() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("img.qcow2");
        let body = b"fake image content";
        let port = serve_responses(vec![http_200(body)]);
        let ctx = DownloadContext { quiet: true };

        download_with_resume(&format!("http://127.0.0.1:{port}/img"), &dest, &ctx).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[test]
    fn test_download_200_ok_no_partial_file_remains() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("img.qcow2");
        let port = serve_responses(vec![http_200(b"data")]);
        let ctx = DownloadContext { quiet: true };

        download_with_resume(&format!("http://127.0.0.1:{port}/img"), &dest, &ctx).unwrap();

        assert!(!partial_path(&dest).exists());
    }

    #[test]
    fn test_download_non2xx_returns_error_with_status_code() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("img.qcow2");
        let port = serve_responses(vec![http_status(404, "Not Found")]);
        let ctx = DownloadContext { quiet: true };

        let err = download_with_resume(&format!("http://127.0.0.1:{port}/img"), &dest, &ctx)
            .unwrap_err();
        assert!(err.to_string().contains("download failed: HTTP 404"), "got: {err}");
    }

    #[test]
    fn test_download_416_deletes_partial_and_retries_with_200() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("img.qcow2");
        // Pre-create a partial file so a Range header is sent.
        let partial = partial_path(&dest);
        std::fs::write(&partial, b"stale").unwrap();

        let body = b"fresh content";
        let port = serve_responses(vec![
            http_status(416, "Range Not Satisfiable"),
            http_200(body),
        ]);
        let ctx = DownloadContext { quiet: true };

        download_with_resume(&format!("http://127.0.0.1:{port}/img"), &dest, &ctx).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!partial.exists());
    }

    #[test]
    fn test_download_206_appends_to_existing_partial() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("img.qcow2");
        let partial = partial_path(&dest);
        std::fs::write(&partial, b"hello").unwrap();

        let port = serve_responses(vec![http_206(b" world")]);
        let ctx = DownloadContext { quiet: true };

        download_with_resume(&format!("http://127.0.0.1:{port}/img"), &dest, &ctx).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"hello world");
    }

    #[test]
    fn test_download_200_after_range_request_truncates_and_succeeds() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("img.qcow2");
        let partial = partial_path(&dest);
        // Old partial content that must NOT appear in the final file.
        std::fs::write(&partial, b"old stale data").unwrap();

        let body = b"new full content";
        let port = serve_responses(vec![http_200(body)]);
        let ctx = DownloadContext { quiet: true };

        download_with_resume(&format!("http://127.0.0.1:{port}/img"), &dest, &ctx).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    // ── property tests ────────────────────────────────────────────────────────

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn prop_partial_path_always_ends_with_dot_partial(
                name in "[a-z0-9._-]{1,30}"
            ) {
                let dest = PathBuf::from(format!("/tmp/{name}"));
                let p = partial_path(&dest);
                let s = p.to_string_lossy();
                prop_assert!(s.ends_with(".partial"), "got: {s}");
            }

            /// Any byte content is preserved exactly after a 200 download.
            #[test]
            fn prop_download_any_content_preserved(
                body in prop::collection::vec(0u8..=255, 0..512)
            ) {
                let dir = TempDir::new().expect("tempdir");
                let dest = dir.path().join("img.qcow2");
                let port = serve_responses(vec![http_200(&body)]);
                let ctx = DownloadContext { quiet: true };
                download_with_resume(&format!("http://127.0.0.1:{port}/img"), &dest, &ctx)
                    .expect("download");
                prop_assert_eq!(std::fs::read(&dest).expect("read"), body);
            }

            /// Any http/https URL is resolved to HttpUrl without error.
            #[test]
            fn prop_resolve_source_http_url_always_succeeds(
                path in "[a-z0-9/._-]{1,40}"
            ) {
                for scheme in &["http", "https"] {
                    let url = format!("{scheme}://example.com/{path}");
                    let result = resolve_source(Some(&url));
                    prop_assert!(result.is_ok(), "url={url}");
                    prop_assert!(matches!(result.unwrap(), ImageSource::HttpUrl(_)));
                }
            }

            /// Metadata roundtrip preserves all string fields.
            #[test]
            fn prop_metadata_roundtrip_preserves_fields(
                version in "[a-z0-9._-]{1,20}",
                sha256 in "[a-f0-9]{64}",
                arch in "[a-z0-9]{3,10}",
                source in "https://[a-z]{3,10}\\.com/[a-z]{1,10}",
            ) {
                let dir = TempDir::new().expect("tempdir");
                let meta = ImageMetadata {
                    version: version.clone(),
                    sha256: sha256.clone(),
                    arch: arch.clone(),
                    downloaded_at: Utc::now(),
                    source: source.clone(),
                };
                write_metadata(dir.path(), &meta).expect("write");
                let loaded = load_metadata(dir.path()).expect("load").expect("some");
                prop_assert_eq!(loaded.version, version);
                prop_assert_eq!(loaded.sha256, sha256);
                prop_assert_eq!(loaded.arch, arch);
                prop_assert_eq!(loaded.source, source);
            }

            /// sha256_file output is always 64 lowercase hex characters.
            #[test]
            fn prop_sha256_file_output_is_64_lowercase_hex(
                content in prop::collection::vec(any::<u8>(), 0..1024)
            ) {
                let dir = TempDir::new().expect("tempdir");
                let path = dir.path().join("f");
                std::fs::write(&path, &content).expect("write");
                let hash = sha256_file(&path).expect("hash");
                prop_assert_eq!(hash.len(), 64);
                prop_assert!(
                    hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                    "non-lowercase-hex in output: {hash}"
                );
            }

            /// sha256_file is deterministic: same content always yields the same hash.
            #[test]
            fn prop_sha256_file_deterministic(
                content in prop::collection::vec(any::<u8>(), 0..512)
            ) {
                let dir = TempDir::new().expect("tempdir");
                let path = dir.path().join("f");
                std::fs::write(&path, &content).expect("write");
                let h1 = sha256_file(&path).expect("hash1");
                let h2 = sha256_file(&path).expect("hash2");
                prop_assert_eq!(h1, h2);
            }

            /// extract_checksum roundtrips any valid 64-char lowercase hex string.
            #[test]
            fn prop_extract_checksum_valid_hex_roundtrip(
                hex in "[a-f0-9]{64}"
            ) {
                let dir = TempDir::new().expect("tempdir");
                let path = dir.path().join("img.sha256");
                std::fs::write(&path, format!("{hex}  img.qcow2\n")).expect("write");
                let extracted = extract_checksum_from_signed_file(&path).expect("extract");
                prop_assert_eq!(extracted, hex);
            }

            /// parse_releases returns the exact URLs present in the JSON for any
            /// valid tag / URL combination.
            #[test]
            fn prop_parse_releases_valid_release_returns_correct_urls(
                tag     in "[a-z0-9._-]{1,20}",
                img_url in "https://[a-z]{3,10}\\.com/[a-z0-9]{1,20}\\.qcow2",
                sha_url in "https://[a-z]{3,10}\\.com/[a-z0-9]{1,20}\\.sha256",
            ) {
                let json = serde_json::json!([{
                    "tag_name": tag,
                    "assets": [
                        {"name": format!("polis-workspace-{tag}-amd64.qcow2"),        "browser_download_url": img_url},
                        {"name": format!("polis-workspace-{tag}-amd64.qcow2.sha256"), "browser_download_url": sha_url},
                    ]
                }]);
                let r = parse_releases(&json, "amd64").expect("should find release");
                prop_assert_eq!(&r.tag, &tag);
                prop_assert_eq!(&r.image_url, &img_url);
                prop_assert_eq!(&r.checksum_url, &sha_url);
            }
        }
    }
}
