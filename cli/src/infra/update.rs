//! Update infrastructure — implements `UpdateChecker` using GitHub releases.

use std::io::{Read, Seek, Write};

use anyhow::{Context, Result};
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::application::ports::{UpdateChecker, UpdateInfo, VerifiedAsset};

/// The base64-encoded ed25519 public key used to verify release signatures.
pub const POLIS_PUBLIC_KEY_B64: &str = "jI42dOaR/5mN1T0hH+QeWc+L0aH9BwG1L7Yd/4O5QeQ=";

/// Maximum download size for release assets (100 MB).
const MAX_DOWNLOAD_SIZE_BYTES: u64 = 100 * 1024 * 1024;

/// Uses GitHub releases API to check and apply updates.
#[derive(Clone, Copy)]
pub struct GithubUpdateChecker;

impl UpdateChecker for GithubUpdateChecker {
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn check(&self, current: &str) -> Result<UpdateInfo> {
        let releases = self_update::backends::github::ReleaseList::configure()
            .repo_owner("OdraLabsHQ")
            .repo_name("polis")
            .build()
            .context("failed to configure update check")?
            .fetch()
            .context("failed to check for updates")?;

        let Some(latest) = releases.first() else {
            return Ok(UpdateInfo::UpToDate);
        };

        let latest_version = latest.version.trim_start_matches('v');
        let latest_ver = semver::Version::parse(latest_version)
            .with_context(|| format!("invalid release version: {latest_version}"))?;
        let current_ver = semver::Version::parse(current)
            .with_context(|| format!("invalid current version: {current}"))?;

        if latest_ver <= current_ver {
            return Ok(UpdateInfo::UpToDate);
        }

        let release_notes = latest
            .body
            .as_deref()
            .map(parse_release_notes)
            .unwrap_or_default();

        let asset_name = get_asset_name();
        let download_url = latest
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .map(|a| a.download_url.clone())
            .ok_or_else(|| anyhow::anyhow!("no release asset for this platform ({asset_name})"))?;

        Ok(UpdateInfo::Available {
            version: latest_version.to_string(),
            release_notes,
            download_url,
        })
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn download_and_verify(&self, download_url: &str) -> Result<VerifiedAsset> {
        // 1. Download binary to temp file (not memory)
        let mut temp_file =
            tempfile::NamedTempFile::new().context("creating temporary file for download")?;

        // Note: download_to_file adds its own context, so we don't wrap here
        download_to_file(download_url, temp_file.as_file_mut())?;

        // 2. Download .sha256 checksum file
        // Note: download_checksum adds its own context
        let checksum_url = format!("{download_url}.sha256");
        let expected_checksum = download_checksum(&checksum_url)?;

        // 3. Verify SHA-256 checksum against downloaded binary
        temp_file
            .as_file_mut()
            .rewind()
            .context("rewinding temp file for checksum")?;
        // Note: compute_sha256 adds its own context
        let actual_checksum = compute_sha256(temp_file.as_file_mut())?;

        if actual_checksum != expected_checksum {
            anyhow::bail!("checksum mismatch: expected {expected_checksum}, got {actual_checksum}");
        }

        // 4. Verify ed25519 signature using zipsign-api
        temp_file
            .as_file_mut()
            .rewind()
            .context("rewinding temp file for signature verification")?;
        // Note: verify_embedded_signature adds its own context
        verify_embedded_signature(temp_file.as_file_mut(), download_url)?;

        // 5. Return VerifiedAsset with temp file path and sha256
        let temp_path = temp_file
            .into_temp_path()
            .keep()
            .context("persisting temp file")?;

        Ok(VerifiedAsset {
            temp_path,
            sha256: actual_checksum,
        })
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn install(&self, asset: VerifiedAsset) -> Result<()> {
        // Extract the binary from the archive to a temp file
        let binary_path = extract_binary_from_archive(&asset.temp_path)?;

        // Use self_replace::self_replace() for atomic binary replacement
        self_replace::self_replace(&binary_path).context("atomic binary replacement failed")?;

        // Clean up temp files
        // The original archive temp file
        if asset.temp_path.exists() {
            let _ = std::fs::remove_file(&asset.temp_path);
        }
        // The extracted binary temp file
        if binary_path.exists() {
            let _ = std::fs::remove_file(&binary_path);
        }

        Ok(())
    }
}

/// Returns the platform-specific asset name.
/// Uses compile-time cfg! checks for consistency.
#[must_use]
pub const fn get_asset_name() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "polis-linux-amd64.tar.gz"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "polis-linux-arm64.tar.gz"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "polis-darwin-amd64.tar.gz"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "polis-darwin-arm64.tar.gz"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "polis-windows-amd64.zip"
    }
}

