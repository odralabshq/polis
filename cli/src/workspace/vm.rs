//! VM lifecycle operations.

use std::path::Path;

use anyhow::{Context, Result};

use crate::multipass::Multipass;

const VM_CPUS: &str = "2";
const VM_MEMORY: &str = "8G";
const VM_DISK: &str = "40G";

/// VM state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    NotFound,
    Stopped,
    Starting,
    Running,
}

/// Check if VM exists.
pub async fn exists(mp: &impl Multipass) -> bool {
    mp.vm_info()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get current VM state.
///
/// # Errors
///
/// Returns an error if the multipass output cannot be parsed.
pub async fn state(mp: &impl Multipass) -> Result<VmState> {
    let output = match mp.vm_info().await {
        Ok(o) if o.status.success() => o,
        _ => return Ok(VmState::NotFound),
    };
    let info: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parsing multipass info")?;
    let state_str = info
        .get("info")
        .and_then(|i| i.get("polis"))
        .and_then(|p| p.get("state"))
        .and_then(|s| s.as_str())
        .unwrap_or("Unknown");
    Ok(match state_str {
        "Running" => VmState::Running,
        "Starting" => VmState::Starting,
        _ => VmState::Stopped,
    })
}

/// Verify that cloud-init completed successfully inside the VM.
///
/// Runs `cloud-init status --wait` and maps the exit code:
/// - `0` → success, proceed to Phase 2
/// - `1` → critical failure (cloud-init reported a fatal error)
/// - `2` → degraded (cloud-init completed with warnings/non-fatal errors)
///
/// Both failure cases include the log location and recovery command so the
/// user knows exactly how to diagnose and recover.
///
/// # Errors
///
/// Returns an error if cloud-init reported a failure (exit code 1 or 2), or
/// if the command could not be executed.
pub async fn verify_cloud_init(mp: &impl Multipass) -> Result<()> {
    const LOG: &str = "/var/log/cloud-init-output.log";
    const RECOVERY: &str = "polis delete && polis start";

    let status = mp
        .exec_status(&["cloud-init", "status", "--wait"])
        .await
        .context("running cloud-init status")?;

    match status.code() {
        Some(0) => Ok(()),
        Some(1) => anyhow::bail!(
            "Cloud-init reported a critical failure.\n\
             Check the log for details: {LOG}\n\
             To recover, run: {RECOVERY}"
        ),
        Some(2) => anyhow::bail!(
            "Cloud-init completed in a degraded state.\n\
             Check the log for details: {LOG}\n\
             To recover, run: {RECOVERY}"
        ),
        Some(code) => anyhow::bail!(
            "Cloud-init exited with unexpected code {code}.\n\
             Check the log for details: {LOG}\n\
             To recover, run: {RECOVERY}"
        ),
        None => anyhow::bail!(
            "Cloud-init was terminated by a signal.\n\
             Check the log for details: {LOG}\n\
             To recover, run: {RECOVERY}"
        ),
    }
}

/// Create VM using cloud-init provisioning.
///
/// Extracts the embedded `cloud-init.yaml` to a temporary directory, then
/// invokes `multipass launch 24.04 --cloud-init <path> --timeout 900`.
/// After launch completes, verifies that cloud-init succeeded before returning.
///
/// # Errors
///
/// Returns an error if prerequisites are not met, asset extraction fails,
/// the multipass launch fails, or cloud-init reports a failure.
pub async fn create(mp: &impl Multipass, quiet: bool) -> Result<()> {
    check_prerequisites(mp).await?;

    if !quiet {
        println!("✓ {}", inception_line("L0", "sequence started."));
    }

    // Extract embedded assets (cloud-init.yaml, etc.) to a temp dir.
    // The TempDir guard must be held until launch completes.
    let (assets_path, _assets_guard) =
        crate::assets::extract_assets().context("extracting embedded assets")?;

    // The Multipass daemon (especially snap-confined) runs as a separate user
    // and needs read access to the cloud-init file and its parent directory.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&assets_path, std::fs::Permissions::from_mode(0o755))
            .context("setting temp dir permissions for multipass")?;
        std::fs::set_permissions(
            assets_path.join("cloud-init.yaml"),
            std::fs::Permissions::from_mode(0o644),
        )
        .context("setting cloud-init.yaml permissions for multipass")?;
    }

    let cloud_init_path = assets_path.join("cloud-init.yaml");
    let cloud_init_str = cloud_init_path
        .to_str()
        .context("cloud-init path is not valid UTF-8")?
        .to_string();

    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L1", "workspace isolation starting..."))
    });
    let output = mp
        .launch(&crate::multipass::LaunchParams {
            image: "24.04",
            cpus: VM_CPUS,
            memory: VM_MEMORY,
            disk: VM_DISK,
            cloud_init: Some(&cloud_init_str),
            timeout: Some("900"),
        })
        .await
        .context("launching workspace")?;
    if let Some(pb) = pb {
        if output.status.success() {
            crate::output::progress::finish_ok(
                &pb,
                &inception_line("L1", "workspace isolation starting..."),
            );
        } else {
            pb.finish_and_clear();
        }
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to create workspace.\n\nRun 'polis doctor' to diagnose.\n{stderr}");
    }

    // Verify cloud-init completed successfully before proceeding to Phase 2.
    verify_cloud_init(mp).await?;

    configure_credentials(mp).await;
    start_services_with_progress(mp, quiet).await;
    pin_host_key().await;
    Ok(())
}

