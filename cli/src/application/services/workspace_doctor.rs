//! Application service — workspace doctor use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::Result;

use crate::application::ports::{
    CommandRunner, FileTransfer, InstanceInspector, LocalPaths, NetworkProbe, ProgressReporter,
    ShellExecutor,
};
use crate::domain::health::DoctorChecks;

/// Run the doctor probe/diagnose workflow.
///
/// Accepts port trait bounds so the caller can inject real or mock
/// implementations. The service never touches `OutputContext` or any
/// presentation type — rendering is the caller's responsibility.
///
/// # Errors
///
/// Returns an error if any health probe fails to execute.
#[allow(dead_code)] // Public API — not yet called from commands/doctor.rs
pub async fn run_doctor(
    provisioner: &(impl InstanceInspector + ShellExecutor + FileTransfer),
    reporter: &impl ProgressReporter,
    cmd_runner: &impl CommandRunner,
    network_probe: &impl NetworkProbe,
    paths: &impl LocalPaths,
) -> Result<DoctorChecks> {
    reporter.step("checking prerequisites...");
    let prerequisites = probe_prerequisites(cmd_runner).await?;

    reporter.step("checking workspace...");
    let workspace = probe_workspace(provisioner, cmd_runner, paths).await?;

    reporter.step("checking network...");
    let network = probe_network(network_probe).await?;

    reporter.step("checking security...");
    let security = probe_security(provisioner).await?;

    reporter.success("diagnostics complete");

    Ok(DoctorChecks {
        prerequisites,
        workspace,
        network,
        security,
    })
}

// ── Internal probes ───────────────────────────────────────────────────────────

async fn probe_prerequisites(
    cmd_runner: &impl CommandRunner,
) -> Result<crate::domain::health::PrerequisiteChecks> {
    let output = cmd_runner.run("multipass", &["version"]).await;
    let Ok(output) = output else {
        return Ok(crate::domain::health::PrerequisiteChecks {
            multipass_found: false,
            multipass_version: None,
            multipass_version_ok: false,
        });
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version_str = stdout
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .map(str::to_owned);

    let version_ok = version_str
        .as_deref()
        .and_then(|v| semver::Version::parse(v).ok())
        .is_none_or(|v| v >= semver::Version::new(1, 16, 0));

    Ok(crate::domain::health::PrerequisiteChecks {
        multipass_found: true,
        multipass_version: version_str,
        multipass_version_ok: version_ok,
    })
}

async fn probe_workspace(
    provisioner: &(impl InstanceInspector + ShellExecutor),
    cmd_runner: &impl CommandRunner,
    paths: &impl LocalPaths,
) -> Result<crate::domain::health::WorkspaceChecks> {
    let disk_space_gb = probe_disk_space_gb(cmd_runner).await?;
    let image = probe_image_cache(paths);

    // Check VM readiness via provisioner
    let ready = crate::application::services::vm::lifecycle::state(provisioner)
        .await
        .ok()
        == Some(crate::application::services::vm::lifecycle::VmState::Running);

    Ok(crate::domain::health::WorkspaceChecks {
        ready,
        disk_space_gb,
        disk_space_ok: disk_space_gb >= 10,
        image,
    })
}

async fn probe_network(
    network_probe: &impl NetworkProbe,
) -> Result<crate::domain::health::NetworkChecks> {
    let internet = network_probe
        .check_tcp_connectivity("8.8.8.8", 53)
        .await
        .unwrap_or(false);
    let dns = network_probe
        .check_dns_resolution("dns.google")
        .await
        .unwrap_or(false);
    Ok(crate::domain::health::NetworkChecks { internet, dns })
}

async fn probe_security(
    provisioner: &(impl InstanceInspector + ShellExecutor),
) -> Result<crate::domain::health::SecurityChecks> {
    let vm_running = crate::application::services::vm::lifecycle::state(provisioner)
        .await
        .ok()
        == Some(crate::application::services::vm::lifecycle::VmState::Running);

    if !vm_running {
        return Ok(crate::domain::health::SecurityChecks {
            process_isolation: false,
            traffic_inspection: false,
            malware_db_current: false,
            malware_db_age_hours: 0,
            certificates_valid: false,
            certificates_expire_days: 0,
        });
    }

    let (
        process_isolation,
        traffic_inspection,
        (malware_db_current, malware_db_age_hours),
        (certificates_valid, certificates_expire_days),
    ) = tokio::join!(
        probe_process_isolation(provisioner),
        probe_gate_health(provisioner),
        probe_malware_db(provisioner),
        probe_certificates(provisioner),
    );

    Ok(crate::domain::health::SecurityChecks {
        process_isolation,
        traffic_inspection,
        malware_db_current,
        malware_db_age_hours,
        certificates_valid,
        certificates_expire_days,
    })
}

// ── Low-level probe helpers ───────────────────────────────────────────────────

async fn probe_disk_space_gb(cmd_runner: &impl CommandRunner) -> Result<u64> {
    #[cfg(windows)]
    {
        let out = cmd_runner
            .run(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    "((Get-PSDrive C).Free / 1GB) -as [int]",
                ],
            )
            .await?;
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("cannot parse disk space: {e}"))
    }
    #[cfg(not(windows))]
    {
        let out = cmd_runner.run("df", &["-k", "/"]).await?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .nth(1)
            .and_then(|l| l.split_whitespace().nth(3))
            .and_then(|s| s.parse::<u64>().ok())
            .map(|kb| kb / (1024 * 1024))
            .ok_or_else(|| anyhow::anyhow!("cannot parse df output"))
    }
}

