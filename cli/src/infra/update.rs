//! Update infrastructure — implements `UpdateChecker` using GitHub releases.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};

use crate::application::services::update::{SignatureInfo, UpdateChecker, UpdateInfo};

/// The base64-encoded ed25519 public key used to verify release signatures.
pub const POLIS_PUBLIC_KEY_B64: &str = "jI42dOaR/5mN1T0hH+QeWc+L0aH9BwG1L7Yd/4O5QeQ=";

/// Uses GitHub releases API to check and apply updates.
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

        let asset_name = get_asset_name()?;
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
    fn verify_signature(&self, download_url: &str) -> Result<SignatureInfo> {
        let response = ureq::get(download_url)
            .call()
            .context("failed to download release asset")?;

        let mut data = Vec::new();
        response
            .into_reader()
            .take(100 * 1024 * 1024)
            .read_to_end(&mut data)
            .context("failed to read release asset")?;

        let hash = Sha256::digest(&data);
        let actual_sha256 = crate::domain::workspace::hex_encode(&hash);

        let checksum_url = format!("{download_url}.sha256");
        let checksum_response = ureq::get(&checksum_url)
            .call()
            .context("failed to download checksum file")?;

        let checksum_content = checksum_response
            .into_string()
            .context("failed to read checksum file")?;

        let expected_sha256 = checksum_content
            .split_whitespace()
            .next()
            .ok_or_else(|| anyhow::anyhow!("invalid checksum file format"))?;

        anyhow::ensure!(
            actual_sha256 == expected_sha256,
            "checksum mismatch: expected {expected_sha256}, got {actual_sha256}"
        );

        let public_key_bytes =
            base64_decode(POLIS_PUBLIC_KEY_B64).context("decoding embedded public key")?;
        let key_array: [u8; 32] = public_key_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
        let keys = zipsign_api::verify::collect_keys([Ok(key_array)])
            .map_err(|e| anyhow::anyhow!("invalid public key: {e}"))?;

        let mut cursor = Cursor::new(&data);
        zipsign_api::verify::verify_tar(&mut cursor, &keys, Some(b""))
            .map_err(|e| anyhow::anyhow!("signature verification failed: {e}"))?;

        Ok(SignatureInfo {
            sha256: actual_sha256,
        })
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn perform_update(&self, version: &str) -> Result<()> {
        let status = self_update::backends::github::Update::configure()
            .repo_owner("OdraLabsHQ")
            .repo_name("polis")
            .bin_name("polis")
            .show_download_progress(true)
            .current_version(env!("CARGO_PKG_VERSION"))
            .target_version_tag(&format!("v{version}"))
            .build()
            .context("failed to configure update")?
            .update()
            .context("failed to perform update")?;

        anyhow::ensure!(status.updated(), "update did not complete");
        Ok(())
    }
}

pub(crate) fn get_asset_name() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let name = match (os, arch) {
        ("linux", "x86_64") => "polis-linux-amd64.tar.gz",
        ("linux", "aarch64") => "polis-linux-arm64.tar.gz",
        ("macos", "x86_64") => "polis-darwin-amd64.tar.gz",
        ("macos", "aarch64") => "polis-darwin-arm64.tar.gz",
        ("windows", "x86_64") => "polis-windows-amd64.tar.gz",
        _ => anyhow::bail!("unsupported platform: {os}-{arch}"),
    };
    Ok(name.to_string())
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

pub(crate) fn base64_decode(input: &str) -> Result<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn decode_char(c: u8) -> Option<u8> {
        #[allow(clippy::cast_possible_truncation)]
        ALPHABET.iter().position(|&x| x == c).map(|p| p as u8)
    }

    let input = input.trim_end_matches('=');
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;

    for &byte in input.as_bytes() {
        let val = decode_char(byte).ok_or_else(|| anyhow::anyhow!("invalid base64 character"))?;
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            #[allow(clippy::cast_possible_truncation)]
            output.push((buf >> bits) as u8);
        }
    }

    Ok(output)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
mod tests {
    use super::*;
    use crate::domain::workspace::hex_encode;

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
    fn test_get_asset_name_current_platform_returns_tar_gz() {
        let name = get_asset_name().expect("current platform should be supported");
        assert!(
            name.ends_with(".tar.gz"),
            "asset name should be a .tar.gz: {name}"
        );
    }

    #[test]
    fn test_get_asset_name_linux_amd64_returns_correct_name() {
        if std::env::consts::OS == "linux" && std::env::consts::ARCH == "x86_64" {
            let name = get_asset_name().expect("linux-amd64 should be supported");
            assert_eq!(name, "polis-linux-amd64.tar.gz");
        }
    }

    // -----------------------------------------------------------------------
    // hex_encode — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_hex_encode_empty_returns_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_single_byte() {
        assert_eq!(hex_encode(&[0x00]), "00");
        assert_eq!(hex_encode(&[0xff]), "ff");
        assert_eq!(hex_encode(&[0xab]), "ab");
    }

    #[test]
    fn test_hex_encode_multiple_bytes() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }
}