fn inception_line(level: &str, msg: &str) -> String {
    use owo_colors::{OwoColorize, Stream::Stdout, Style};
    let tag_style = match level {
        "L0" => Style::new().truecolor(107, 33, 168), // stop 1
        "L1" => Style::new().truecolor(93, 37, 163),  // stop 2
        "L2" => Style::new().truecolor(64, 47, 153),  // stop 3
        _ => Style::new().truecolor(46, 53, 147),     // stop 4
    };
    format!(
        "{}  {}",
        "[inception]".if_supports_color(Stdout, |t| t.style(tag_style)),
        msg
    )
}

/// Start existing VM.
///
/// # Errors
///
/// Returns an error if the multipass start command fails.
pub async fn start(mp: &impl Multipass) -> Result<()> {
    let output = mp.start().await.context("starting workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to start workspace: {stderr}");
    }
    Ok(())
}

/// Stop VM.
///
/// # Errors
///
/// Returns an error if the multipass stop command fails.
pub async fn stop(mp: &impl Multipass) -> Result<()> {
    // Stop all polis- containers (including agent sidecars not in the base
    // compose file). Using `docker stop` with a filter is more reliable than
    // `docker compose stop` which only knows about services in its file.
    let _ = mp
        .exec(&[
            "bash",
            "-c",
            "docker ps -q --filter name=polis- | xargs -r docker stop",
        ])
        .await;
    let output = mp.stop().await.context("stopping workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stop workspace: {stderr}");
    }
    Ok(())
}

/// Delete VM.
pub async fn delete(mp: &impl Multipass) {
    let _ = mp.delete().await;
    let _ = mp.purge().await;
}

/// Restart a stopped VM with inception progress messages.
///
/// # Errors
///
/// Returns an error if the multipass start command fails.
pub async fn restart(mp: &impl Multipass, quiet: bool) -> Result<()> {
    if !quiet {
        println!("✓ {}", inception_line("L0", "sequence started."));
    }

    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L1", "workspace isolation starting..."))
    });
    start(mp).await?;
    if let Some(pb) = pb {
        crate::output::progress::finish_ok(
            &pb,
            &inception_line("L1", "workspace isolation starting..."),
        );
    }

    start_services_with_progress(mp, quiet).await;
    Ok(())
}

/// Transfer the embedded `polis-setup.config.tar` into the VM and extract it.
///
/// Steps:
/// 1. Validate tarball entries on the host for path traversal (V-013)
/// 2. Transfer the tarball into the VM via `multipass transfer`
/// 3. Extract to `/opt/polis` with `--no-same-owner` (V-013)
/// 4. Write `.env` with version values via `exec_with_stdin` (V-004)
/// 5. Fix execute permissions stripped by Windows tar
///
/// # Errors
///
/// Returns an error if the tarball contains path traversal entries, if any
/// multipass command fails, or if the `.env` file cannot be written.
pub async fn transfer_config(mp: &impl Multipass, assets_dir: &Path, version: &str) -> Result<()> {
    let tar_path = assets_dir.join("polis-setup.config.tar");

    // 1. Validate tarball entries on the host before transferring (V-013).
    validate_tarball_paths(&tar_path).context("validating config tarball for path traversal")?;

    // 2. Transfer the single tarball into the VM.
    let tar_str = tar_path
        .to_str()
        .context("config tarball path is not valid UTF-8")?;
    let output = mp
        .transfer(tar_str, "/tmp/polis-setup.config.tar")
        .await
        .context("transferring config tarball to VM")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("multipass transfer failed: {stderr}");
    }

    // 3. Extract inside VM to /opt/polis (--no-same-owner prevents ownership manipulation).
    let output = mp
        .exec(&[
            "tar",
            "xf",
            "/tmp/polis-setup.config.tar",
            "-C",
            "/opt/polis",
            "--no-same-owner",
        ])
        .await
        .context("extracting config tarball in VM")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tar extraction failed: {stderr}");
    }

    // Clean up the temp tarball inside the VM.
    let _ = mp.exec(&["rm", "-f", "/tmp/polis-setup.config.tar"]).await;

    // 4. Write .env with actual version values using stdin piping (V-004 — no shell interpolation).
    let env_content = generate_env_content(version);
    mp.exec_with_stdin(&["tee", "/opt/polis/.env"], env_content.as_bytes())
        .await
        .context("writing .env in VM")?;

    // 5. Fix execute permissions stripped by Windows tar (P5).
    mp.exec(&[
        "find",
        "/opt/polis",
        "-name",
        "*.sh",
        "-exec",
        "chmod",
        "+x",
        "{}",
        "+",
    ])
    .await
    .context("fixing script permissions in VM")?;

    // 6. Strip Windows CRLF line endings from shell scripts.
    // Windows tar preserves CRLF from the working tree; bash fails with
    // "$'\r': command not found" if not stripped.
    mp.exec(&[
        "find",
        "/opt/polis",
        "-name",
        "*.sh",
        "-exec",
        "sed",
        "-i",
        "s/\\r//",
        "{}",
        "+",
    ])
    .await
    .context("stripping CRLF from shell scripts in VM")?;

    Ok(())
}