fn probe_image_cache(paths: &impl LocalPaths) -> crate::domain::health::ImageCheckResult {
    let images_dir = paths.images_dir();
    let cached = images_dir.join("polis.qcow2").exists();
    crate::domain::health::ImageCheckResult {
        cached,
        version: None,
        sha256_preview: None,
        polis_image_override: std::env::var("POLIS_IMAGE").ok(),
        version_drift: None,
    }
}

async fn probe_process_isolation(mp: &impl ShellExecutor) -> bool {
    mp.exec(&["sysbox-runc", "--version"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn probe_gate_health(mp: &impl ShellExecutor) -> bool {
    let output = mp
        .exec(&[
            "docker",
            "compose",
            "-f",
            crate::domain::workspace::COMPOSE_PATH,
            "ps",
            "--format",
            "json",
            "gate",
        ])
        .await;
    let Ok(output) = output else { return false };
    if !output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .next()
        .and_then(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|c| c.get("State")?.as_str().map(|s| s == "running"))
        })
        .unwrap_or(false)
}

async fn probe_malware_db(mp: &impl ShellExecutor) -> (bool, u64) {
    let output = mp
        .exec(&[
            "docker", "exec", "polis-scanner", "sh", "-c",
            "stat -c %Y /var/lib/clamav/daily.cld /var/lib/clamav/daily.cvd 2>/dev/null | sort -rn | head -1",
        ])
        .await;
    let Ok(output) = output else {
        return (false, 0);
    };
    if !output.status.success() {
        return (false, 0);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Ok(mtime) = stdout.trim().parse::<u64>() else {
        return (false, 0);
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age_hours = now.saturating_sub(mtime) / 3600;
    (age_hours <= 24, age_hours)
}

async fn probe_certificates(mp: &impl ShellExecutor) -> (bool, i64) {
    let output = mp
        .exec(&[
            "openssl",
            "x509",
            "-enddate",
            "-noout",
            "-in",
            "/opt/polis/certs/ca/ca.pem",
        ])
        .await;
    let Ok(output) = output else {
        return (false, 0);
    };
    if !output.status.success() {
        return (false, 0);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(date_str) = stdout.strip_prefix("notAfter=").map(str::trim) else {
        return (false, 0);
    };
    let Ok(expiry) = chrono::NaiveDateTime::parse_from_str(date_str, "%b %d %H:%M:%S %Y GMT")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(date_str, "%b  %d %H:%M:%S %Y GMT"))
    else {
        return (false, 0);
    };
    let now = chrono::Utc::now().naive_utc();
    let days = (expiry - now).num_days();
    (days > 0, days)
}
