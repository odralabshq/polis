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
    if health_checks_failed {
        retransfer_config_forced(mp, reporter, assets_dir, version).await?;
    }
    ensure_docker_running(mp, reporter).await?;
    ensure_sysbox_registered(mp, reporter).await?;
    ensure_config_present(mp, reporter, assets_dir, version).await?;
    let certs_regenerated = ensure_certs_valid(mp, reporter).await?;
    ensure_polis_service_enabled(mp, reporter).await?;
    restart_compose_services(mp, reporter, certs_regenerated).await?;
    Ok(())
}

async fn retransfer_config_forced(
    mp: &(impl ShellExecutor + FileTransfer),
    reporter: &impl ProgressReporter,
    assets_dir: &std::path::Path,
    version: &str,
) -> Result<()> {
    reporter.step("Re-transferring config (health checks failed, VM state untrusted)...");
    transfer_config(mp, assets_dir, version)
        .await
        .context("re-transferring config to VM")?;
    reporter.success("Config re-transferred");
    Ok(())
}

async fn ensure_docker_running(
    mp: &impl ShellExecutor,
    reporter: &impl ProgressReporter,
) -> Result<()> {
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
    Ok(())
}

async fn ensure_sysbox_registered(
    mp: &impl ShellExecutor,
    reporter: &impl ProgressReporter,
) -> Result<()> {
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
    Ok(())
}

async fn ensure_config_present(
    mp: &(impl ShellExecutor + FileTransfer),
    reporter: &impl ProgressReporter,
    assets_dir: &std::path::Path,
    version: &str,
) -> Result<()> {
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
    Ok(())
}

async fn ensure_certs_valid(
    mp: &impl ShellExecutor,
    reporter: &impl ProgressReporter,
) -> Result<bool> {
    reporter.step("Checking certificates...");
    let sentinel_ok = mp
        .exec(&["test", "-f", "/opt/polis/.certs-ready"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

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
        Ok(false)
    } else {
        if sentinel_ok && !cert_expiry_ok {
            reporter.step("Certificates expiring soon, forcing regeneration...");
            mp.exec(&["rm", "-f", "/opt/polis/.certs-ready"]).await.ok();
        }
        reporter.step("Generating certificates and secrets...");
        generate_certs_and_secrets(mp)
            .await
            .context("generating certificates during repair")?;
        reporter.success("Certificates generated");
        Ok(true)
    }
}

async fn ensure_polis_service_enabled(
    mp: &impl ShellExecutor,
    reporter: &impl ProgressReporter,
) -> Result<()> {
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
    Ok(())
}

async fn restart_compose_services(
    mp: &impl ShellExecutor,
    reporter: &impl ProgressReporter,
    certs_regenerated: bool,
) -> Result<()> {
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
