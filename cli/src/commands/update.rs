//! `polis update` — self-update with signature verification (V-008).

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use anyhow::{Context, Result};
use dialoguer::Confirm;
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zipsign_api::{
    PUBLIC_KEY_LENGTH, VerifyingKey, unsign::copy_and_unsign_tar, verify::verify_tar,
};

use crate::output::OutputContext;

/// Embedded ed25519 public key for release signature verification.
/// This key is set during the release build process.
/// Format: 32-byte ed25519 public key, base64-encoded.
pub(crate) const POLIS_PUBLIC_KEY_B64: &str = "0p+AGW1jqNEos8o6cxDUl2objZhZFOXy4BQseFNHIqI=";

/// Signer identity displayed to users.
const SIGNER_NAME: &str = "Odra Labs";

/// Short key fingerprint for display.
const KEY_FINGERPRINT: &str = "polis-release-v1";

// ── Manifest types ────────────────────────────────────────────────────────────

/// Signed versions manifest published as a GitHub release asset.
/// Controls which versions of each component are current.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct VersionsManifest {
    /// Schema version for forward compatibility.
    pub manifest_version: u32,
    /// VM image version info.
    pub vm_image: VmImageVersion,
    /// Container image versions, keyed by service name.
    pub containers: BTreeMap<String, String>,
}

/// VM image version within the manifest.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct VmImageVersion {
    /// Semver tag (e.g., `"v0.3.0"`).
    pub version: String,
    /// Asset filename (e.g., `"polis-v0.3.0-amd64.qcow2"`).
    pub asset: String,
}

/// Validate a version tag against strict semver format.
///
/// Accepted: `v0.3.0`, `v1.0.0-rc.1`, `v2.0.0-beta.3`
/// Rejected: `v0.3.1; curl evil.com`, `latest`, `v1`, empty string
///
/// Uses `semver::Version::parse()` after stripping the `v` prefix.
/// Pre-release identifiers are additionally checked to contain only
/// `[a-zA-Z0-9.]` characters (V-004).
///
/// # Errors
///
/// Returns an error if the tag does not match the expected semver pattern.
#[allow(dead_code)]
pub fn validate_version_tag(tag: &str) -> Result<()> {
    let bare = tag
        .strip_prefix('v')
        .ok_or_else(|| anyhow::anyhow!("invalid version tag: {tag}"))?;
    let ver =
        semver::Version::parse(bare).with_context(|| format!("invalid version tag: {tag}"))?;
    // Validate pre-release identifiers contain only [a-zA-Z0-9.]
    anyhow::ensure!(
        ver.pre.is_empty()
            || ver
                .pre
                .as_str()
                .chars()
                .all(|c: char| c.is_ascii_alphanumeric() || c == '.'),
        "invalid version tag: {tag}"
    );
    Ok(())
}

/// Download and verify the signed `versions.json` manifest from the latest
/// GitHub release.
///
/// # Verification chain
/// 1. Find `versions.json` asset in the latest release
/// 2. Download the signed tar
/// 3. Verify ed25519 signature via zipsign
/// 4. Extract JSON content from the tar
/// 5. Parse into [`VersionsManifest`]
/// 6. Validate `manifest_version == 1`
/// 7. Validate all version tags
///
/// # Errors
///
/// Returns an error if download fails, signature is invalid, JSON is
/// malformed, `manifest_version` is unsupported, or any version tag fails
/// validation.
#[allow(dead_code)]
pub fn load_versions_manifest() -> Result<VersionsManifest> {
    let url = std::env::var("POLIS_GITHUB_API_URL")
        .unwrap_or_else(|_| crate::commands::init::GITHUB_RELEASES_URL.to_string());

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
            let s = resp
                .into_string()
                .context("failed to read GitHub API response")?;
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

    let releases = body
        .as_array()
        .context("failed to parse GitHub API response")?;
    let latest = releases
        .first()
        .ok_or_else(|| anyhow::anyhow!("versions.json not found in latest release"))?;

    let assets = latest["assets"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("versions.json not found in latest release"))?;

    let download_url = assets
        .iter()
        .find(|a| a["name"].as_str() == Some("versions.json"))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| anyhow::anyhow!("versions.json not found in latest release"))?
        .to_string();

    // Download the signed tar
    let response = ureq::get(&download_url)
        .call()
        .context("failed to download versions.json")?;
    let mut signed_bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut signed_bytes)
        .context("failed to read versions.json")?;

    // Verify signature before trusting any content (V-002)
    let verifying_key = build_verifying_key()?;
    let mut cursor = Cursor::new(&signed_bytes);
    verify_tar(&mut cursor, &[verifying_key], None).context(
        "versions.json signature verification failed \
         — the manifest may have been tampered with",
    )?;

    // Extract JSON from the signed tar
    let json_bytes = extract_tar_content(&signed_bytes)?;
    let manifest: VersionsManifest = serde_json::from_slice(&json_bytes).with_context(|| {
        format!(
            "failed to parse versions.json: {}",
            String::from_utf8_lossy(&json_bytes)
        )
    })?;

    anyhow::ensure!(
        manifest.manifest_version == 1,
        "unsupported manifest version: {}. Update your CLI: polis update",
        manifest.manifest_version
    );

    validate_version_tag(&manifest.vm_image.version).with_context(|| {
        format!(
            "invalid version tag in manifest: {}",
            manifest.vm_image.version
        )
    })?;
    for version in manifest.containers.values() {
        validate_version_tag(version)
            .with_context(|| format!("invalid version tag in manifest: {version}"))?;
    }

    Ok(manifest)
}

