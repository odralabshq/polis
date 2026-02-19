//! `polis update` — self-update with signature verification (V-008).

use std::io::{Cursor, Read};

use anyhow::{Context, Result};
use dialoguer::Confirm;
use owo_colors::OwoColorize;
use sha2::{Digest, Sha256};
use zipsign_api::{PUBLIC_KEY_LENGTH, VerifyingKey, verify::verify_tar};

use crate::output::OutputContext;

/// Embedded ed25519 public key for release signature verification.
/// This key is set during the release build process.
/// Format: 32-byte ed25519 public key, base64-encoded.
const POLIS_PUBLIC_KEY_B64: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

/// Signer identity displayed to users.
const SIGNER_NAME: &str = "Odra Labs";

/// Short key fingerprint for display.
const KEY_FINGERPRINT: &str = "polis-release-v1";

// ── Public types ──────────────────────────────────────────────────────────────

/// Information about an available update.
pub enum UpdateInfo {
    /// A newer version is available.
    Available {
        /// The new version string (without leading `v`).
        version: String,
        /// Up to 5 bullet-point release notes.
        release_notes: Vec<String>,
        /// Direct download URL for the platform asset.
        download_url: String,
    },
    /// Already on the latest version.
    UpToDate,
}

/// Signature verification result.
pub struct SignatureInfo {
    /// Human-readable signer name.
    pub signer: String,
    /// Short key fingerprint.
    pub key_id: String,
    /// Hex-encoded SHA-256 of the downloaded asset.
    pub sha256: String,
}

/// Abstraction over the update backend, enabling test doubles.
pub trait UpdateChecker {
    /// Check whether a newer version is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the release list cannot be fetched or parsed.
    fn check(&self, current: &str) -> Result<UpdateInfo>;

    /// Verify the cryptographic signature of the release asset.
    ///
    /// # Errors
    ///
    /// Returns an error if the signature is missing or invalid.
    fn verify_signature(&self, download_url: &str) -> Result<SignatureInfo>;

    /// Download and replace the current binary.
    ///
    /// # Errors
    ///
    /// Returns an error if the download or binary replacement fails.
    fn perform_update(&self, version: &str) -> Result<()>;
}

/// Production implementation using GitHub releases.
pub struct GithubUpdateChecker;

impl UpdateChecker for GithubUpdateChecker {
    fn check(&self, current: &str) -> Result<UpdateInfo> {
        check_for_update(current)
    }

    fn verify_signature(&self, download_url: &str) -> Result<SignatureInfo> {
        verify_signature(download_url)
    }

