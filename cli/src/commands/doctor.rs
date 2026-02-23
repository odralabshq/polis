//! `polis doctor` — system health diagnostics.

use std::path::Path;

use anyhow::{Context, Result};
use owo_colors::OwoColorize;
use serde::Serialize;

use crate::output::OutputContext;

// ── Public types ──────────────────────────────────────────────────────────────

/// All check categories returned by the doctor command.
#[derive(Debug)]
pub struct DoctorChecks {
    /// Prerequisite checks (multipass version, hypervisor).
    pub prerequisites: PrerequisiteChecks,
    /// Workspace health.
    pub workspace: WorkspaceChecks,
    /// Network health.
    pub network: NetworkChecks,
    /// Security health.
    pub security: SecurityChecks,
}

/// Prerequisite checks — multipass version and platform hypervisor.
#[derive(Debug)]
pub struct PrerequisiteChecks {
    /// Whether `multipass` is on PATH.
    pub multipass_found: bool,
    /// Installed Multipass version string (e.g. `"1.16.1"`), if found.
    pub multipass_version: Option<String>,
    /// Whether the installed version meets the minimum (1.16.0).
    pub multipass_version_ok: bool,
    /// Linux only: whether the `removable-media` snap interface is connected.
    pub removable_media_connected: Option<bool>,
}

/// Workspace health checks.
#[derive(Debug)]
pub struct WorkspaceChecks {
    /// Whether the workspace can be started.
    pub ready: bool,
    /// Available disk space in GB.
    pub disk_space_gb: u64,
    /// Whether disk space meets the 10 GB minimum.
    pub disk_space_ok: bool,
    /// Image cache status.
    pub image: ImageCheckResult,
}

/// Result of image health checks.
#[derive(Debug, Default, Serialize)]
pub struct ImageCheckResult {
    /// Whether a cached image exists at `~/.polis/images/polis.qcow2`.
    pub cached: bool,
    /// Version from `image.json` (if available).
    pub version: Option<String>,
    /// SHA-256 preview (first 12 hex chars) from `image.json`.
    pub sha256_preview: Option<String>,
    /// Value of `POLIS_IMAGE` env var if set (override active).
    pub polis_image_override: Option<String>,
    /// Whether cached version differs from latest available.
    pub version_drift: Option<VersionDrift>,
}

/// Version drift between cached and latest available image.
#[derive(Debug, Serialize)]
pub struct VersionDrift {
    /// Currently cached version.
    pub current: String,
    /// Latest available version.
    pub latest: String,
}

/// Network health checks.
#[derive(Debug)]
pub struct NetworkChecks {
    /// Whether internet connectivity is available.
    pub internet: bool,
    /// Whether DNS resolution is working.
    pub dns: bool,
}

/// Security health checks.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // fields are spec-mandated; a bitfield would obscure intent
pub struct SecurityChecks {
    /// Whether process isolation (sysbox) is active.
    pub process_isolation: bool,
    /// Whether traffic inspection is responding.
    pub traffic_inspection: bool,
    /// Whether the malware scanner database is current.
    pub malware_db_current: bool,
    /// Hours since the malware database was last updated.
    pub malware_db_age_hours: u64,
    /// Whether certificates are valid.
    pub certificates_valid: bool,
    /// Days until certificate expiry (≤ 0 means expired).
    pub certificates_expire_days: i64,
}

// ── HealthProbe trait ─────────────────────────────────────────────────────────

/// Abstraction over health check backends, enabling test doubles.
#[allow(async_fn_in_trait)] // Send bounds not required; probe is always called on the same task
pub trait HealthProbe {
    /// Check prerequisite health (multipass version, hypervisor).
    ///
    /// # Errors
    ///
    /// Returns an error if the prerequisite check cannot be performed.
    async fn check_prerequisites(&self) -> Result<PrerequisiteChecks>;

    /// Check workspace health.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace check cannot be performed.
    async fn check_workspace(&self) -> Result<WorkspaceChecks>;

    /// Check network health.
    ///
    /// # Errors
    ///
    /// Returns an error if the network check cannot be performed.
    async fn check_network(&self) -> Result<NetworkChecks>;

    /// Check security health.
    ///
    /// # Errors
    ///
    /// Returns an error if the security check cannot be performed.
    async fn check_security(&self) -> Result<SecurityChecks>;
}

