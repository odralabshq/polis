//! Application service — workspace repair use-case.
//!
//! Orchestrates the repair of a workspace by checking and fixing various
//! infrastructure components in order.

use anyhow::{Context, Result};

use crate::application::ports::{FileTransfer, InstanceInspector, ProgressReporter, ShellExecutor};
use crate::application::services::vm::provision::{generate_certs_and_secrets, transfer_config};

/// Repair the workspace.
///
/// # Errors
///
/// Returns an error if any repair step fails fatally.
pub async fn run_repair(
    mp: &(impl InstanceInspector + ShellExecutor + FileTransfer),
    reporter: &impl ProgressReporter,
    assets_dir: &std::path::Path,
    version: &str,
    health_checks_failed: bool,
) -> Result<()> {
    // When health checks failed, VM state is untrusted — always re-transfer config
    // from host to prevent tampered scripts from persisting and running as root.
    if health_checks_failed {
        reporter.step("Re-transferring config (health checks failed, VM state untrusted)...");
        transfer_config(mp, assets_dir, version)
            .await
            .context("re-transferring config to VM")?;
        reporter.success("Config re-transferred");
    }

    // 1. Ensure Docker is running
    reporter.step("Checking Docker daemon...");
    let docker_ok = mp
        .exec(&["docker", "info"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if docker_ok {
        reporter.step("Docker already running");
    } else {
        reporter.step("Restarting Docker...");
        mp.exec(&["sudo", "systemctl", "restart", "docker"])
            .await
            .context("restarting docker")?;
        reporter.success("Docker restarted");
    }

    // 2. Ensure sysbox runtime is registered
    reporter.step("Checking sysbox runtime...");
    let sysbox_ok = mp
        .exec(&["docker", "info", "--format", "{{.Runtimes}}"])
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("sysbox-runc"))
        .unwrap_or(false);
    if sysbox_ok {
        reporter.step("sysbox-runc already registered");
    } else {
        reporter.step("Restarting sysbox and Docker...");
        mp.exec(&["sudo", "systemctl", "restart", "sysbox"])
            .await
            .context("restarting sysbox")?;
        mp.exec(&["sudo", "systemctl", "restart", "docker"])
            .await
            .context("restarting docker after sysbox")?;
        reporter.success("sysbox and Docker restarted");
    }

    // 3. Re-transfer config if /opt/polis is missing or .env is absent
    reporter.step("Checking /opt/polis config...");
    let config_ok = mp
        .exec(&["test", "-f", "/opt/polis/.env"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if config_ok {
        reporter.step("/opt/polis config present");
    } else {
        reporter.step("Re-transferring config...");
        transfer_config(mp, assets_dir, version)
            .await
            .context("re-transferring config to VM")?;
        reporter.success("Config re-transferred");
    }

    // 3.5: Ensure certs exist and are not expiring within 7 days
    let mut certs_regenerated = false;
    reporter.step("Checking certificates...");
    let sentinel_ok = mp
        .exec(&["test", "-f", "/opt/polis/.certs-ready"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    // Also check cert expiry — regenerate if expiring within 7 days (604800 seconds)
    let cert_expiry_ok = if sentinel_ok {
        mp.exec(&[
            "bash",
            "-c",
            "openssl x509 -in /opt/polis/certs/valkey/server.crt -checkend 604800 -noout 2>/dev/null",
        ])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
    } else {
        false
    };

    if sentinel_ok && cert_expiry_ok {
        reporter.step("Certificates present and valid");
    } else {
        if sentinel_ok && !cert_expiry_ok {
            reporter.step("Certificates expiring soon, forcing regeneration...");
            mp.exec(&["rm", "-f", "/opt/polis/.certs-ready"]).await.ok();
        }
        reporter.step("Generating certificates and secrets...");
        generate_certs_and_secrets(mp)
            .await
            .context("generating certificates during repair")?;
        certs_regenerated = true;
        reporter.success("Certificates generated");
    }

    // 4. Ensure polis.service is enabled
    reporter.step("Checking polis.service...");
    let service_enabled = mp
        .exec(&["systemctl", "is-enabled", "polis.service"])
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(false);
    if service_enabled {
        reporter.step("polis.service already enabled");
    } else {
        reporter.step("Enabling polis.service...");
        mp.exec(&["sudo", "systemctl", "enable", "polis.service"])
            .await
            .context("enabling polis.service")?;
        reporter.success("polis.service enabled");
    }

    // 5. Restart compose services — use compose down when certs were regenerated
    // to ensure credential consistency (prevents rolling restart mismatch where
    // some containers read new secrets while Valkey still has old ones).
    if certs_regenerated {
        reporter.step("Stopping services for clean restart...");
        mp.exec(&["bash", "-c", "cd /opt/polis && docker compose down"])
            .await
            .context("stopping services before restart")?;
    }
    reporter.step("Restarting services...");
    mp.exec(&[
        "bash",
        "-c",
        "cd /opt/polis && docker compose --env-file .env up -d --remove-orphans",
    ])
    .await
    .context("restarting compose services")?;
    reporter.success("Services restarted");

    Ok(())
}