pub(crate) fn parse_release_notes(body: &str) -> Vec<String> {
    body.lines()
        .filter(|l| l.starts_with("- ") || l.starts_with("* "))
        .map(|l| {
            l.strip_prefix("- ")
                .or_else(|| l.strip_prefix("* "))
                .unwrap_or(l)
                .to_string()
        })
        .take(5)
        .collect()
}

// ── Download and verification helpers ─────────────────────────────────────────

/// Download a file from a URL to a writer, respecting the size limit.
fn download_to_file(url: &str, writer: &mut impl Write) -> Result<()> {
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("downloading release asset from {url}"))?;

    let content_length = response
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());

    if let Some(len) = content_length
        && len > MAX_DOWNLOAD_SIZE_BYTES
    {
        anyhow::bail!("download size {len} exceeds limit of {MAX_DOWNLOAD_SIZE_BYTES} bytes");
    }

    let mut reader = response.into_reader().take(MAX_DOWNLOAD_SIZE_BYTES);
    std::io::copy(&mut reader, writer)
        .with_context(|| format!("writing downloaded content from {url}"))?;

    Ok(())
}

/// Download and parse a .sha256 checksum file.
/// Expected format: `<hex-checksum>  <filename>` or just `<hex-checksum>`.
fn download_checksum(url: &str) -> Result<String> {
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("downloading checksum from {url}"))?;

    let body = response
        .into_string()
        .with_context(|| format!("reading checksum response from {url}"))?;

    // Parse checksum - format is either "<hash>  <filename>" or just "<hash>"
    let checksum = body
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty checksum file at {url}"))?
        .to_lowercase();

    // Validate it looks like a hex SHA-256 (64 hex chars)
    if checksum.len() != 64 || !checksum.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!(
            "invalid checksum format at {url}: expected 64 hex characters, got {checksum}"
        );
    }

    Ok(checksum)
}

/// Compute the SHA-256 hash of a reader's contents.
fn compute_sha256(reader: &mut impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .context("reading file for checksum")?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash = hasher.finalize();
    Ok(hex_encode(&hash))
}

/// Verify the embedded ed25519 signature in a signed archive.
fn verify_embedded_signature(file: &mut (impl Read + Seek), download_url: &str) -> Result<()> {
    // Decode the public key using the standard base64 crate
    let public_key_bytes = base64::engine::general_purpose::STANDARD
        .decode(POLIS_PUBLIC_KEY_B64)
        .context("decoding public key from base64")?;

    let verifying_key = zipsign_api::VerifyingKey::try_from(public_key_bytes.as_slice())
        .map_err(|e| anyhow::anyhow!("invalid public key: {e}"))?;

    let keys = [verifying_key];

    // Determine archive type from URL and verify accordingly
    // Context is empty string as used in release signing
    let context: Option<&[u8]> = Some(b"");

    if download_url.ends_with(".tar.gz") {
        zipsign_api::verify::verify_tar(file, &keys, context)
            .map_err(|e| anyhow::anyhow!("signature verification failed: {e}"))?;
    } else if std::path::Path::new(download_url)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        zipsign_api::verify::verify_zip(file, &keys, context)
            .map_err(|e| anyhow::anyhow!("signature verification failed: {e}"))?;
    } else {
        anyhow::bail!("unsupported archive format for signature verification");
    }

    Ok(())
}