/// Production implementation that queries the real system.
pub struct SystemProbe<'a, M: crate::multipass::Multipass> {
    mp: &'a M,
}

impl<'a, M: crate::multipass::Multipass> SystemProbe<'a, M> {
    /// Create a new system probe with the given multipass implementation.
    pub fn new(mp: &'a M) -> Self {
        Self { mp }
    }
}

impl<M: crate::multipass::Multipass> HealthProbe for SystemProbe<'_, M> {
    async fn check_prerequisites(&self) -> Result<PrerequisiteChecks> {
        tokio::task::spawn_blocking(probe_prerequisites)
            .await
            .context("prerequisites check task panicked")
    }

    async fn check_workspace(&self) -> Result<WorkspaceChecks> {
        let (disk_space_gb, image) = tokio::join!(disk_space_gb(), check_image());
        let disk_space_gb = disk_space_gb?;
        Ok(WorkspaceChecks {
            ready: true,
            disk_space_gb,
            disk_space_ok: disk_space_gb >= 10,
            image,
        })
    }

    async fn check_network(&self) -> Result<NetworkChecks> {
        let (internet, dns) = tokio::join!(check_internet(), check_dns());
        Ok(NetworkChecks { internet, dns })
    }

    async fn check_security(&self) -> Result<SecurityChecks> {
        let vm_running = crate::workspace::vm::state(self.mp).await.ok()
            == Some(crate::workspace::vm::VmState::Running);

        let process_isolation = check_process_isolation().await;

        if !vm_running {
            return Ok(SecurityChecks {
                process_isolation,
                traffic_inspection: false,
                malware_db_current: false,
                malware_db_age_hours: 0,
                certificates_valid: false,
                certificates_expire_days: 0,
            });
        }

        let (
            traffic_inspection,
            (malware_db_current, malware_db_age_hours),
            (certificates_valid, certificates_expire_days),
        ) = tokio::join!(
            check_gate_health(self.mp),
            check_malware_db(self.mp),
            check_certificates(self.mp),
        );
        Ok(SecurityChecks {
            process_isolation,
            traffic_inspection,
            malware_db_current,
            malware_db_age_hours,
            certificates_valid,
            certificates_expire_days,
        })
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run `polis doctor`.
///
/// # Errors
///
/// Returns an error if health checks cannot be executed or output fails.
pub async fn run(
    ctx: &OutputContext,
    json: bool,
    verbose: bool,
    mp: &impl crate::multipass::Multipass,
) -> Result<()> {
    run_with(ctx, json, verbose, &SystemProbe::new(mp)).await
}

/// Run doctor with a custom health probe (for testing).
///
/// # Errors
///
/// Returns an error if health checks cannot be executed or output fails.
pub async fn run_with(
    ctx: &OutputContext,
    json: bool,
    verbose: bool,
    probe: &impl HealthProbe,
) -> Result<()> {
    let (prerequisites, workspace, network, security) = tokio::try_join!(
        probe.check_prerequisites(),
        probe.check_workspace(),
        probe.check_network(),
        probe.check_security(),
    )?;
    let checks = DoctorChecks {
        prerequisites,
        workspace,
        network,
        security,
    };
    let issues = collect_issues(&checks);

    if json {
        print_json_output(&checks, &issues)?;
    } else {
        print_human_output(ctx, &checks, &issues, verbose);
    }
    Ok(())
}

/// Build and print JSON output for doctor checks.
fn print_json_output(checks: &DoctorChecks, issues: &[String]) -> Result<()> {
    let status = if issues.is_empty() {
        "healthy"
    } else {
        "unhealthy"
    };
    let out = serde_json::json!({
        "status": status,
        "checks": {
            "prerequisites": {
                "multipass_found": checks.prerequisites.multipass_found,
                "multipass_version": checks.prerequisites.multipass_version,
                "multipass_version_ok": checks.prerequisites.multipass_version_ok,
                "removable_media_connected": checks.prerequisites.removable_media_connected,
            },
            "workspace": {
                "ready": checks.workspace.ready,
                "disk_space_gb": checks.workspace.disk_space_gb,
                "disk_space_ok": checks.workspace.disk_space_ok,
                "image": checks.workspace.image,
            },
            "network": {
                "internet": checks.network.internet,
                "dns": checks.network.dns,
            },
            "security": {
                "process_isolation": checks.security.process_isolation,
                "traffic_inspection": checks.security.traffic_inspection,
                "malware_db_current": checks.security.malware_db_current,
                "malware_db_age_hours": checks.security.malware_db_age_hours,
                "certificates_valid": checks.security.certificates_valid,
                "certificates_expire_days": checks.security.certificates_expire_days,
            },
        },
        "issues": issues,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).context("JSON serialization")?
    );
    Ok(())
}

/// Print human-readable doctor output.
fn print_human_output(
    ctx: &OutputContext,
    checks: &DoctorChecks,
    issues: &[String],
    verbose: bool,
) {
    println!();
    println!("  {}", "Polis Health Check".style(ctx.styles.header));
    println!();

    print_prerequisites_section(ctx, &checks.prerequisites);
    print_workspace_section(ctx, &checks.workspace);
    print_network_section(ctx, &checks.network);
    print_security_section(ctx, &checks.security);
    print_summary(ctx, issues, verbose);

    println!();
}

/// Print the network section of the human-readable health report.
fn print_network_section(ctx: &OutputContext, net: &NetworkChecks) {
    println!("  Network:");
    print_check(ctx, net.internet, "Internet connectivity");
    print_check(ctx, net.dns, "DNS resolution working");
    println!();
}

/// Print the summary section with issues.
fn print_summary(ctx: &OutputContext, issues: &[String], verbose: bool) {
    println!();
    if issues.is_empty() {
        println!("  {} Everything looks good!", "✓".style(ctx.styles.success));
        return;
    }
    let hint = if verbose {
        ""
    } else {
        " Run with --verbose for details."
    };
    println!(
        "  {} Found {} issues.{hint}",
        "✗".style(ctx.styles.error),
        issues.len(),
    );
    if verbose {
        println!();
        for issue in issues {
            println!("    {} {issue}", "✗".style(ctx.styles.error));
        }
    }
}

/// Print the prerequisites section of the human-readable health report.
fn print_prerequisites_section(ctx: &OutputContext, pre: &PrerequisiteChecks) {
    println!("  Prerequisites:");
    if !pre.multipass_found {
        print_check(ctx, false, "multipass not found");
        #[cfg(target_os = "linux")]
        println!("      Install: sudo snap install multipass");
        #[cfg(not(target_os = "linux"))]
        println!("      Install: https://multipass.run/install");
        println!();
        return;
    }
    let ver = pre.multipass_version.as_deref().unwrap_or("unknown");
    print_check(
        ctx,
        pre.multipass_version_ok,
        &format!("Multipass {ver} (need ≥ 1.16.0)"),
    );
    if !pre.multipass_version_ok {
        #[cfg(target_os = "linux")]
        println!("      Update: sudo snap refresh multipass");
        #[cfg(not(target_os = "linux"))]
        println!("      Update: https://multipass.run/install");
    }
    if let Some(connected) = pre.removable_media_connected {
        print_check(ctx, connected, "removable-media interface connected");
        if !connected {
            println!("      Fix: sudo snap connect multipass:removable-media");
        }
    }
    println!();
}

/// Print the workspace section of the human-readable health report.
fn print_workspace_section(ctx: &OutputContext, ws: &WorkspaceChecks) {
    println!("  Workspace:");
    print_check(ctx, ws.ready, "Ready to start");

    let img = &ws.image;
    if img.cached {
        let version_str = img.version.as_deref().unwrap_or("unknown");
        let sha_str = img
            .sha256_preview
            .as_deref()
            .map(|s| format!(" (SHA256: {s}...)"))
            .unwrap_or_default();
        print_check(ctx, true, &format!("Image cached: {version_str}{sha_str}"));
        if let Some(drift) = &img.version_drift {
            println!(
                "    {} Newer image available: {}",
                "⚠".style(ctx.styles.warning),
                drift.latest
            );
            println!("      Update with: polis delete --all && polis start");
        }
    } else {
        print_check(ctx, false, "No workspace image cached");
        println!("      Run 'polis start' to download the image (~3.2 GB)");
    }

    if ws.disk_space_ok {
        print_check(
            ctx,
            true,
            &format!("{} GB disk space available", ws.disk_space_gb),
        );
    } else {
        print_check(
            ctx,
            false,
            &format!(
                "Low disk space ({} GB available, need 10 GB)",
                ws.disk_space_gb
            ),
        );
    }
    println!();

    // POLIS_IMAGE override warning (V-011, F-006)
    if let Some(override_path) = &img.polis_image_override {
        println!(
            "  {} POLIS_IMAGE override active: {override_path}",
            "⚠".style(ctx.styles.warning)
        );
        if std::path::Path::new(override_path).exists() {
            println!("    This overrides the default image in ~/.polis/images/");
        } else {
            println!(
                "    {} POLIS_IMAGE set but file not found: {override_path}",
                "⚠".style(ctx.styles.warning)
            );
        }
        println!("    Unset with: unset POLIS_IMAGE");
        println!();
    }
}

/// Print the security section of the human-readable health report.
fn print_security_section(ctx: &OutputContext, sec: &SecurityChecks) {
    println!("  Security:");
    print_check(ctx, sec.process_isolation, "Process isolation active");
    print_check(ctx, sec.traffic_inspection, "Traffic inspection responding");
    print_check(
        ctx,
        sec.malware_db_current,
        &format!(
            "Malware scanner database current (updated: {}h ago)",
            sec.malware_db_age_hours,
        ),
    );

    let expire_days = sec.certificates_expire_days;
    if expire_days > 30 {
        print_check(
            ctx,
            true,
            "Certificates valid (no immediate action required)",
        );
    } else if expire_days > 0 {
        println!(
            "    {} Certificates expire soon",
            "⚠".style(ctx.styles.warning)
        );
    } else {
        print_check(ctx, false, "Certificates expired");
    }
}

fn print_check(ctx: &OutputContext, ok: bool, msg: &str) {
    if ok {
        println!("    {} {msg}", "✓".style(ctx.styles.success));
    } else {
        println!("    {} {msg}", "✗".style(ctx.styles.error));
    }
}

// ── Issue collection ──────────────────────────────────────────────────────────

/// Collect actionable issues from check results.
///
/// Certificates expiring in 1–30 days are a warning only and are NOT included.
#[must_use]
pub fn collect_issues(checks: &DoctorChecks) -> Vec<String> {
    let mut issues = Vec::new();
    if !checks.prerequisites.multipass_found {
        issues.push("multipass is not installed".to_string());
    } else if !checks.prerequisites.multipass_version_ok {
        let ver = checks
            .prerequisites
            .multipass_version
            .as_deref()
            .unwrap_or("unknown");
        issues.push(format!("Multipass {ver} is too old (need ≥ 1.16.0)"));
    }
    if !checks.workspace.disk_space_ok {
        issues.push(format!(
            "Low disk space ({} GB available, need 10 GB)",
            checks.workspace.disk_space_gb,
        ));
    }
    if !checks.network.dns {
        issues.push("DNS resolution failed".to_string());
    }
    if !checks.security.traffic_inspection {
        issues.push("Traffic inspection not responding".to_string());
    }
    if !checks.security.malware_db_current {
        issues.push(format!(
            "Malware scanner database stale (updated: {}h ago)",
            checks.security.malware_db_age_hours
        ));
    }
    if checks.security.certificates_expire_days <= 0 {
        issues.push("Certificates expired".to_string());
    }
    issues
}

// ── System check helpers ──────────────────────────────────────────────────────

/// Blocking probe for prerequisite checks (runs in `spawn_blocking`).
fn probe_prerequisites() -> PrerequisiteChecks {
    use std::process::Command;

    let output = Command::new("multipass").arg("version").output();
    let Ok(output) = output else {
        return PrerequisiteChecks {
            multipass_found: false,
            multipass_version: None,
            multipass_version_ok: false,
            removable_media_connected: None,
        };
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

    // Linux only: check removable-media snap interface
    #[cfg(target_os = "linux")]
    let removable_media_connected = {
        let conn = Command::new("snap")
            .args(["connections", "multipass"])
            .output()
            .ok();
        conn.map(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            // Slot column reads " :removable-media" when connected; plug name
            // "multipass:removable-media" has no leading space before ":".
            text.contains(" :removable-media")
        })
    };
    #[cfg(not(target_os = "linux"))]
    let removable_media_connected = None;

    PrerequisiteChecks {
        multipass_found: true,
        multipass_version: version_str,
        multipass_version_ok: version_ok,
        removable_media_connected,
    }
}

/// Check image cache status, metadata, `POLIS_IMAGE` override, and version drift.
async fn check_image() -> ImageCheckResult {
    let Ok(images_dir) = crate::workspace::image::images_dir() else {
        return ImageCheckResult::default();
    };
    let cached = images_dir.join("polis.qcow2").exists();

    let (version, sha256_preview) = if cached {
        read_image_json(&images_dir)
    } else {
        (None, None)
    };

    let polis_image_override = std::env::var("POLIS_IMAGE").ok();

    let version_drift = match version.clone() {
        Some(current) => check_version_drift(current).await,
        None => None,
    };

    ImageCheckResult {
        cached,
        version,
        sha256_preview,
        polis_image_override,
        version_drift,
    }
}

/// Read version and SHA-256 preview from `image.json`. Fails silently.
fn read_image_json(images_dir: &Path) -> (Option<String>, Option<String>) {
    let Ok(content) = std::fs::read_to_string(images_dir.join("image.json")) else {
        return (None, None);
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return (None, None);
    };
    let version = val
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let sha256_preview = val
        .get("sha256")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(12).collect());
    (version, sha256_preview)
}