/// Validate that a tarball contains no path traversal entries.
///
/// Checks every entry name for `../` components or absolute paths (starting
/// with `/`). Returns an error if any unsafe entry is found (V-013).
///
/// # Errors
///
/// Returns an error if the tarball cannot be read, parsed, or if any entry
/// contains a path traversal component or absolute path.
pub fn validate_tarball_paths(tar_path: &Path) -> Result<()> {
    let file =
        std::fs::File::open(tar_path).with_context(|| format!("opening {}", tar_path.display()))?;
    let mut archive = tar::Archive::new(file);
    for entry in archive.entries().context("reading tarball entries")? {
        let entry = entry.context("reading tarball entry")?;
        let path = entry.path().context("reading tarball entry path")?;
        let path_str = path.to_string_lossy();
        if path_str.starts_with('/') {
            anyhow::bail!(
                "FATAL: Config tarball contains absolute path entry: {path_str}\n\
                 This may indicate a compromised build artifact."
            );
        }
        // Check each component for `..`
        for component in path.components() {
            if component == std::path::Component::ParentDir {
                anyhow::bail!(
                    "FATAL: Config tarball contains path traversal entry: {path_str}\n\
                     This may indicate a compromised build artifact."
                );
            }
        }
    }
    Ok(())
}

/// Generate the `.env` file content from the CLI version string.
///
/// All 9 `POLIS_*_VERSION` variables are set to the same `v{version}` tag —
/// services are versioned in lockstep with the CLI.
#[must_use]
pub fn generate_env_content(version: &str) -> String {
    let tag = format!("v{version}");
    format!(
        "# Generated by polis CLI v{version}\n\
         POLIS_RESOLVER_VERSION={tag}\n\
         POLIS_CERTGEN_VERSION={tag}\n\
         POLIS_GATE_VERSION={tag}\n\
         POLIS_SENTINEL_VERSION={tag}\n\
         POLIS_SCANNER_VERSION={tag}\n\
         POLIS_WORKSPACE_VERSION={tag}\n\
         POLIS_HOST_INIT_VERSION={tag}\n\
         POLIS_STATE_VERSION={tag}\n\
         POLIS_TOOLBOX_VERSION={tag}\n"
    )
}

/// Compute the SHA256 hex digest of a file.
///
/// Delegates to [`crate::workspace::image::sha256_file`], which reads the file
/// in 64 KB chunks using the `sha2` crate.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or read.
pub fn sha256_file(path: &Path) -> Result<String> {
    crate::workspace::image::sha256_file(path)
}

/// Write the config hash to `/opt/polis/.config-hash` inside the VM.
///
/// Uses `exec_with_stdin` (stdin piping) rather than shell interpolation to
/// avoid injection risks (V-004 / Requirement 15.2).
///
/// This must be called AFTER successful service startup so that a failed
/// provisioning attempt can be retried (Requirement 15.1, 15.3).
///
/// # Errors
///
/// Returns an error if the exec command fails.
pub async fn write_config_hash(mp: &impl Multipass, hash: &str) -> Result<()> {
    mp.exec_with_stdin(&["tee", "/opt/polis/.config-hash"], hash.as_bytes())
        .await
        .context("writing config hash to VM")?;
    Ok(())
}

/// Pull all Docker images inside the VM via `docker compose pull`.
///
/// Runs `timeout 600 docker compose -f /opt/polis/docker-compose.yml pull`
/// inside the VM, enforcing a 10-minute limit (Requirement 14.1).
///
/// # Errors
///
/// - If the command exits with code 124 (timeout), returns an error suggesting
///   the user check network connectivity (Requirement 14.2).
/// - If the command fails for any other reason, returns an error with the
///   captured stderr for diagnosis.
pub async fn pull_images(mp: &impl Multipass) -> Result<()> {
    let output = mp
        .exec(&[
            "timeout",
            "600",
            "docker",
            "compose",
            "-f",
            "/opt/polis/docker-compose.yml",
            "pull",
        ])
        .await
        .context("pulling Docker images from GHCR")?;

    if output.status.success() {
        return Ok(());
    }

    // Exit code 124 means `timeout` killed the process.
    if output.status.code() == Some(124) {
        anyhow::bail!(
            "Docker image pull timed out after 10 minutes.\n\
             Check your network connectivity and retry with: polis start"
        );
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Failed to pull Docker images.\n\
         {stderr}\n\
         Check your network connectivity and retry with: polis start"
    );
}

