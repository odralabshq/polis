//! `polis update` — self-update with checksum and signature verification.

use std::io::{Cursor, Read};

use anyhow::{Context, Result};
use clap::Args;
use dialoguer::Confirm;
use owo_colors::OwoColorize;
use sha2::{Digest, Sha256};

use crate::multipass::Multipass;
use crate::output::OutputContext;
use crate::workspace::{digest, vm};

/// Arguments for the update command.
#[derive(Args)]
pub struct UpdateArgs {
    /// Check for updates without applying them
    #[arg(long)]
    pub check: bool,
}

/// Embedded ed25519 public key (base64) for verifying signed CLI release archives.
///
/// The corresponding private key is stored as `POLIS_SIGNING_KEY` in GitHub
/// Actions secrets and used by the release workflow to sign `.tar.gz` / `.zip`
/// archives via `zipsign`.
pub(crate) const POLIS_PUBLIC_KEY_B64: &str = "0p+AGW1jqNEos8o6cxDUl2objZhZFOXy4BQseFNHIqI=";

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

/// Checksum verification result.
pub struct SignatureInfo {
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

/// Run `polis update [--check]`.
///
/// Checks GitHub for a newer release, verifies its signature, prompts the user,
/// then downloads and replaces the current binary. If the VM is running, also
/// updates the VM config.
///
/// # Errors
///
/// Returns an error if the version check, signature verification, download, or
/// user prompt fails.
#[allow(clippy::unused_async)] // async contract: will gain awaits when download is made async
pub async fn run(
    args: &UpdateArgs,
    ctx: &OutputContext,
    checker: &impl UpdateChecker,
    mp: &impl Multipass,
) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    if !ctx.quiet {
        println!("Checking for updates...");
        println!();
    }

    let cli_update = checker.check(current)?;

    match &cli_update {
        UpdateInfo::UpToDate => {
            println!(
                "  {} CLI v{current} (latest)",
                "✓".style(ctx.styles.success),
            );
        }
        UpdateInfo::Available {
            version,
            release_notes,
            ..
        } => {
            println!("  CLI v{current} → v{version} available");
            if !release_notes.is_empty() {
                println!();
                println!("  Changes in v{version}:");
                for note in release_notes {
                    println!("    • {note}");
                }
            }
        }
    }

    if args.check {
        println!();
        println!("Run 'polis update' to apply the update.");
        return Ok(());
    }

    if matches!(cli_update, UpdateInfo::Available { .. }) {
        apply_cli_update(ctx, checker, cli_update)?;
    }

    // After CLI self-update, update VM config if the VM is running
    let vm_state = vm::state(mp).await?;
    if vm_state == vm::VmState::Running {
        if !ctx.quiet {
            println!();
            println!("Updating VM config...");
        }
        update_config(mp, ctx.quiet).await?;
    }

    println!();
    Ok(())
}

/// Update the VM config when the CLI has been updated to a new version.
///
/// Extracts embedded assets, computes the SHA256 of the new config tarball,
/// and compares it against the hash stored in the VM. If they differ, stops
/// services, transfers the new config, pulls images, verifies digests,
/// restarts services, and writes the new hash.
///
/// # Errors
///
/// Returns an error if any step of the update cycle fails.
pub async fn update_config(mp: &impl Multipass, quiet: bool) -> Result<()> {
    // 1. Extract embedded assets (new version's tarball)
    let (assets_dir, _guard) =
        crate::assets::extract_assets().context("extracting embedded assets")?;

    // 2. Compute SHA256 of the new config tarball
    let new_hash = vm::sha256_file(&assets_dir.join("polis-setup.config.tar"))
        .context("computing config tarball hash")?;

    // 3. Read current hash from VM
    let hash_output = mp
        .exec(&["cat", "/opt/polis/.config-hash"])
        .await
        .context("reading current config hash from VM")?;
    let current_hash = String::from_utf8_lossy(&hash_output.stdout)
        .trim()
        .to_string();

    // 4. If hashes match, config is up to date
    if new_hash == current_hash {
        if !quiet {
            println!("  Config is up to date");
        }
        return Ok(());
    }

    // 5. Hashes differ — perform full config update cycle
    if !quiet {
        println!("  Config changed — updating...");
    }

    // 5a. Stop services
    mp.exec(&[
        "docker",
        "compose",
        "-f",
        "/opt/polis/docker-compose.yml",
        "down",
    ])
    .await
    .context("stopping services")?;

    // 5b. Transfer new config
    let version = env!("CARGO_PKG_VERSION");
    vm::transfer_config(mp, &assets_dir, version)
        .await
        .context("transferring new config")?;

    // 5c. Pull new images
    vm::pull_images(mp).await.context("pulling Docker images")?;

    // 5d. Verify image digests
    digest::verify_image_digests(mp)
        .await
        .context("verifying image digests")?;

    // 5e. Restart services
    mp.exec(&[
        "docker",
        "compose",
        "-f",
        "/opt/polis/docker-compose.yml",
        "up",
        "-d",
    ])
    .await
    .context("restarting services")?;

    // 5f. Write new hash AFTER successful restart
    vm::write_config_hash(mp, &new_hash)
        .await
        .context("writing new config hash")?;

    if !quiet {
        println!("  Config updated successfully");
    }

    Ok(())
}