/// Build the [`VerifyingKey`] from the embedded public key constant.
///
/// # Errors
///
/// Returns an error if the key is malformed.
#[allow(dead_code)]
fn build_verifying_key() -> Result<VerifyingKey> {
    let key_b64 = std::env::var("POLIS_VERIFYING_KEY_B64")
        .unwrap_or_else(|_| POLIS_PUBLIC_KEY_B64.to_string());
    let key_bytes = base64_decode(&key_b64).context("invalid embedded public key encoding")?;
    anyhow::ensure!(
        key_bytes.len() == PUBLIC_KEY_LENGTH,
        "embedded public key has wrong length: expected {PUBLIC_KEY_LENGTH}, got {}",
        key_bytes.len()
    );
    let key_array: [u8; PUBLIC_KEY_LENGTH] = key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key conversion failed"))?;
    VerifyingKey::from_bytes(&key_array).context("invalid embedded public key")
}

/// Extract the first file entry's content from a zipsign-signed tar archive.
///
/// # Errors
///
/// Returns an error if the tar cannot be read or contains no entries.
#[allow(dead_code)]
fn extract_tar_content(signed_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut unsigned = Cursor::new(Vec::new());
    copy_and_unsign_tar(&mut Cursor::new(signed_bytes), &mut unsigned)
        .context("failed to unsign versions.json tar")?;
    unsigned.set_position(0);

    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(unsigned));
    let mut entry = archive
        .entries()
        .context("failed to read tar entries")?
        .next()
        .ok_or_else(|| anyhow::anyhow!("versions.json tar is empty"))??;

    let mut content = Vec::new();
    entry
        .read_to_end(&mut content)
        .context("failed to read versions.json content")?;
    Ok(content)
}

// ── Container update flow (issue 08) ─────────────────────────────────────────

use crate::multipass::Multipass;

/// GHCR registry prefix for Polis container images.
const GHCR_PREFIX: &str = "ghcr.io/odralabshq";

/// Path to `docker-compose.yml` inside the VM.
const COMPOSE_PATH: &str = "/opt/polis/docker-compose.yml";

/// Path to the `.env` file that pins container image versions inside the VM.
const ENV_PATH: &str = "/opt/polis/.env";

/// Map a container image name to its `.env` variable name.
///
/// `polis-gate-oss` → `POLIS_GATE_VERSION`
/// `polis-host-init-oss` → `POLIS_HOST_INIT_VERSION`
fn image_name_to_env_var(image_name: &str) -> String {
    let middle = image_name
        .strip_prefix("polis-")
        .and_then(|s| s.strip_suffix("-oss"))
        .unwrap_or(image_name);
    format!("POLIS_{}_VERSION", middle.replace('-', "_").to_uppercase())
}

/// A planned container update with current and target versions.
#[allow(dead_code)]
#[derive(Debug)]
pub struct ContainerUpdate {
    /// Service name in `docker-compose.yml` (e.g., `"gate"`).
    pub service_key: String,
    /// Full image name (e.g., `"polis-gate-oss"`).
    pub image_name: String,
    /// Currently deployed version tag (e.g., `"v0.3.0"`).
    pub current_version: String,
    /// Target version from `versions.json`.
    pub target_version: String,
}