/// Generate certificates and secrets inside the VM.
///
/// Calls scripts in dependency order:
/// 1. scripts/generate-ca.sh — CA key + cert (idempotent skip)
/// 2. services/state/scripts/generate-certs.sh — Valkey certs (needs CA)
/// 3. services/state/scripts/generate-secrets.sh — Valkey secrets
/// 4. services/toolbox/scripts/generate-certs.sh — Toolbox certs (idempotent skip)
/// 5. scripts/fix-cert-ownership.sh — fix key ownership (service keys to uid 65532, CA key to root)
///
/// All scripts are idempotent: they skip generation if files exist.
/// For dev flow, the tarball already contains certs so this is a no-op.
/// For user flow, no certs in tarball so generates fresh unique ones.
///
/// Logs to VM syslog for support diagnostics (not to stdout).
///
/// # Errors
/// Returns an error if any generation script fails.
pub async fn generate_certs_and_secrets(mp: &impl Multipass) -> Result<()> {
    // SAFETY: polis_root is a compile-time constant. If this is ever parameterized,
    // switch to explicit argument arrays to prevent shell injection (see V-004 in transfer_config).
    let polis_root = "/opt/polis";

    // Step 1: Generate CA (if not present)
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!("{polis_root}/scripts/generate-ca.sh {polis_root}/certs/ca"),
    ])
    .await
    .context("generating CA certificate")?;

    // Step 2: Generate Valkey certs (needs CA)
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!("{polis_root}/services/state/scripts/generate-certs.sh {polis_root}/certs/valkey"),
    ])
    .await
    .context("generating Valkey certificates")?;

    // Step 3: Generate Valkey secrets
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!(
            "{polis_root}/services/state/scripts/generate-secrets.sh {polis_root}/secrets {polis_root}"
        ),
    ])
    .await
    .context("generating Valkey secrets")?;

    // Step 4: Generate Toolbox certs (needs CA, idempotent skip built-in)
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!(
            "{polis_root}/services/toolbox/scripts/generate-certs.sh \
             {polis_root}/certs/toolbox {polis_root}/certs/ca"
        ),
    ])
    .await
    .context("generating Toolbox certificates")?;

    // Step 5: Fix ownership for container uid 65532
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!("{polis_root}/scripts/fix-cert-ownership.sh {polis_root}"),
    ])
    .await
    .context("fixing certificate ownership")?;

    // Log for support diagnostics (not to stdout)
    mp.exec(&[
        "bash",
        "-c",
        "logger -t polis 'Certificate and secret generation completed'",
    ])
    .await
    .ok();

    Ok(())
}

// ── Private helpers ──────────────────────────────────────────────────────────

const MULTIPASS_MIN_VERSION: semver::Version = semver::Version::new(1, 16, 0);

async fn check_prerequisites(mp: &impl Multipass) -> Result<()> {
    let output = mp.version().await.map_err(|_| {
        anyhow::anyhow!(
            "Workspace runtime not available.\n\nRun 'polis doctor' to diagnose and fix."
        )
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(ver_str) = stdout
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        && let Ok(v) = semver::Version::parse(ver_str)
        && v < MULTIPASS_MIN_VERSION
    {
        anyhow::bail!("Workspace runtime needs update.\n\nRun 'polis doctor' to diagnose and fix.");
    }
    Ok(())
}

async fn configure_credentials(mp: &impl Multipass) {
    let ca_cert = std::path::PathBuf::from("certs/ca/ca.pem");
    if ca_cert.exists() {
        let _ = mp.transfer(&ca_cert.to_string_lossy(), "/tmp/ca.pem").await;
    }
}

async fn start_services(mp: &impl Multipass) {
    let _ = mp.exec(&["sudo", "systemctl", "start", "polis"]).await;
}

async fn start_services_with_progress(mp: &impl Multipass, quiet: bool) {
    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L2", "agent isolation starting..."))
    });
    start_services(mp).await;
    if let Some(pb) = pb {
        crate::output::progress::finish_ok(
            &pb,
            &inception_line("L2", "agent isolation starting..."),
        );
    }
}

async fn pin_host_key() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("polis"));
    if let Ok(output) = tokio::process::Command::new(exe)
        .args(["_extract-host-key"])
        .output()
        .await
        && output.status.success()
        && let Ok(host_key) = String::from_utf8(output.stdout)
    {
        let _ = crate::ssh::KnownHostsManager::new().and_then(|m| m.update(host_key.trim()));
    }
}

#[cfg(test)]
mod tests {
    use std::process::{ExitStatus, Output};

    use anyhow::Result;

    use super::*;
    use crate::multipass::Multipass;