/// Encode bytes as lowercase hexadecimal.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// Extract the binary from a tar.gz or zip archive.
/// Returns the path to the extracted binary in a temp file.
fn extract_binary_from_archive(archive_path: &std::path::Path) -> Result<std::path::PathBuf> {
    let archive_name = archive_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // Determine binary name based on platform
    let binary_name = if cfg!(windows) { "polis.exe" } else { "polis" };

    // Create a temp file for the extracted binary
    let mut binary_temp =
        tempfile::NamedTempFile::new().context("creating temp file for extracted binary")?;

    if archive_name.ends_with(".tar.gz") || archive_path.to_string_lossy().ends_with(".tar.gz") {
        extract_from_tar_gz(archive_path, binary_name, binary_temp.as_file_mut())?;
    } else if std::path::Path::new(archive_name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        || archive_path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        extract_from_zip(archive_path, binary_name, binary_temp.as_file_mut())?;
    } else {
        // Try to detect format by reading magic bytes
        let file = std::fs::File::open(archive_path).context("opening archive to detect format")?;
        let mut reader = std::io::BufReader::new(file);
        let mut magic = [0u8; 2];
        reader
            .read_exact(&mut magic)
            .context("reading archive magic bytes")?;
        drop(reader);

        if magic == [0x1f, 0x8b] {
            // gzip magic bytes
            extract_from_tar_gz(archive_path, binary_name, binary_temp.as_file_mut())?;
        } else if magic == [0x50, 0x4b] {
            // zip magic bytes (PK)
            extract_from_zip(archive_path, binary_name, binary_temp.as_file_mut())?;
        } else {
            anyhow::bail!("unsupported archive format");
        }
    }

    // Persist the temp file so it's not deleted when dropped
    let binary_path = binary_temp
        .into_temp_path()
        .keep()
        .context("persisting extracted binary temp file")?;

    Ok(binary_path)
}

/// Extract a binary from a tar.gz archive.
fn extract_from_tar_gz(
    archive_path: &std::path::Path,
    binary_name: &str,
    writer: &mut impl Write,
) -> Result<()> {
    use flate2::read::GzDecoder;

    let file = std::fs::File::open(archive_path).context("opening tar.gz archive")?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries().context("reading tar entries")? {
        let mut entry = entry.context("reading tar entry")?;
        let path = entry.path().context("reading tar entry path")?;

        // Check if this entry is the binary we're looking for
        // It could be at the root or in a subdirectory
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if file_name == binary_name {
            std::io::copy(&mut entry, writer).context("extracting binary from tar.gz")?;
            return Ok(());
        }
    }

    anyhow::bail!("binary '{binary_name}' not found in tar.gz archive")
}

/// Extract a binary from a zip archive.
fn extract_from_zip(
    archive_path: &std::path::Path,
    binary_name: &str,
    writer: &mut impl Write,
) -> Result<()> {
    let file = std::fs::File::open(archive_path).context("opening zip archive")?;
    let mut archive = zip::ZipArchive::new(file).context("reading zip archive")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("reading zip entry")?;

        let path = match entry.enclosed_name() {
            Some(p) => p.clone(),
            None => continue,
        };

        // Check if this entry is the binary we're looking for
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if file_name == binary_name {
            std::io::copy(&mut entry, writer).context("extracting binary from zip")?;
            return Ok(());
        }
    }

    anyhow::bail!("binary '{binary_name}' not found in zip archive")
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // parse_release_notes — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_release_notes_dash_bullets_extracts_items() {
        let body = "- Improved credential detection\n- Faster workspace startup\n- Bug fixes";
        let notes = parse_release_notes(body);
        assert_eq!(
            notes,
            vec![
                "Improved credential detection",
                "Faster workspace startup",
                "Bug fixes"
            ]
        );
    }

    #[test]
    fn test_parse_release_notes_star_bullets_extracts_items() {
        let body = "* item one\n* item two";
        let notes = parse_release_notes(body);
        assert_eq!(notes, vec!["item one", "item two"]);
    }

    #[test]
    fn test_parse_release_notes_empty_body_returns_empty() {
        let notes = parse_release_notes("");
        assert!(notes.is_empty());
    }

    #[test]
    fn test_parse_release_notes_non_bullet_lines_are_ignored() {
        let body = "# v0.3.0\n\nSome prose.\n\n- actual item";
        let notes = parse_release_notes(body);
        assert_eq!(notes, vec!["actual item"]);
    }

    #[test]
    fn test_parse_release_notes_limits_to_five_items() {
        let body = (1..=10)
            .map(|i| format!("- item {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let notes = parse_release_notes(&body);
        assert_eq!(notes.len(), 5);
    }

    // -----------------------------------------------------------------------
    // get_asset_name — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_asset_name_current_platform_returns_archive() {
        let name = get_asset_name();
        let path = std::path::Path::new(&name);
        if cfg!(windows) {
            assert!(
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("zip")),
                "Windows asset name should be a .zip: {name}"
            );
        } else {
            assert!(
                name.ends_with(".tar.gz"),
                "Unix asset name should be a .tar.gz: {name}"
            );
        }
    }

    #[test]
    fn test_get_asset_name_linux_amd64_returns_correct_name() {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            let name = get_asset_name();
            assert_eq!(name, "polis-linux-amd64.tar.gz");
        }
    }
}