    fn perform_update(&self, version: &str) -> Result<()> {
        perform_update(version)
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run `polis update`.
///
/// Checks GitHub for a newer release, verifies its signature, prompts the user,
/// then downloads and replaces the current binary.
///
/// # Errors
///
/// Returns an error if the version check, signature verification, download, or
/// user prompt fails.
#[allow(clippy::unused_async)] // async contract: will gain awaits when download is made async
pub async fn run(ctx: &OutputContext, checker: &impl UpdateChecker) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    if !ctx.quiet {
        println!("  Checking for updates...");
        println!();
    }

    match checker.check(current)? {
        UpdateInfo::UpToDate => {
            println!(
                "  {} You're running the latest version (v{current})",
                "✓".style(ctx.styles.success),
            );
        }
        UpdateInfo::Available {
            version,
            release_notes,
            download_url,
        } => {
            println!("  Current: v{current}");
            println!("  Latest:  v{version}");
            println!();

            if !release_notes.is_empty() {
                println!("  Changes in v{version}:");
                for note in &release_notes {
                    println!("    • {note}");
                }
                println!();
            }

            if !ctx.quiet {
                println!("  Verifying signature...");
            }
            let sig = checker
                .verify_signature(&download_url)
                .context("signature verification failed")?;

            let sha_preview = sig.sha256.get(..12).unwrap_or(&sig.sha256);
            println!(
                "    {} Signed by: {} (key: {})",
                "✓".style(ctx.styles.success),
                sig.signer,
                sig.key_id,
            );
            println!(
                "    {} SHA-256: {sha_preview}...",
                "✓".style(ctx.styles.success),
            );
            println!();

            let confirmed = Confirm::new()
                .with_prompt("Update now?")
                .default(true)
                .interact()
                .context("reading confirmation")?;

            if !confirmed {
                return Ok(());
            }

            if !ctx.quiet {
                println!("  Downloading...");
            }
            checker.perform_update(&version).context("update failed")?;

            println!();
            println!("  {} Updated to v{version}", "✓".style(ctx.styles.success),);
            println!();
            println!("  Restart your terminal or run: exec polis");
        }
    }

    println!();
    Ok(())
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn check_for_update(current: &str) -> Result<UpdateInfo> {
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

fn get_asset_name() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let name = match (os, arch) {
        ("linux", "x86_64") => "polis-linux-amd64.tar.gz",
        ("linux", "aarch64") => "polis-linux-arm64.tar.gz",
        ("macos", "x86_64") => "polis-darwin-amd64.tar.gz",
        ("macos", "aarch64") => "polis-darwin-arm64.tar.gz",
        _ => anyhow::bail!("unsupported platform: {os}-{arch}"),
    };
    Ok(name.to_string())
}

fn parse_release_notes(body: &str) -> Vec<String> {
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

/// Verifies the zipsign ed25519 signature of a release asset.
///
/// Downloads the asset, verifies the embedded signature against the embedded
/// public key, and computes the SHA-256 hash.
///
/// # Errors
///
/// Returns an error if download fails, signature is invalid, or no matching key.
fn verify_signature(download_url: &str) -> Result<SignatureInfo> {
    // Download the release asset
    let response = ureq::get(download_url)
        .call()
        .context("failed to download release asset")?;

    let mut data = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut data)
        .context("failed to read release asset")?;

    // Compute SHA-256 hash
    let hash = Sha256::digest(&data);
    let sha256 = hex_encode(&hash);

    // Decode the embedded public key
    let key_bytes =
        base64_decode(POLIS_PUBLIC_KEY_B64).context("invalid embedded public key encoding")?;

    anyhow::ensure!(
        key_bytes.len() == PUBLIC_KEY_LENGTH,
        "embedded public key has wrong length: expected {PUBLIC_KEY_LENGTH}, got {}",
        key_bytes.len()
    );

    let key_array: [u8; PUBLIC_KEY_LENGTH] = key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key conversion failed"))?;

    let verifying_key =
        VerifyingKey::from_bytes(&key_array).context("invalid embedded public key")?;

    // Verify the signature
    let mut cursor = Cursor::new(&data);
    verify_tar(&mut cursor, &[verifying_key], None).context("signature verification failed")?;

    Ok(SignatureInfo {
        signer: SIGNER_NAME.to_string(),
        key_id: KEY_FINGERPRINT.to_string(),
        sha256,
    })
}

/// Encode bytes as lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[(b >> 4) as usize]));
        out.push(char::from(HEX[(b & 0xf) as usize]));
    }
    out
}

/// Minimal base64 decoder (standard alphabet, no padding required).
fn base64_decode(input: &str) -> Result<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn decode_char(c: u8) -> Option<u8> {
        // SAFETY: ALPHABET has 64 entries, so position is always < 64 and fits in u8.
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
            // SAFETY: We're extracting exactly 8 bits after the shift.
            #[allow(clippy::cast_possible_truncation)]
            output.push((buf >> bits) as u8);
        }
    }

    Ok(output)
}