/// Compare cached version against latest GitHub release. Returns `None` on any failure.
async fn check_version_drift(current: String) -> Option<VersionDrift> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(crate::workspace::image::resolve_latest_image_url),
    )
    .await;
    let Ok(Ok(Ok(resolved))) = result else {
        return None;
    };
    if resolved.tag == current {
        None
    } else {
        Some(VersionDrift {
            current,
            latest: resolved.tag,
        })
    }
}

async fn disk_space_gb() -> Result<u64> {
    let out = tokio::process::Command::new("df")
        .args(["-BG", "/"])
        .output()
        .await
        .context("df failed")?;
    let text = String::from_utf8_lossy(&out.stdout);
    // df -BG output (second line): "/dev/sda1  100G  55G  45G  55% /"
    // column index 3 is "Available", e.g. "45G"
    text.lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().nth(3))
        .and_then(|s| s.trim_end_matches('G').parse::<u64>().ok())
        .ok_or_else(|| anyhow::anyhow!("cannot parse df output"))
}

async fn check_internet() -> bool {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;
    const ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53);
    tokio::task::spawn_blocking(|| {
        std::net::TcpStream::connect_timeout(&ADDR, Duration::from_secs(3)).is_ok()
    })
    .await
    .unwrap_or(false)
}

async fn check_dns() -> bool {
    tokio::task::spawn_blocking(|| {
        use std::net::ToSocketAddrs;
        "dns.google:443".to_socket_addrs().is_ok()
    })
    .await
    .unwrap_or(false)
}

