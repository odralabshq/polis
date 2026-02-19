//! Init command — download and verify the workspace VM image.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Args;
use serde::{Deserialize, Serialize};

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
                arch: current_arch(),
                downloaded_at: Utc::now(),
                source: source_str,
            })
        }
        ImageSource::HttpUrl(url) => {
            download_with_resume(url, &dest)?;
            let sha256 = verify_image_integrity(&dest, &sidecar)?;
            Ok(ImageMetadata {
                version: "unknown".to_string(),
                sha256,
                arch: current_arch(),
                downloaded_at: Utc::now(),
                source: url.clone(),
            })
        }
        ImageSource::GitHubLatest => {
            let (url, version) = resolve_latest_image_url()?;
            download_with_resume(&url, &dest)?;
            let sha256 = verify_image_integrity(&dest, &sidecar)?;
            Ok(ImageMetadata {
                version,
                sha256,
                arch: current_arch(),
                downloaded_at: Utc::now(),
                source: url,
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

/// Returns the current CPU architecture string.
fn current_arch() -> String {
    if cfg!(target_arch = "aarch64") {
        "arm64".to_string()
    } else {
        "amd64".to_string()
    }
}

// ── Stubs (filled by issues 02–04) ──────────────────────────────────────────

/// Download `url` to `dest` with resume support.
///
/// Stub — implemented in issue 02.
///
/// # Errors
///
/// Returns an error if the download fails.
pub(crate) fn download_with_resume(url: &str, dest: &Path) -> Result<()> {
    anyhow::bail!(
        "download_with_resume not yet implemented (issue 02): url={url}, dest={}",
        dest.display()
    )
}

/// Verify the image at `image_path` against the signed sidecar at `sidecar_path`.
///
/// Returns the hex-encoded SHA-256 of the image on success.
///
/// Stub — implemented in issue 03.
///
/// # Errors
///
/// Returns an error if verification fails.
pub(crate) fn verify_image_integrity(image_path: &Path, sidecar_path: &Path) -> Result<String> {
    anyhow::bail!(
        "verify_image_integrity not yet implemented (issue 03): image={}, sidecar={}",
        image_path.display(),
        sidecar_path.display()
    )
}

/// Resolve the latest image URL and version tag from the GitHub API.
///
/// Returns `(url, version)`.
///
/// Stub — implemented in issue 04.
///
/// # Errors
///
/// Returns an error if the GitHub API call fails.
pub(crate) fn resolve_latest_image_url() -> Result<(String, String)> {
    anyhow::bail!("resolve_latest_image_url not yet implemented (issue 04)")
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
        let arch = current_arch();
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
    fn test_download_with_resume_stub_returns_error() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("out.qcow2");
        assert!(download_with_resume("https://example.com/img", &dest).is_err());
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

    // ── property tests ────────────────────────────────────────────────────────

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
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
        }
    }
}