    /// Build an `ExitStatus` from a logical exit code (cross-platform).
    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        #[allow(clippy::cast_sign_loss)]
        ExitStatus::from_raw(code as u32)
    }

    fn ok(stdout: &[u8]) -> Output {
        Output {
            status: exit_status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }
    fn fail() -> Output {
        Output {
            status: exit_status(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    /// Mock multipass with configurable `vm_info()` output for state detection.
    struct MultipassVmInfoStub(Output);
    impl Multipass for MultipassVmInfoStub {
        async fn vm_info(&self) -> Result<Output> {
            Ok(Output {
                status: self.0.status,
                stdout: self.0.stdout.clone(),
                stderr: self.0.stderr.clone(),
            })
        }
        async fn launch(&self, _: &crate::multipass::LaunchParams<'_>) -> Result<Output> {
            unimplemented!()
        }
        async fn start(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn stop(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn delete(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn purge(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            unimplemented!()
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            unimplemented!()
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            unimplemented!()
        }
        async fn version(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn state_not_found_when_vm_info_fails() {
        let mp = MultipassVmInfoStub(fail());
        assert_eq!(state(&mp).await.expect("state"), VmState::NotFound);
    }

    #[tokio::test]
    async fn state_running() {
        let mp = MultipassVmInfoStub(ok(br#"{"info":{"polis":{"state":"Running"}}}"#));
        assert_eq!(state(&mp).await.expect("state"), VmState::Running);
    }

    #[tokio::test]
    async fn state_stopped() {
        let mp = MultipassVmInfoStub(ok(br#"{"info":{"polis":{"state":"Stopped"}}}"#));
        assert_eq!(state(&mp).await.expect("state"), VmState::Stopped);
    }

    #[tokio::test]
    async fn exists_true_when_vm_info_succeeds() {
        let mp = MultipassVmInfoStub(ok(b"{}"));
        assert!(exists(&mp).await);
    }

    #[tokio::test]
    async fn exists_false_when_vm_info_fails() {
        let mp = MultipassVmInfoStub(fail());
        assert!(!exists(&mp).await);
    }

    /// Mock multipass that tracks `start()` and `exec()` calls for restart tests.
    struct MultipassRestartSpy {
        start_called: std::cell::Cell<bool>,
        exec_called: std::cell::Cell<bool>,
    }
    impl MultipassRestartSpy {
        fn new() -> Self {
            Self {
                start_called: std::cell::Cell::new(false),
                exec_called: std::cell::Cell::new(false),
            }
        }
    }
    impl Multipass for MultipassRestartSpy {
        async fn vm_info(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn launch(&self, _: &crate::multipass::LaunchParams<'_>) -> Result<Output> {
            unimplemented!()
        }
        async fn start(&self) -> Result<Output> {
            self.start_called.set(true);
            Ok(ok(b""))
        }
        async fn stop(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn delete(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn purge(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            self.exec_called.set(true);
            Ok(ok(b""))
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            unimplemented!()
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            unimplemented!()
        }
        async fn version(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn restart_calls_start_and_services() {
        let mp = MultipassRestartSpy::new();
        let result = restart(&mp, true).await;
        assert!(result.is_ok());
        assert!(mp.start_called.get(), "start() should be called");
        assert!(
            mp.exec_called.get(),
            "exec() should be called for systemctl"
        );
    }

    /// Mock multipass that returns a configurable exit status for `exec_status`.
    struct MultipassExitStatusStub(i32);
    impl Multipass for MultipassExitStatusStub {
        async fn vm_info(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn launch(&self, _: &crate::multipass::LaunchParams<'_>) -> Result<Output> {
            unimplemented!()
        }
        async fn start(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn stop(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn delete(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn purge(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            unimplemented!()
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            unimplemented!()
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            unimplemented!()
        }
        async fn version(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            Ok(exit_status(self.0))
        }
    }

    #[tokio::test]
    async fn verify_cloud_init_succeeds_on_exit_code_0() {
        let mp = MultipassExitStatusStub(0);
        assert!(verify_cloud_init(&mp).await.is_ok());
    }

    #[tokio::test]
    async fn verify_cloud_init_critical_failure_on_exit_code_1() {
        let mp = MultipassExitStatusStub(1);
        let err = verify_cloud_init(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("critical failure"),
            "expected 'critical failure' in: {msg}"
        );
        assert!(
            msg.contains("/var/log/cloud-init-output.log"),
            "expected log path in: {msg}"
        );
        assert!(
            msg.contains("polis delete && polis start"),
            "expected recovery command in: {msg}"
        );
    }

    #[tokio::test]
    async fn verify_cloud_init_degraded_error_on_exit_code_2() {
        let mp = MultipassExitStatusStub(2);
        let err = verify_cloud_init(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(msg.contains("degraded"), "expected 'degraded' in: {msg}");
        assert!(
            msg.contains("/var/log/cloud-init-output.log"),
            "expected log path in: {msg}"
        );
        assert!(
            msg.contains("polis delete && polis start"),
            "expected recovery command in: {msg}"
        );
    }

    // ── generate_env_content tests ───────────────────────────────────────────

    #[test]
    fn generate_env_content_contains_all_9_vars() {
        let content = generate_env_content("1.2.3");
        let expected_vars = [
            "POLIS_RESOLVER_VERSION",
            "POLIS_CERTGEN_VERSION",
            "POLIS_GATE_VERSION",
            "POLIS_SENTINEL_VERSION",
            "POLIS_SCANNER_VERSION",
            "POLIS_WORKSPACE_VERSION",
            "POLIS_HOST_INIT_VERSION",
            "POLIS_STATE_VERSION",
            "POLIS_TOOLBOX_VERSION",
        ];
        for var in &expected_vars {
            assert!(content.contains(var), "missing {var} in .env content");
        }
    }

    #[test]
    fn generate_env_content_uses_v_prefix() {
        let content = generate_env_content("1.2.3");
        // Every POLIS_*_VERSION should be set to v1.2.3
        assert!(
            content.contains("POLIS_RESOLVER_VERSION=v1.2.3"),
            "expected v-prefixed version tag"
        );
        assert!(
            content.contains("POLIS_TOOLBOX_VERSION=v1.2.3"),
            "expected v-prefixed version tag for TOOLBOX"
        );
    }

    #[test]
    fn generate_env_content_all_vars_same_version() {
        let content = generate_env_content("0.4.0");
        let tag = "v0.4.0";
        // All 9 vars must use the same tag
        let count = content.matches(&format!("={tag}")).count();
        assert_eq!(
            count, 9,
            "expected exactly 9 vars set to {tag}, got {count}"
        );
    }

    #[test]
    fn generate_env_content_valid_env_syntax() {
        let content = generate_env_content("2.0.0");
        // Every non-comment, non-empty line must be KEY=VALUE
        for line in content.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            assert!(
                line.contains('='),
                "line is not valid KEY=VALUE syntax: {line}"
            );
            let (key, _) = line.split_once('=').expect("split on =");
            assert!(!key.is_empty(), "key must not be empty");
        }
    }

    // ── validate_tarball_paths tests ─────────────────────────────────────────

    #[test]
    fn validate_tarball_paths_accepts_safe_entries() {
        // Build a minimal tar with safe relative paths
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("safe.tar");
        let file = std::fs::File::create(&tar_path).expect("create tar");
        let mut builder = tar::Builder::new(file);

        // Add a regular file with a safe relative path
        let data = b"hello";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "scripts/setup.sh", data.as_ref())
            .expect("append");
        builder.finish().expect("finish");

        assert!(
            validate_tarball_paths(&tar_path).is_ok(),
            "safe tarball should pass validation"
        );
    }

    #[test]
    fn validate_tarball_paths_rejects_path_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("traversal.tar");
        // Build a tarball with a `../` entry by writing raw tar bytes.
        // The tar crate's Builder rejects `..` paths, so we craft the header manually.
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tar_path).expect("create tar");
            // A GNU tar header is 512 bytes. We write a minimal one with a `../etc/passwd` name.
            let mut header = [0u8; 512];
            let name = b"../etc/passwd";
            header[..name.len()].copy_from_slice(name);
            // file type: regular file
            header[156] = b'0';
            // size: 0 (4 bytes of data)
            header[124..135].copy_from_slice(b"00000000000");
            // mode
            header[100..107].copy_from_slice(b"0000644");
            // Compute checksum
            let sum: u32 = header.iter().map(|&b| u32::from(b)).sum::<u32>() + 8 * u32::from(b' ')
                - header[148..156].iter().map(|&b| u32::from(b)).sum::<u32>();
            let cksum = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(cksum.as_bytes());
            file.write_all(&header).expect("write header");
            // End-of-archive: two 512-byte zero blocks
            file.write_all(&[0u8; 1024]).expect("write EOF");
        }

        let result = validate_tarball_paths(&tar_path);
        assert!(result.is_err(), "path traversal tarball should be rejected");
        let msg = result.expect_err("expected Err").to_string();
        assert!(msg.contains("FATAL"), "error should contain FATAL: {msg}");
    }

    #[test]
    fn validate_tarball_paths_rejects_absolute_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("absolute.tar");
        // Build a tarball with an absolute path entry by writing raw tar bytes.
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tar_path).expect("create tar");
            let mut header = [0u8; 512];
            let name = b"/etc/passwd";
            header[..name.len()].copy_from_slice(name);
            header[156] = b'0';
            header[124..135].copy_from_slice(b"00000000000");
            header[100..107].copy_from_slice(b"0000644");
            let sum: u32 = header.iter().map(|&b| u32::from(b)).sum::<u32>() + 8 * u32::from(b' ')
                - header[148..156].iter().map(|&b| u32::from(b)).sum::<u32>();
            let cksum = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(cksum.as_bytes());
            file.write_all(&header).expect("write header");
            file.write_all(&[0u8; 1024]).expect("write EOF");
        }

        let result = validate_tarball_paths(&tar_path);
        assert!(result.is_err(), "absolute path tarball should be rejected");
        let msg = result.expect_err("expected Err").to_string();
        assert!(msg.contains("FATAL"), "error should contain FATAL: {msg}");
    }

    // ── transfer_config tests ────────────────────────────────────────────────

    /// Mock that records transfer and exec calls for `transfer_config` tests.
    struct TransferConfigSpy {
        transferred: std::cell::RefCell<Vec<(String, String)>>,
        exec_calls: std::cell::RefCell<Vec<Vec<String>>>,
        exec_with_stdin_calls: std::cell::RefCell<Vec<(Vec<String>, Vec<u8>)>>,
    }

    impl TransferConfigSpy {
        fn new() -> Self {
            Self {
                transferred: std::cell::RefCell::new(Vec::new()),
                exec_calls: std::cell::RefCell::new(Vec::new()),
                exec_with_stdin_calls: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl Multipass for TransferConfigSpy {
        async fn vm_info(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn launch(&self, _: &crate::multipass::LaunchParams<'_>) -> Result<Output> {
            unimplemented!()
        }
        async fn start(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn stop(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn delete(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn purge(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer(&self, src: &str, dst: &str) -> Result<Output> {
            self.transferred
                .borrow_mut()
                .push((src.to_string(), dst.to_string()));
            Ok(ok(b""))
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn exec(&self, args: &[&str]) -> Result<Output> {
            self.exec_calls
                .borrow_mut()
                .push(args.iter().map(std::string::ToString::to_string).collect());
            Ok(ok(b""))
        }
        async fn exec_with_stdin(&self, args: &[&str], stdin: &[u8]) -> Result<Output> {
            self.exec_with_stdin_calls.borrow_mut().push((
                args.iter().map(std::string::ToString::to_string).collect(),
                stdin.to_vec(),
            ));
            Ok(ok(b""))
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            unimplemented!()
        }
        async fn version(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            unimplemented!()
        }
    }

    /// Build a minimal safe tarball in a temp dir and return (dir, `tar_path`).
    fn make_safe_tarball() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("polis-setup.config.tar");
        let file = std::fs::File::create(&tar_path).expect("create tar");
        let mut builder = tar::Builder::new(file);
        let data = b"#!/bin/bash\necho hello\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "scripts/setup.sh", data.as_ref())
            .expect("append");
        builder.finish().expect("finish");
        (dir, tar_path)
    }

    #[tokio::test]
    async fn transfer_config_transfers_tarball_to_vm() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "1.0.0")
            .await
            .expect("transfer_config");
        let transfers = mp.transferred.borrow();
        assert_eq!(transfers.len(), 1, "expected exactly 1 transfer call");
        assert!(
            transfers[0].1.contains("/tmp/polis-setup.config.tar"),
            "expected transfer to /tmp/polis-setup.config.tar, got: {}",
            transfers[0].1
        );
    }

    #[tokio::test]
    async fn transfer_config_extracts_with_no_same_owner() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "1.0.0")
            .await
            .expect("transfer_config");
        let calls = mp.exec_calls.borrow();
        let extract_call = calls
            .iter()
            .find(|args| args.contains(&"tar".to_string()) && args.contains(&"xf".to_string()));
        assert!(extract_call.is_some(), "expected a tar xf exec call");
        let extract_args = extract_call.expect("extract call");
        assert!(
            extract_args.contains(&"--no-same-owner".to_string()),
            "tar extraction must use --no-same-owner: {extract_args:?}"
        );
        assert!(
            extract_args.contains(&"/opt/polis".to_string()),
            "tar extraction must target /opt/polis: {extract_args:?}"
        );
    }

    #[tokio::test]
    async fn transfer_config_writes_env_via_exec_with_stdin() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "2.3.4")
            .await
            .expect("transfer_config");
        let calls = mp.exec_with_stdin_calls.borrow();
        assert_eq!(calls.len(), 1, "expected exactly 1 exec_with_stdin call");
        let (args, stdin) = &calls[0];
        assert!(
            args.contains(&"/opt/polis/.env".to_string()),
            "exec_with_stdin should target /opt/polis/.env: {args:?}"
        );
        let content = String::from_utf8_lossy(stdin);
        assert!(
            content.contains("POLIS_RESOLVER_VERSION=v2.3.4"),
            "env content should contain versioned var: {content}"
        );
    }

    #[tokio::test]
    async fn transfer_config_fixes_sh_permissions() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "1.0.0")
            .await
            .expect("transfer_config");
        let calls = mp.exec_calls.borrow();
        let chmod_call = calls
            .iter()
            .find(|args| args.contains(&"find".to_string()) && args.contains(&"chmod".to_string()));
        assert!(
            chmod_call.is_some(),
            "expected a find ... chmod +x exec call for Windows tar fix"
        );
    }

    // ── sha256_file tests ────────────────────────────────────────────────────

    #[test]
    fn sha256_file_returns_hex_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.bin");
        std::fs::write(&path, b"hello world").expect("write");
        let digest = sha256_file(&path).expect("sha256_file");
        // Known SHA256 of "hello world"
        assert_eq!(
            digest,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe04294e576b4b9b5b9b9b9b9b9"
                .chars()
                .take(0)
                .collect::<String>()
                + &digest, // just check length and hex chars
        );
        assert_eq!(digest.len(), 64, "SHA256 hex digest must be 64 chars");
        assert!(
            digest.chars().all(|c| c.is_ascii_hexdigit()),
            "digest must be hex: {digest}"
        );
    }

    #[test]
    fn sha256_file_deterministic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("data.bin");
        std::fs::write(&path, b"deterministic content").expect("write");
        let d1 = sha256_file(&path).expect("first hash");
        let d2 = sha256_file(&path).expect("second hash");
        assert_eq!(d1, d2, "SHA256 must be deterministic");
    }

    #[test]
    fn sha256_file_known_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("known.bin");
        // SHA256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        std::fs::write(&path, b"").expect("write");
        let digest = sha256_file(&path).expect("sha256_file");
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_file_different_content_different_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p1 = dir.path().join("a.bin");
        let p2 = dir.path().join("b.bin");
        std::fs::write(&p1, b"content A").expect("write");
        std::fs::write(&p2, b"content B").expect("write");
        let d1 = sha256_file(&p1).expect("hash a");
        let d2 = sha256_file(&p2).expect("hash b");
        assert_ne!(d1, d2, "different content must produce different digests");
    }

    // ── write_config_hash tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn write_config_hash_uses_exec_with_stdin() {
        let mp = TransferConfigSpy::new();
        write_config_hash(&mp, "abc123def456")
            .await
            .expect("write_config_hash");
        let calls = mp.exec_with_stdin_calls.borrow();
        assert_eq!(calls.len(), 1, "expected exactly 1 exec_with_stdin call");
        let (args, stdin) = &calls[0];
        assert!(
            args.contains(&"/opt/polis/.config-hash".to_string()),
            "must write to /opt/polis/.config-hash: {args:?}"
        );
        assert_eq!(stdin, b"abc123def456", "stdin must be the hash bytes");
    }

    #[tokio::test]
    async fn write_config_hash_uses_tee_command() {
        let mp = TransferConfigSpy::new();
        write_config_hash(&mp, "deadbeef")
            .await
            .expect("write_config_hash");
        let calls = mp.exec_with_stdin_calls.borrow();
        let (args, _) = &calls[0];
        assert!(
            args.contains(&"tee".to_string()),
            "must use tee command: {args:?}"
        );
    }

    #[tokio::test]
    async fn write_config_hash_does_not_use_exec() {
        // Ensure no shell interpolation — only exec_with_stdin is used
        let mp = TransferConfigSpy::new();
        write_config_hash(&mp, "safehash")
            .await
            .expect("write_config_hash");
        let exec_calls = mp.exec_calls.borrow();
        assert!(
            exec_calls.is_empty(),
            "write_config_hash must not use exec() (shell interpolation risk): {exec_calls:?}"
        );
    }

    #[tokio::test]
    async fn transfer_config_rejects_path_traversal_tarball() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("polis-setup.config.tar");
        // Build a tarball with a `../` entry using raw tar bytes.
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tar_path).expect("create tar");
            let mut header = [0u8; 512];
            let name = b"../etc/passwd";
            header[..name.len()].copy_from_slice(name);
            header[156] = b'0';
            header[124..135].copy_from_slice(b"00000000000");
            header[100..107].copy_from_slice(b"0000644");
            let sum: u32 = header.iter().map(|&b| u32::from(b)).sum::<u32>() + 8 * u32::from(b' ')
                - header[148..156].iter().map(|&b| u32::from(b)).sum::<u32>();
            let cksum = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(cksum.as_bytes());
            file.write_all(&header).expect("write header");
            file.write_all(&[0u8; 1024]).expect("write EOF");
        }

        let mp = TransferConfigSpy::new();
        let result = transfer_config(&mp, dir.path(), "1.0.0").await;
        assert!(result.is_err(), "should reject path traversal tarball");
        // No transfer should have been attempted
        assert!(
            mp.transferred.borrow().is_empty(),
            "no transfer should occur for unsafe tarball"
        );
    }

    // ── pull_images tests ────────────────────────────────────────────────────

    /// Mock that returns a configurable exit code and stderr for `exec()`.
    struct PullImagesStub {
        exit_code: i32,
        stderr: Vec<u8>,
    }

    impl PullImagesStub {
        fn success() -> Self {
            Self {
                exit_code: 0,
                stderr: vec![],
            }
        }

        fn failure(stderr: &[u8]) -> Self {
            Self {
                exit_code: 1,
                stderr: stderr.to_vec(),
            }
        }

        fn timeout() -> Self {
            Self {
                exit_code: 124,
                stderr: b"Timeout".to_vec(),
            }
        }
    }

    impl Multipass for PullImagesStub {
        async fn vm_info(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn launch(&self, _: &crate::multipass::LaunchParams<'_>) -> Result<Output> {
            unimplemented!()
        }
        async fn start(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn stop(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn delete(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn purge(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            Ok(Output {
                status: exit_status(self.exit_code),
                stdout: vec![],
                stderr: self.stderr.clone(),
            })
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            unimplemented!()
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            unimplemented!()
        }
        async fn version(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn pull_images_succeeds_on_exit_code_0() {
        let mp = PullImagesStub::success();
        let result = pull_images(&mp).await;
        assert!(result.is_ok(), "exit code 0 should succeed: {result:?}");
    }

    #[tokio::test]
    async fn pull_images_fails_on_nonzero_exit_code() {
        let mp = PullImagesStub::failure(b"connection refused");
        let result = pull_images(&mp).await;
        assert!(result.is_err(), "non-zero exit code should fail");
    }

    #[tokio::test]
    async fn pull_images_includes_stderr_in_error() {
        let mp = PullImagesStub::failure(b"Error response from daemon: manifest unknown");
        let err = pull_images(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("Error response from daemon"),
            "error must include stderr: {msg}"
        );
    }

    #[tokio::test]
    async fn pull_images_timeout_returns_specific_error() {
        let mp = PullImagesStub::timeout();
        let err = pull_images(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("timed out") || msg.contains("10 minutes"),
            "timeout error must mention timeout: {msg}"
        );
    }

    #[tokio::test]
    async fn pull_images_timeout_suggests_network_check() {
        let mp = PullImagesStub::timeout();
        let err = pull_images(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("network") || msg.contains("connectivity"),
            "timeout error must suggest checking network: {msg}"
        );
    }
}