async fn check_process_isolation() -> bool {
    tokio::process::Command::new("sysbox-runc")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if the gate container is running inside the multipass VM.
async fn check_gate_health(mp: &impl crate::multipass::Multipass) -> bool {
    let output = mp
        .exec(&[
            "docker",
            "compose",
            "-f",
            crate::workspace::COMPOSE_PATH,
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

/// Check `ClamAV` database freshness inside the multipass VM.
async fn check_malware_db(mp: &impl crate::multipass::Multipass) -> (bool, u64) {
    // Find the newest DB file (daily.cvd or daily.cld) in the scanner container.
    let output = mp
        .exec(&[
            "docker",
            "exec",
            "polis-scanner",
            "sh",
            "-c",
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
    // ClamAV database is considered current if updated within 24 hours
    (age_hours <= 24, age_hours)
}

/// Check CA certificate expiry inside the multipass VM.
async fn check_certificates(mp: &impl crate::multipass::Multipass) -> (bool, i64) {
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

    // Output format: "notAfter=Feb 15 12:00:00 2036 GMT"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(date_str) = stdout.strip_prefix("notAfter=").map(str::trim) else {
        return (false, 0);
    };

    // Parse the date and compute days until expiry
    let Ok(expiry) = chrono::NaiveDateTime::parse_from_str(date_str, "%b %d %H:%M:%S %Y GMT")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(date_str, "%b  %d %H:%M:%S %Y GMT"))
    else {
        return (false, 0);
    };

    let now = chrono::Utc::now().naive_utc();
    let days = (expiry - now).num_days();
    (days > 0, days)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{
        DoctorChecks, ImageCheckResult, NetworkChecks, PrerequisiteChecks, SecurityChecks,
        VersionDrift, WorkspaceChecks, collect_issues,
    };

    fn all_healthy() -> DoctorChecks {
        DoctorChecks {
            prerequisites: PrerequisiteChecks {
                multipass_found: true,
                multipass_version: Some("1.16.1".to_string()),
                multipass_version_ok: true,
                removable_media_connected: None,
            },
            workspace: WorkspaceChecks {
                ready: true,
                disk_space_gb: 45,
                disk_space_ok: true,
                image: ImageCheckResult {
                    cached: true,
                    version: Some("v0.3.0".to_string()),
                    sha256_preview: Some("a1b2c3d4e5f6".to_string()),
                    polis_image_override: None,
                    version_drift: None,
                },
            },
            network: NetworkChecks {
                internet: true,
                dns: true,
            },
            security: SecurityChecks {
                process_isolation: true,
                traffic_inspection: true,
                malware_db_current: true,
                malware_db_age_hours: 2,
                certificates_valid: true,
                certificates_expire_days: 365,
            },
        }
    }

    #[test]
    fn test_collect_issues_all_healthy_returns_empty() {
        let issues = collect_issues(&all_healthy());
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn test_collect_issues_low_disk_returns_disk_issue() {
        let mut checks = all_healthy();
        checks.workspace.disk_space_gb = 2;
        checks.workspace.disk_space_ok = false;

        let issues = collect_issues(&checks);
        assert!(
            issues
                .iter()
                .any(|i: &String| i.contains("disk") || i.contains("Disk")),
            "expected a disk issue, got: {issues:?}"
        );
    }

    #[test]
    fn test_collect_issues_dns_failed_returns_dns_issue() {
        let mut checks = all_healthy();
        checks.network.dns = false;

        let issues = collect_issues(&checks);
        assert!(
            issues
                .iter()
                .any(|i: &String| i.to_lowercase().contains("dns")),
            "expected a DNS issue, got: {issues:?}"
        );
    }

    #[test]
    fn test_collect_issues_traffic_inspection_failed_returns_issue() {
        let mut checks = all_healthy();
        checks.security.traffic_inspection = false;

        let issues = collect_issues(&checks);
        assert!(
            issues
                .iter()
                .any(|i: &String| i.to_lowercase().contains("traffic")
                    || i.to_lowercase().contains("inspection")),
            "expected a traffic inspection issue, got: {issues:?}"
        );
    }

    #[test]
    fn test_collect_issues_expired_certs_returns_issue() {
        let mut checks = all_healthy();
        checks.security.certificates_expire_days = 0;

        let issues = collect_issues(&checks);
        assert!(
            issues
                .iter()
                .any(|i: &String| i.to_lowercase().contains("cert")),
            "expected a certificate issue, got: {issues:?}"
        );
    }

    #[test]
    fn test_collect_issues_expiring_soon_not_in_issues() {
        // Certs expiring in 7 days are a warning (⚠), not an error (✗).
        // They must NOT appear in the issues list.
        let mut checks = all_healthy();
        checks.security.certificates_expire_days = 7;

        let issues = collect_issues(&checks);
        assert!(
            !issues
                .iter()
                .any(|i: &String| i.to_lowercase().contains("cert")),
            "expiring-soon certs should be a warning, not an issue, got: {issues:?}"
        );
    }

    #[test]
    fn test_collect_issues_multiple_failures_all_collected() {
        let checks = DoctorChecks {
            prerequisites: PrerequisiteChecks {
                multipass_found: true,
                multipass_version: Some("1.16.1".to_string()),
                multipass_version_ok: true,
                removable_media_connected: None,
            },
            workspace: WorkspaceChecks {
                ready: true,
                disk_space_gb: 2,
                disk_space_ok: false,
                image: ImageCheckResult::default(),
            },
            network: NetworkChecks {
                internet: true,
                dns: false,
            },
            security: SecurityChecks {
                process_isolation: true,
                traffic_inspection: false,
                malware_db_current: true,
                malware_db_age_hours: 2,
                certificates_valid: true,
                certificates_expire_days: 365,
            },
        };

        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 3, "expected 3 issues, got: {issues:?}");
    }

    // -----------------------------------------------------------------------
    // read_image_json — unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_image_json_valid_json_extracts_version_and_sha256() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("image.json"),
            r#"{"version":"v0.3.0","sha256":"abcdef123456789012345678","arch":"amd64","downloaded_at":"2024-01-01T00:00:00Z","source":"https://example.com"}"#,
        )
        .expect("write");
        let (version, sha256_preview) = super::read_image_json(dir.path());
        assert_eq!(version.as_deref(), Some("v0.3.0"));
        assert_eq!(sha256_preview.as_deref(), Some("abcdef123456"));
    }

    #[test]
    fn test_read_image_json_missing_file_returns_none_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (version, sha256_preview) = super::read_image_json(dir.path());
        assert!(version.is_none());
        assert!(sha256_preview.is_none());
    }

    #[test]
    fn test_read_image_json_malformed_json_returns_none_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("image.json"), b"not json").expect("write");
        let (version, sha256_preview) = super::read_image_json(dir.path());
        assert!(version.is_none());
        assert!(sha256_preview.is_none());
    }

    #[test]
    fn test_read_image_json_missing_version_field_returns_none_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("image.json"),
            r#"{"sha256":"abcdef123456789012345678"}"#,
        )
        .expect("write");
        let (version, sha256_preview) = super::read_image_json(dir.path());
        assert!(version.is_none());
        assert_eq!(sha256_preview.as_deref(), Some("abcdef123456"));
    }

    #[test]
    fn test_read_image_json_sha256_preview_truncated_to_12_chars() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("image.json"),
            r#"{"version":"v0.3.0","sha256":"aabbccddeeff00112233445566778899"}"#,
        )
        .expect("write");
        let (_, sha256_preview) = super::read_image_json(dir.path());
        assert_eq!(sha256_preview.as_deref(), Some("aabbccddeeff"));
    }

    #[test]
    fn test_image_check_result_default_is_not_cached() {
        let r = ImageCheckResult::default();
        assert!(!r.cached);
        assert!(r.version.is_none());
        assert!(r.sha256_preview.is_none());
        assert!(r.polis_image_override.is_none());
        assert!(r.version_drift.is_none());
    }

    #[test]
    fn test_version_drift_fields_accessible() {
        let d = VersionDrift {
            current: "v0.3.0".to_string(),
            latest: "v0.3.1".to_string(),
        };
        assert_eq!(d.current, "v0.3.0");
        assert_eq!(d.latest, "v0.3.1");
    }

    // -----------------------------------------------------------------------
    // PrerequisiteChecks — collect_issues unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_collect_issues_multipass_not_found_returns_issue() {
        let mut checks = all_healthy();
        checks.prerequisites.multipass_found = false;
        checks.prerequisites.multipass_version = None;
        checks.prerequisites.multipass_version_ok = false;
        let issues = collect_issues(&checks);
        assert!(
            issues
                .iter()
                .any(|i| i.to_lowercase().contains("multipass")),
            "got: {issues:?}"
        );
    }

    #[test]
    fn test_collect_issues_multipass_version_too_old_returns_issue() {
        let mut checks = all_healthy();
        checks.prerequisites.multipass_version = Some("1.15.0".to_string());
        checks.prerequisites.multipass_version_ok = false;
        let issues = collect_issues(&checks);
        assert!(
            issues
                .iter()
                .any(|i| i.contains("1.15.0") || i.to_lowercase().contains("old")),
            "got: {issues:?}"
        );
    }

    #[test]
    fn test_collect_issues_removable_media_connected_no_issue() {
        let mut checks = all_healthy();
        checks.prerequisites.removable_media_connected = Some(true);
        let issues = collect_issues(&checks);
        assert!(
            !issues.iter().any(|i| i.contains("removable-media")),
            "got: {issues:?}"
        );
    }

    #[test]
    fn test_collect_issues_removable_media_none_no_issue() {
        let mut checks = all_healthy();
        checks.prerequisites.removable_media_connected = None;
        let issues = collect_issues(&checks);
        assert!(
            !issues.iter().any(|i| i.contains("removable-media")),
            "got: {issues:?}"
        );
    }
}