/// Rollback information captured before applying updates.
#[allow(dead_code)]
#[derive(Debug)]
pub struct RollbackInfo {
    /// `(service_key, previous_ghcr_image_ref)` pairs.
    pub previous_refs: Vec<(String, String)>,
}

/// Build the full GHCR image reference.
///
/// # Example
///
/// ```text
/// ghcr_ref("polis-gate-oss", "v0.3.1")
/// // → "ghcr.io/odralabshq/polis-gate-oss:v0.3.1"
/// ```
fn ghcr_ref(image_name: &str, version: &str) -> String {
    format!("{GHCR_PREFIX}/{image_name}:{version}")
}

/// Map a `versions.json` container name to its `docker-compose.yml` service key.
///
/// Strips the `polis-` prefix and `-oss` suffix.
/// Falls back to the original name if the pattern does not match.
///
/// # Example
///
/// ```text
/// container_to_service_key("polis-gate-oss") // → "gate"
/// ```
fn container_to_service_key(container_name: &str) -> &str {
    container_name
        .strip_prefix("polis-")
        .and_then(|s| s.strip_suffix("-oss"))
        .unwrap_or(container_name)
}

/// Read the currently deployed version for a container image from the `.env` file.
///
/// Returns `None` if the `.env` file does not exist or the key is absent
/// (caller treats this as "unknown", triggering an update).
///
/// # Errors
///
/// Returns an error if the `multipass exec` call fails unexpectedly.
fn get_deployed_version(image_name: &str, mp: &impl Multipass) -> Result<Option<String>> {
    let output = mp.exec(&["cat", ENV_PATH]).context("failed to read .env")?;

    if !output.status.success() {
        return Ok(None);
    }

    let env_var = image_name_to_env_var(image_name);
    let content = String::from_utf8_lossy(&output.stdout);
    for line in content.lines() {
        if let Some(val) = line.strip_prefix(&format!("{env_var}=")) {
            let v = val.trim().to_string();
            return Ok(if v.is_empty() { None } else { Some(v) });
        }
    }
    Ok(None)
}

/// Pull a specific container image inside the VM via `docker pull`.
///
/// Uses the full image reference so the pull is independent of the `.env` state.
/// Returns `false` if the pull fails (caller decides whether to abort).
///
/// # Errors
///
/// Returns an error only if `multipass exec` itself cannot be spawned.
fn pull_container_image(image_ref: &str, mp: &impl Multipass) -> Result<bool> {
    let output = mp
        .exec(&["docker", "pull", image_ref])
        .context("failed to run multipass exec docker pull")?;

    Ok(output.status.success())
}

/// Capture the current env var values for all services being updated (for rollback).
///
/// Stores the old `POLIS_*_VERSION` value for each update, or an empty string
/// if the key was not yet set (meaning rollback should remove it).
///
/// # Errors
///
/// Returns an error if the `multipass exec` call fails unexpectedly.
fn capture_rollback_info(updates: &[ContainerUpdate], mp: &impl Multipass) -> Result<RollbackInfo> {
    let output = mp
        .exec(&["cat", ENV_PATH])
        .context("failed to read .env for rollback")?;
    let content = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        String::new()
    };

    let previous_refs = updates
        .iter()
        .map(|u| {
            let env_var = image_name_to_env_var(&u.image_name);
            let old_val = content
                .lines()
                .find_map(|l| {
                    l.strip_prefix(&format!("{env_var}="))
                        .map(|v| v.trim().to_string())
                })
                .unwrap_or_default();
            (env_var, old_val)
        })
        .collect();

    Ok(RollbackInfo { previous_refs })
}

