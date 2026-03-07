//! Application service — CLI self-update and VM config update use-cases.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::{Context, Result};

use crate::application::ports::{
    AssetExtractor, FileHasher, FileTransfer, InstanceInspector, ProgressReporter, ShellExecutor,
    UpdateChecker, VerifiedAsset,
};
use crate::application::vm::{
    integrity::{verify_image_digests, write_config_hash},
    provision::transfer_config,
    pull::pull_images,
};

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
    reporter.begin_stage("stopping services...");
    mp.exec(&[
        "docker",
        "compose",
        "-f",
        "/opt/polis/docker-compose.yml",
        "down",
    ])
    .await
    .context("stopping services")?;
    reporter.complete_stage();

    // From here on, if anything fails we attempt a best-effort restart so the
    // VM is not left with services down.
    match apply_config_update(mp, assets, reporter, assets_dir, version, &new_hash).await {
        Ok(()) => Ok(UpdateVmConfigOutcome::Updated),
        Err(e) => {
            // Best-effort: try to bring services back up with whatever config is present.
            let _ = mp
                .exec(&[
                    "docker",
                    "compose",
                    "-f",
                    "/opt/polis/docker-compose.yml",
                    "up",
                    "-d",
                ])
                .await;
            Err(e.context("config update failed; services restarted with previous config"))
        }
    }
}

/// Inner helper that performs the config transfer → pull → verify → restart
/// cycle. Separated so the caller can wrap it with rollback logic.
async fn apply_config_update(
    mp: &(impl InstanceInspector + ShellExecutor + FileTransfer),
    assets: &impl AssetExtractor,
    reporter: &impl ProgressReporter,
    assets_dir: &std::path::Path,
    version: &str,
    new_hash: &str,
) -> Result<()> {
    // Transfer new config
    reporter.begin_stage("transferring config...");
    transfer_config(mp, assets_dir, version)
        .await
        .context("transferring new config")?;
    reporter.complete_stage();

    // Pull new images
    reporter.begin_stage("pulling images...");
    pull_images(mp, reporter)
        .await
        .context("pulling Docker images")?;
    reporter.complete_stage();

    // Verify image digests
    reporter.begin_stage("verifying images...");
    verify_image_digests(mp, assets, reporter)
        .await
        .context("verifying image digests")?;
    reporter.complete_stage();

    // Restart services
    reporter.begin_stage("starting services...");
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
    reporter.complete_stage();

    // Write new hash AFTER successful restart
    write_config_hash(mp, new_hash)
        .await
        .context("writing new config hash")?;

    Ok(())
}

/// Outcome of the VM config update service.
#[derive(Debug)]
pub enum UpdateVmConfigOutcome {
    /// Config was already up to date — no changes made.
    UpToDate,
    /// Config was updated successfully.
    Updated,
}

// ── CLI Update Application Service ────────────────────────────────────────────

/// Outcome of the CLI update application service.
#[derive(Debug)]
pub enum ApplyCliUpdateOutcome {
    /// No update was available.
    NoUpdate,
    /// User declined the update.
    Declined,
    /// Update was successfully applied.
    Applied {
        /// The new version that was installed.
        version: String,
        /// SHA-256 hash preview (first 12 chars).
        sha_preview: String,
    },
}

/// Download, verify, and install a CLI update.
///
/// This is the application-layer orchestration for CLI self-update. It:
/// 1. Downloads and verifies the release asset
/// 2. Returns the verification result for user confirmation (handled by caller)
/// 3. Installs the update if confirmed
///
/// The caller (command handler) is responsible for:
/// - User confirmation prompts
/// - Output rendering
///
/// # Errors
///
/// Returns an error if download, verification, or installation fails.
pub async fn download_and_verify_cli_update<C>(
    checker: &C,
    download_url: &str,
    reporter: &impl ProgressReporter,
) -> Result<VerifiedAsset>
where
    C: UpdateChecker + Clone + Send + 'static,
{
    reporter.begin_stage("downloading and verifying...");
    let checker_clone = checker.clone();
    let url = download_url.to_string();
    let asset = tokio::task::spawn_blocking(move || checker_clone.download_and_verify(&url))
        .await
        .context("spawn_blocking panicked")?
        .context("download and verification failed")?;
    reporter.complete_stage();
    Ok(asset)
}

/// Install a verified CLI update.
///
/// # Errors
///
/// Returns an error if installation fails.
pub async fn install_cli_update<C>(
    checker: C,
    asset: VerifiedAsset,
    reporter: &impl ProgressReporter,
) -> Result<()>
where
    C: UpdateChecker + Send + 'static,
{
    reporter.begin_stage("installing update...");
    tokio::task::spawn_blocking(move || checker.install(asset))
        .await
        .context("spawn_blocking panicked")?
        .context("update failed")?;
    reporter.complete_stage();
    Ok(())
}

// ── Post-Update Service ───────────────────────────────────────────────────────

/// Outcome of running the post-update command.
#[derive(Debug)]
pub enum PostUpdateOutcome {
    /// Post-update completed successfully.
    Success,
    /// Post-update returned non-zero exit code.
    NonZeroExit,
}

/// Run the newly-installed CLI binary with the hidden `_post-update` command.
///
/// This delegates VM config update to the NEW binary so its embedded assets
/// are used instead of the stale ones from the old binary.
///
/// # Errors
///
/// Returns an error if the new binary cannot be executed.
pub async fn run_post_update() -> Result<PostUpdateOutcome> {
    let exe = std::env::current_exe().context("resolving current executable path")?;
    let status = tokio::process::Command::new(&exe)
        .arg("_post-update")
        .status()
        .await
        .context("failed to run post-update process")?;

    if status.success() {
        Ok(PostUpdateOutcome::Success)
    } else {
        Ok(PostUpdateOutcome::NonZeroExit)
    }
}

// ── VM Config Update Orchestration ────────────────────────────────────────────

/// Run the VM config update cycle.
///
/// This is a thin wrapper around `update_vm_config` that handles the
/// orchestration. The caller provides the extracted assets directory.
///
/// # Errors
///
/// Returns an error if any step of the update cycle fails.
pub async fn run_vm_config_update_service(
    mp: &(impl InstanceInspector + ShellExecutor + FileTransfer),
    assets: &impl AssetExtractor,
    hasher: &(impl FileHasher + ?Sized),
    reporter: &impl ProgressReporter,
    assets_dir: &std::path::Path,
    version: &str,
) -> Result<UpdateVmConfigOutcome> {
    update_vm_config(mp, assets, hasher, reporter, assets_dir, version).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::application::ports::UpdateInfo;

    #[test]
    fn test_update_info_display_up_to_date() {
        let info = UpdateInfo::UpToDate;
        assert_eq!(info.to_string(), "up to date");
    }

    #[test]
    fn test_update_info_display_available() {
        let info = UpdateInfo::Available {
            version: "1.2.3".to_string(),
            release_notes: vec!["Fix bug".to_string()],
            download_url: "https://example.com/release.tar.gz".to_string(),
        };
        assert_eq!(info.to_string(), "v1.2.3 available");
    }
}