fn apply_cli_update(
    ctx: &OutputContext,
    checker: &impl UpdateChecker,
    cli_update: UpdateInfo,
) -> Result<()> {
    let UpdateInfo::Available {
        version,
        download_url,
        ..
    } = cli_update
    else {
        return Ok(());
    };

    println!();
    if !ctx.quiet {
        println!("  Verifying checksum...");
    }
    let sig = checker
        .verify_signature(&download_url)
        .context("checksum verification failed")?;

    let sha_preview = sig.sha256.get(..12).unwrap_or(&sig.sha256);
    println!(
        "    {} SHA-256: {sha_preview}...",
        "✓".style(ctx.styles.success),
    );
    println!();

    let confirmed = Confirm::new()
        .with_prompt("Update CLI now?")
        .default(true)
        .interact()
        .context("reading confirmation")?;

    if confirmed {
        if !ctx.quiet {
            println!("  Downloading...");
        }
        checker.perform_update(&version).context("update failed")?;
        println!();
        println!(
            "  {} CLI updated to v{version}",
            "✓".style(ctx.styles.success)
        );
        println!();
        println!("  Restart your terminal or run: exec polis");
    }
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

/// Verifies the SHA-256 checksum and ed25519 signature of a release asset.
///
/// Downloads the `.tar.gz` archive and its `.sha256` sidecar, verifies the
/// checksum matches, then verifies the embedded `zipsign` ed25519 signature
/// using the compile-time public key.
///
/// # Errors
///
/// Returns an error if download fails, checksum mismatches, or signature is
/// missing/invalid.
fn verify_signature(download_url: &str) -> Result<SignatureInfo> {
    // Download the release asset
    let response = ureq::get(download_url)
        .call()
        .context("failed to download release asset")?;

    let mut data = Vec::new();
    response
        .into_reader()
        .take(100 * 1024 * 1024)
        .read_to_end(&mut data)
        .context("failed to read release asset")?;

    // Compute SHA-256 hash
    let hash = Sha256::digest(&data);
    let actual_sha256 = hex_encode(&hash);

    // Download .sha256 file
    let checksum_url = format!("{download_url}.sha256");
    let checksum_response = ureq::get(&checksum_url)
        .call()
        .context("failed to download checksum file")?;

    let checksum_content = checksum_response
        .into_string()
        .context("failed to read checksum file")?;

    // Parse checksum (format: "hash  filename")
    let expected_sha256 = checksum_content
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid checksum file format"))?;

    anyhow::ensure!(
        actual_sha256 == expected_sha256,
        "checksum mismatch: expected {expected_sha256}, got {actual_sha256}"
    );

    // Verify zipsign ed25519 signature embedded in the .tar.gz
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

/// Encode bytes as lowercase hex string.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[(b >> 4) as usize]));
        out.push(char::from(HEX[(b & 0xf) as usize]));
    }
    out
}

/// Minimal base64 decoder (standard alphabet, no padding required).
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

    /// Stub multipass that fails on any call (for tests that shouldn't reach multipass).
    struct MultipassUnreachableStub;
    impl Multipass for MultipassUnreachableStub {
        async fn vm_info(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: vm_info not expected")
        }
        async fn launch(
            &self,
            _: &crate::multipass::LaunchParams<'_>,
        ) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: launch not expected")
        }
        async fn start(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: start not expected")
        }
        async fn stop(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: stop not expected")
        }
        async fn delete(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: delete not expected")
        }
        async fn purge(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: purge not expected")
        }
        async fn transfer(&self, _: &str, _: &str) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: transfer not expected")
        }
        async fn transfer_recursive(
            &self,
            _: &str,
            _: &str,
        ) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: transfer_recursive not expected")
        }
        async fn exec(&self, _: &[&str]) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: exec not expected")
        }
        async fn exec_with_stdin(
            &self,
            _: &[&str],
            _: &[u8],
        ) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: exec_with_stdin not expected")
        }
        fn exec_spawn(&self, _: &[&str]) -> anyhow::Result<tokio::process::Child> {
            anyhow::bail!("stub: exec_spawn not expected")
        }
        async fn version(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: version not expected")
        }
        async fn exec_status(&self, _: &[&str]) -> anyhow::Result<std::process::ExitStatus> {
            anyhow::bail!("stub: exec_status not expected")
        }
    }

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

        let args = UpdateArgs { check: true };
        let ctx = OutputContext::new(true, true);
        let result = run(&args, &ctx, &AlwaysUpToDate, &MultipassUnreachableStub).await;
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
                Err(anyhow::anyhow!("checksum verification failed"))
            }
            fn perform_update(&self, _version: &str) -> anyhow::Result<()> {
                unreachable!("should not update when checksum is invalid")
            }
        }

        let args = UpdateArgs { check: false };
        let ctx = OutputContext::new(true, true);
        let result = run(&args, &ctx, &BadSignature, &MultipassUnreachableStub).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("checksum"),
            "error should mention checksum"
        );
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