/// Set or update a single `KEY=value` entry in the `.env` file inside the VM.
///
/// Atomically replaces any existing line for `key` and appends the new value.
/// If `value` is empty, the key is removed without adding a new line (rollback
/// of a key that did not previously exist).
///
/// # Errors
///
/// Returns an error if the shell command fails.
fn set_env_var(key: &str, value: &str, mp: &impl Multipass) -> Result<()> {
    let cmd = if value.is_empty() {
        format!(
            "grep -v '^{key}=' {ENV_PATH} 2>/dev/null > {ENV_PATH}.tmp && mv {ENV_PATH}.tmp {ENV_PATH} || true"
        )
    } else {
        format!(
            "{{ grep -v '^{key}=' {ENV_PATH} 2>/dev/null; echo '{key}={value}'; }} > {ENV_PATH}.tmp && mv {ENV_PATH}.tmp {ENV_PATH}"
        )
    };
    let output = mp
        .exec(&["bash", "-c", &cmd])
        .context("failed to update .env")?;
    anyhow::ensure!(
        output.status.success(),
        "failed to set {key} in .env: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

/// Restart the given services inside the VM via `docker compose up -d`.
///
/// # Errors
///
/// Returns an error if the restart command fails.
fn restart_services(service_keys: &[&str], mp: &impl Multipass) -> Result<()> {
    let mut args = vec!["docker", "compose", "-f", COMPOSE_PATH, "up", "-d"];
    args.extend_from_slice(service_keys);

    let output = mp
        .exec(&args)
        .context("failed to run multipass exec docker compose up -d")?;

    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

/// Check whether the workspace VM is running.
fn is_vm_running(mp: &impl Multipass) -> bool {
    mp.vm_info().map(|o| o.status.success()).unwrap_or(false)
}

/// Compute the list of container updates by comparing the manifest against deployed versions.
///
/// # Errors
///
/// Returns an error if any version tag in the manifest fails validation.
fn compute_container_updates(
    manifest: &VersionsManifest,
    mp: &impl Multipass,
) -> Result<Vec<ContainerUpdate>> {
    let mut updates = Vec::new();
    for (image_name, target_version) in &manifest.containers {
        validate_version_tag(target_version)
            .with_context(|| format!("invalid version tag in manifest: {target_version}"))?;

        let service_key = container_to_service_key(image_name).to_string();
        let current_version = get_deployed_version(image_name, mp)
            .unwrap_or(None)
            .unwrap_or_else(|| "unknown".to_string());

        if current_version != *target_version {
            updates.push(ContainerUpdate {
                service_key,
                image_name: image_name.clone(),
                current_version,
                target_version: target_version.clone(),
            });
        }
    }
    Ok(updates)
}

/// Run the container update flow: pull → update compose → restart → rollback on failure.
///
/// # Errors
///
/// Returns an error if the VM is not running, any pull fails, or rollback fails.
pub fn update_containers(
    manifest: &VersionsManifest,
    ctx: &OutputContext,
    mp: &impl Multipass,
) -> Result<()> {
    if !is_vm_running(mp) {
        anyhow::bail!("Workspace is not running. Start it with: polis start");
    }

    let updates = compute_container_updates(manifest, mp)?;

    if updates.is_empty() {
        println!(
            "  {} All containers up to date",
            "✓".style(ctx.styles.success)
        );
        return Ok(());
    }

    // Display update table
    println!("  {:<24} {:<12} Available", "Container", "Current");
    println!("  {}", "─".repeat(52));
    for u in &updates {
        println!(
            "  {:<24} {:<12} {}",
            u.image_name, u.current_version, u.target_version
        );
    }
    println!();

    let confirmed = Confirm::new()
        .with_prompt(format!("Update {} container(s)?", updates.len()))
        .default(true)
        .interact()
        .context("reading confirmation")?;

    if !confirmed {
        return Ok(());
    }

    // Capture rollback info before any changes
    let rollback = capture_rollback_info(&updates, mp)?;

    // Pull all images first (F-002 atomicity — no compose changes until all pulls succeed)
    if !pull_all_images(&updates, ctx, mp)? {
        return Ok(());
    }

    // Update compose tags, restart, and rollback on failure
    apply_updates_with_rollback(&updates, &rollback, ctx, mp)
}

/// Pull all container images. Returns `false` if any pull fails (no changes made).
fn pull_all_images(
    updates: &[ContainerUpdate],
    ctx: &OutputContext,
    mp: &impl Multipass,
) -> Result<bool> {
    for u in updates {
        if !ctx.quiet {
            println!("  Pulling {} {}...", u.image_name, u.target_version);
        }
        let image_ref = ghcr_ref(&u.image_name, &u.target_version);
        if !pull_container_image(&image_ref, mp)? {
            println!(
                "  Pull failed for {}:{}. No changes made.",
                u.image_name, u.target_version
            );
            return Ok(false);
        }
    }
    Ok(true)
}

/// Apply compose tag updates, restart services, and rollback on failure (F-003).
fn apply_updates_with_rollback(
    updates: &[ContainerUpdate],
    rollback: &RollbackInfo,
    ctx: &OutputContext,
    mp: &impl Multipass,
) -> Result<()> {
    let service_keys: Vec<&str> = updates.iter().map(|u| u.service_key.as_str()).collect();

    let apply_result = (|| -> Result<()> {
        for u in updates {
            set_env_var(&image_name_to_env_var(&u.image_name), &u.target_version, mp)?;
        }
        restart_services(&service_keys, mp)
    })();

    if let Err(e) = apply_result {
        eprintln!("  Restart failed. Rolling back...");
        let rollback_result = (|| -> Result<()> {
            for (env_var, old_val) in &rollback.previous_refs {
                set_env_var(env_var, old_val, mp)?;
            }
            restart_services(&service_keys, mp)
        })();

        match rollback_result {
            Ok(()) => anyhow::bail!("Update rolled back: {e}"),
            Err(rb_err) => anyhow::bail!(
                "CRITICAL: Rollback failed. Manual intervention required.\n\
                 Restore {ENV_PATH} from backup.\n\
                 Rollback error: {rb_err}"
            ),
        }
    }

    println!();
    println!("  {} Updated", "✓".style(ctx.styles.success));
    Ok(())
}

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
pub async fn run(
    ctx: &OutputContext,
    checker: &impl UpdateChecker,
    mp: &impl Multipass,
) -> Result<()> {
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

    // Container update check (issue 08)
    println!();
    if !ctx.quiet {
        println!("  Checking container updates...");
        println!();
    }
    match load_versions_manifest() {
        Ok(manifest) => update_containers(&manifest, ctx, mp)?,
        Err(e) => {
            // Non-fatal: container update check failure should not block CLI update
            eprintln!("  Warning: could not load versions manifest: {e}");
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

    /// Stub [`Multipass`] for tests that never reach multipass calls.
    struct StubMultipass;
    impl Multipass for StubMultipass {
        fn vm_info(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: vm_info not expected")
        }
        fn launch(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
        ) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: launch not expected")
        }
        fn start(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: start not expected")
        }
        fn transfer(&self, _: &str, _: &str) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: transfer not expected")
        }
        fn exec(&self, _: &[&str]) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: exec not expected")
        }
        fn version(&self) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("stub: version not expected")
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

        let ctx = OutputContext::new(true, true);
        let result = run(&ctx, &AlwaysUpToDate, &StubMultipass).await;
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
        let result = run(&ctx, &BadSignature, &StubMultipass).await;
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
            let result = rt.block_on(run(&ctx, &UpToDate, &StubMultipass));
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
            let result = rt.block_on(run(&ctx, &BadSig, &StubMultipass));
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

    // -----------------------------------------------------------------------
    // validate_version_tag — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_version_tag_valid_release_returns_ok() {
        assert!(validate_version_tag("v0.3.0").is_ok());
    }

    #[test]
    fn test_validate_version_tag_valid_prerelease_rc_returns_ok() {
        assert!(validate_version_tag("v1.0.0-rc.1").is_ok());
    }

    #[test]
    fn test_validate_version_tag_valid_prerelease_beta_returns_ok() {
        assert!(validate_version_tag("v2.0.0-beta.3").is_ok());
    }

    #[test]
    fn test_validate_version_tag_no_v_prefix_returns_error() {
        let err = validate_version_tag("0.3.0").unwrap_err();
        assert!(err.to_string().contains("invalid version tag"));
    }

    #[test]
    fn test_validate_version_tag_empty_returns_error() {
        assert!(validate_version_tag("").is_err());
    }

    #[test]
    fn test_validate_version_tag_latest_returns_error() {
        assert!(validate_version_tag("latest").is_err());
    }

    #[test]
    fn test_validate_version_tag_injection_semicolon_returns_error() {
        // V-004: shell metacharacter must be rejected
        assert!(validate_version_tag("v0.3.1; curl evil.com").is_err());
    }

    #[test]
    fn test_validate_version_tag_partial_semver_v1_returns_error() {
        assert!(validate_version_tag("v1").is_err());
    }

    #[test]
    fn test_validate_version_tag_partial_semver_v1_2_returns_error() {
        assert!(validate_version_tag("v1.2").is_err());
    }

    #[test]
    fn test_validate_version_tag_prerelease_with_special_chars_returns_error() {
        assert!(validate_version_tag("v1.0.0-rc!1").is_err());
    }

    #[test]
    fn test_validate_version_tag_prerelease_with_space_returns_error() {
        assert!(validate_version_tag("v1.0.0-rc 1").is_err());
    }

    // -----------------------------------------------------------------------
    // VersionsManifest serde — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_versions_manifest_deserialize_valid_json_returns_struct() {
        let json = r#"{
            "manifest_version": 1,
            "vm_image": { "version": "v0.3.0", "asset": "polis-v0.3.0-amd64.qcow2" },
            "containers": { "polis-gate-oss": "v0.3.1" }
        }"#;
        let m: VersionsManifest = serde_json::from_str(json).expect("valid JSON");
        assert_eq!(m.manifest_version, 1);
        assert_eq!(m.vm_image.version, "v0.3.0");
        assert_eq!(m.containers["polis-gate-oss"], "v0.3.1");
    }

    #[test]
    fn test_versions_manifest_deserialize_missing_manifest_version_returns_error() {
        let json =
            r#"{ "vm_image": { "version": "v0.3.0", "asset": "x.qcow2" }, "containers": {} }"#;
        assert!(serde_json::from_str::<VersionsManifest>(json).is_err());
    }

    #[test]
    fn test_versions_manifest_serialize_deserialize_roundtrip() {
        let original = VersionsManifest {
            manifest_version: 1,
            vm_image: VmImageVersion {
                version: "v0.3.0".to_string(),
                asset: "polis-v0.3.0-amd64.qcow2".to_string(),
            },
            containers: [("polis-gate-oss".to_string(), "v0.3.1".to_string())]
                .into_iter()
                .collect(),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: VersionsManifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.manifest_version, original.manifest_version);
        assert_eq!(restored.vm_image.version, original.vm_image.version);
        assert_eq!(restored.containers, original.containers);
    }

    // -----------------------------------------------------------------------
    // validate_version_tag — property
    // -----------------------------------------------------------------------

    proptest! {
        /// Any vX.Y.Z tag (no pre-release) is always accepted.
        #[test]
        fn prop_validate_version_tag_valid_semver_always_ok(
            major in 0u32..100,
            minor in 0u32..100,
            patch in 0u32..100,
        ) {
            let tag = format!("v{major}.{minor}.{patch}");
            prop_assert!(validate_version_tag(&tag).is_ok(), "tag {tag} should be valid");
        }

        /// Tags without a `v` prefix are always rejected.
        #[test]
        fn prop_validate_version_tag_no_v_prefix_always_err(
            major in 0u32..100,
            minor in 0u32..100,
            patch in 0u32..100,
        ) {
            let tag = format!("{major}.{minor}.{patch}");
            prop_assert!(validate_version_tag(&tag).is_err(), "tag {tag} should be invalid");
        }

        /// Tags containing shell metacharacters are always rejected (V-004).
        #[test]
        fn prop_validate_version_tag_shell_metachar_always_err(
            meta in proptest::sample::select(vec![";", "|", "&", "$", "`", "\n", " "]),
        ) {
            let tag = format!("v0.3.0-rc{meta}1");
            prop_assert!(validate_version_tag(&tag).is_err(), "tag with {meta:?} should be invalid");
        }

        // -----------------------------------------------------------------------
        // ghcr_ref — property
        // -----------------------------------------------------------------------

        /// `ghcr_ref` output always starts with the GHCR prefix.
        #[test]
        fn prop_ghcr_ref_always_starts_with_prefix(
            image in "[a-z][a-z0-9-]{1,30}",
            version in "v[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
        ) {
            let r = ghcr_ref(&image, &version);
            prop_assert!(r.starts_with(GHCR_PREFIX), "ref {r:?} must start with GHCR_PREFIX");
        }

        /// `ghcr_ref` output always ends with `:{version}`.
        #[test]
        fn prop_ghcr_ref_always_ends_with_version(
            image in "[a-z][a-z0-9-]{1,30}",
            version in "v[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
        ) {
            let r = ghcr_ref(&image, &version);
            prop_assert!(r.ends_with(&format!(":{version}")), "ref {r:?} must end with :{version}");
        }

        // -----------------------------------------------------------------------
        // container_to_service_key — property
        // -----------------------------------------------------------------------

        /// `polis-{x}-oss` always maps to `{x}`.
        #[test]
        fn prop_container_to_service_key_polis_x_oss_maps_to_x(
            middle in "[a-z][a-z0-9]{1,20}",
        ) {
            let name = format!("polis-{middle}-oss");
            let key = container_to_service_key(&name);
            prop_assert_eq!(key, middle.as_str());
        }
    }

    // -----------------------------------------------------------------------
    // ghcr_ref — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_ghcr_ref_formats_correctly() {
        assert_eq!(
            ghcr_ref("polis-gate-oss", "v0.3.1"),
            "ghcr.io/odralabshq/polis-gate-oss:v0.3.1"
        );
    }

    #[test]
    fn test_ghcr_ref_empty_version_still_formats() {
        let r = ghcr_ref("polis-gate-oss", "");
        assert!(
            r.ends_with(':'),
            "empty version should produce trailing colon"
        );
    }

    // -----------------------------------------------------------------------
    // container_to_service_key — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_container_to_service_key_strips_polis_prefix_and_oss_suffix() {
        assert_eq!(container_to_service_key("polis-gate-oss"), "gate");
    }

    #[test]
    fn test_container_to_service_key_all_known_services() {
        let cases = [
            ("polis-gate-oss", "gate"),
            ("polis-sentinel-oss", "sentinel"),
            ("polis-resolver-oss", "resolver"),
            ("polis-scanner-oss", "scanner"),
            ("polis-workspace-oss", "workspace"),
            ("polis-state-oss", "state"),
            ("polis-toolbox-oss", "toolbox"),
        ];
        for (input, expected) in cases {
            assert_eq!(container_to_service_key(input), expected, "input: {input}");
        }
    }

    #[test]
    fn test_container_to_service_key_no_prefix_returns_original() {
        assert_eq!(container_to_service_key("gate"), "gate");
    }

    #[test]
    fn test_container_to_service_key_only_prefix_no_oss_suffix_returns_original() {
        // "polis-gate" has the prefix but not the "-oss" suffix → fallback
        assert_eq!(container_to_service_key("polis-gate"), "polis-gate");
    }

    // -----------------------------------------------------------------------
    // compute_container_updates — unit
    // -----------------------------------------------------------------------

    fn manifest_with_containers(containers: &[(&str, &str)]) -> VersionsManifest {
        VersionsManifest {
            manifest_version: 1,
            vm_image: VmImageVersion {
                version: "v0.3.0".to_string(),
                asset: "polis-v0.3.0-amd64.qcow2".to_string(),
            },
            containers: containers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn test_compute_container_updates_invalid_tag_returns_error() {
        // V-004: invalid version tag must be rejected before any multipass call
        let manifest = manifest_with_containers(&[("polis-gate-oss", "v0.3.1; curl evil.com")]);
        let result = compute_container_updates(&manifest, &StubMultipass);
        assert!(result.is_err(), "invalid tag must return error");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid version tag"),
            "error must mention invalid version tag"
        );
    }

    #[test]
    fn test_compute_container_updates_injection_tag_returns_error() {
        // V-004: shell metacharacter in tag
        let manifest = manifest_with_containers(&[("polis-gate-oss", "v0.3.0|rm -rf /")]);
        assert!(compute_container_updates(&manifest, &StubMultipass).is_err());
    }

    #[test]
    fn test_compute_container_updates_no_v_prefix_returns_error() {
        let manifest = manifest_with_containers(&[("polis-gate-oss", "0.3.0")]);
        assert!(compute_container_updates(&manifest, &StubMultipass).is_err());
    }

    // -----------------------------------------------------------------------
    // update_containers — unit (VM not running path)
    // -----------------------------------------------------------------------

    // NOTE: update_containers() calls is_vm_running() which spawns multipass.
    // Unit tests must not depend on external processes. This path will be
    // covered once VmChecker trait injection is implemented (testability
    // recommendation in update.rs). Tests live in tests/container_update.rs.
}