fn perform_update(version: &str) -> Result<()> {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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
    // run() via UpdateChecker trait mock — unit
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_run_up_to_date_returns_ok() {
        use crate::output::OutputContext;

        struct AlwaysUpToDate;
        impl UpdateChecker for AlwaysUpToDate {
            fn check(&self, _current: &str) -> anyhow::Result<UpdateInfo> {
                Ok(UpdateInfo::UpToDate)
            }
            fn verify_signature(&self, _url: &str) -> anyhow::Result<SignatureInfo> {
                unreachable!("should not verify when up to date")
            }
            fn perform_update(&self, _version: &str) -> anyhow::Result<()> {
                unreachable!("should not update when up to date")
            }
        }

        let ctx = OutputContext::new(true, true);
        let result = run(&ctx, &AlwaysUpToDate).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_invalid_signature_returns_err() {
        use crate::output::OutputContext;

        struct BadSignature;
        impl UpdateChecker for BadSignature {
            fn check(&self, _current: &str) -> anyhow::Result<UpdateInfo> {
                Ok(UpdateInfo::Available {
                    version: "9.9.9".to_string(),
                    release_notes: vec![],
                    download_url: "https://example.com/polis.tar.gz".to_string(),
                })
            }
            fn verify_signature(&self, _url: &str) -> anyhow::Result<SignatureInfo> {
                Err(anyhow::anyhow!("signature verification failed"))
            }
            fn perform_update(&self, _version: &str) -> anyhow::Result<()> {
                unreachable!("should not update when signature is invalid")
            }
        }

        let ctx = OutputContext::new(true, true);
        let result = run(&ctx, &BadSignature).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("signature"),
            "error should mention signature"
        );
    }

    // -----------------------------------------------------------------------
    // parse_release_notes — property
    // -----------------------------------------------------------------------

    proptest! {
        /// Output never exceeds 5 items regardless of input size.
        #[test]
        fn prop_parse_release_notes_output_never_exceeds_five(
            lines in proptest::collection::vec("[-*] [^\n]{0,80}", 0..20),
        ) {
            let body = lines.join("\n");
            let notes = parse_release_notes(&body);
            prop_assert!(notes.len() <= 5);
        }

        /// No output item retains its original bullet prefix.
        #[test]
        fn prop_parse_release_notes_items_have_no_bullet_prefix(
            lines in proptest::collection::vec(
                proptest::sample::select(vec!["- alpha", "- beta", "* gamma", "* delta"]),
                0..10,
            ),
        ) {
            let body = lines.join("\n");
            let notes = parse_release_notes(&body);
            for note in &notes {
                prop_assert!(
                    !note.starts_with("- ") && !note.starts_with("* "),
                    "item still has bullet prefix: {note:?}"
                );
            }
        }

        /// Non-bullet lines produce no output items.
        #[test]
        fn prop_parse_release_notes_non_bullet_lines_produce_no_items(
            body in "[^-*\n][^\n]{0,80}",
        ) {
            // A body whose first char is not '-' or '*' has no bullet lines.
            let notes = parse_release_notes(&body);
            prop_assert!(notes.is_empty());
        }

        /// run() with UpToDate checker always returns Ok for any current version string.
        #[test]
        fn prop_run_up_to_date_always_ok(version in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}") {
            struct UpToDate;
            impl UpdateChecker for UpToDate {
                fn check(&self, _: &str) -> anyhow::Result<UpdateInfo> { Ok(UpdateInfo::UpToDate) }
                fn verify_signature(&self, _: &str) -> anyhow::Result<SignatureInfo> { unreachable!() }
                fn perform_update(&self, _: &str) -> anyhow::Result<()> { unreachable!() }
            }

            let ctx = crate::output::OutputContext::new(true, true);
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let result = rt.block_on(run(&ctx, &UpToDate));
            prop_assert!(result.is_ok(), "expected Ok for version {version}");
        }

        /// run() with a failing verify_signature always returns Err mentioning "signature".
        #[test]
        fn prop_run_bad_signature_always_err(
            version in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
        ) {
            struct BadSig;
            impl UpdateChecker for BadSig {
                fn check(&self, _: &str) -> anyhow::Result<UpdateInfo> {
                    Ok(UpdateInfo::Available {
                        version: "9.9.9".to_string(),
                        release_notes: vec![],
                        download_url: "https://example.com/polis.tar.gz".to_string(),
                    })
                }
                fn verify_signature(&self, _: &str) -> anyhow::Result<SignatureInfo> {
                    Err(anyhow::anyhow!("signature verification failed"))
                }
                fn perform_update(&self, _: &str) -> anyhow::Result<()> { unreachable!() }
            }

            let ctx = crate::output::OutputContext::new(true, true);
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let result = rt.block_on(run(&ctx, &BadSig));
            prop_assert!(result.is_err());
            prop_assert!(
                result.unwrap_err().to_string().contains("signature"),
                "error for version {version} should mention signature"
            );
        }

        /// hex_encode output length is always 2x input length.
        #[test]
        fn prop_hex_encode_output_length(input in proptest::collection::vec(any::<u8>(), 0..256)) {
            let encoded = hex_encode(&input);
            prop_assert_eq!(encoded.len(), input.len() * 2);
        }

        /// hex_encode output contains only lowercase hex characters.
        #[test]
        fn prop_hex_encode_only_hex_chars(input in proptest::collection::vec(any::<u8>(), 0..256)) {
            let encoded = hex_encode(&input);
            prop_assert!(encoded.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        }

        /// base64_decode rejects invalid characters.
        #[test]
        fn prop_base64_decode_rejects_invalid_chars(invalid in "[^A-Za-z0-9+/=]+") {
            let result = base64_decode(&invalid);
            prop_assert!(result.is_err());
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

    // -----------------------------------------------------------------------
    // base64_decode — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_base64_decode_empty_returns_empty() {
        assert_eq!(base64_decode("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn test_base64_decode_simple() {
        // "SGVsbG8=" decodes to "Hello"
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello".to_vec());
    }

    #[test]
    fn test_base64_decode_no_padding() {
        // "SGVsbG8" (no padding) should also decode to "Hello"
        assert_eq!(base64_decode("SGVsbG8").unwrap(), b"Hello".to_vec());
    }

    #[test]
    fn test_base64_decode_32_bytes() {
        // 32 zero bytes in base64
        let zeros_b64 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let decoded = base64_decode(zeros_b64).unwrap();
        assert_eq!(decoded.len(), 32);
        assert!(decoded.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_base64_decode_invalid_char_returns_err() {
        assert!(base64_decode("!!!").is_err());
    }
}
