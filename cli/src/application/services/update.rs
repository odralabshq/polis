//! Application service — CLI self-update and VM config update use-cases.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::{Context, Result};

use crate::application::ports::{
    AssetExtractor, FileHasher, FileTransfer, InstanceInspector, ProgressReporter, ShellExecutor,
};
use crate::application::services::vm::{
    integrity::{verify_image_digests, write_config_hash},
    lifecycle::{self as vm, VmState},
    provision::transfer_config,
    services::pull_images,
};

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

// ── VM config update service ──────────────────────────────────────────────────

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
pub async fn update_vm_config(
    mp: &(impl InstanceInspector + ShellExecutor + FileTransfer),
    assets: &impl AssetExtractor,
    hasher: &(impl FileHasher + ?Sized),
    reporter: &impl ProgressReporter,
    assets_dir: &std::path::Path,
    version: &str,
) -> Result<UpdateVmConfigOutcome> {
    // Compute SHA256 of the new config tarball
    let new_hash = hasher
        .sha256_file(&assets_dir.join("polis-setup.config.tar"))
        .context("computing config tarball hash")?;

    // Read current hash from VM
    let hash_output = mp
        .exec(&["cat", "/opt/polis/.config-hash"])
        .await
        .context("reading current config hash from VM")?;
    let current_hash = String::from_utf8_lossy(&hash_output.stdout)
        .trim()
        .to_string();

    // If hashes match, config is up to date
    if new_hash == current_hash {
        return Ok(UpdateVmConfigOutcome::UpToDate);
    }

    // Hashes differ — perform full config update cycle

    // Stop services
    mp.exec(&[
        "docker",
        "compose",
        "-f",
        "/opt/polis/docker-compose.yml",
        "down",
    ])
    .await
    .context("stopping services")?;

    // Transfer new config
    transfer_config(mp, assets_dir, version)
        .await
        .context("transferring new config")?;

    // Pull new images
    pull_images(mp, reporter)
        .await
        .context("pulling Docker images")?;

    // Verify image digests
    verify_image_digests(mp, assets)
        .await
        .context("verifying image digests")?;

    // Restart services
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

    // Write new hash AFTER successful restart
    write_config_hash(mp, &new_hash)
        .await
        .context("writing new config hash")?;

    Ok(UpdateVmConfigOutcome::Updated)
}

/// Outcome of the VM config update service.
pub enum UpdateVmConfigOutcome {
    /// Config was already up to date — no changes made.
    UpToDate,
    /// Config was updated successfully.
    Updated,
}

/// Check whether the VM needs a config update (VM must be running).
///
/// Returns `true` if the VM is running and a config update should be performed.
///
/// # Errors
///
/// Returns an error if the VM state cannot be determined.
#[allow(dead_code)] // Not yet called from command handlers
pub async fn should_update_vm_config(mp: &impl InstanceInspector) -> Result<bool> {
    Ok(vm::state(mp).await? == VmState::Running)
}
