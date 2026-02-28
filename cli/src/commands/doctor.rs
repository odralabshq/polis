//! `polis doctor` — system health diagnostics.

use std::path::Path;

use anyhow::{Context, Result};
use owo_colors::OwoColorize;

use crate::app::AppContext;
use crate::output::OutputContext;

// ── Re-exports from domain::health (backward compatibility) ───────────────────

pub use crate::domain::health::{
    DoctorChecks, ImageCheckResult, NetworkChecks, PrerequisiteChecks, SecurityChecks,
    VersionDrift, WorkspaceChecks, collect_issues,
};

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
pub struct SystemProbe<
    'a,
    M: crate::application::ports::InstanceInspector + crate::application::ports::ShellExecutor,
> {
    mp: &'a M,
}

impl<'a, M: crate::application::ports::InstanceInspector + crate::application::ports::ShellExecutor>
    SystemProbe<'a, M>
{
    /// Create a new system probe with the given multipass implementation.
    pub fn new(mp: &'a M) -> Self {
        Self { mp }
    }
}

impl<M: crate::application::ports::InstanceInspector + crate::application::ports::ShellExecutor>
    HealthProbe for SystemProbe<'_, M>
{
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
        let vm_running = crate::application::services::vm::lifecycle::state(self.mp).await.ok() == Some(crate::application::services::vm::lifecycle::VmState::Running);

        if !vm_running {
            return Ok(SecurityChecks {
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
            check_process_isolation(self.mp),
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
    app: &AppContext,
    verbose: bool,
    fix: bool,
    mp: &(
         impl crate::application::ports::InstanceInspector
         + crate::application::ports::ShellExecutor
         + crate::application::ports::FileTransfer
     ),
) -> Result<()> {
    run_with(app, verbose, fix, &SystemProbe::new(mp), mp).await
}

/// Run doctor with a custom health probe (for testing).
///
/// # Errors
///
/// Returns an error if health checks cannot be executed or output fails.
pub async fn run_with(
    app: &AppContext,
    verbose: bool,
    fix: bool,
    probe: &impl HealthProbe,
    mp: &(
         impl crate::application::ports::InstanceInspector
         + crate::application::ports::ShellExecutor
         + crate::application::ports::FileTransfer
     ),
) -> Result<()> {
    let ctx = &app.output;
    let checks_result = tokio::try_join!(
        probe.check_prerequisites(),
        probe.check_workspace(),
        probe.check_network(),
        probe.check_security(),
    );

    match checks_result {
        Ok((prerequisites, workspace, network, security)) => {
            let checks = DoctorChecks {
                prerequisites,
                workspace,
                network,
                security,
            };
            let issues = collect_issues(&checks);

            app.renderer().render_doctor(&checks, &issues, verbose)?;

            if fix && !issues.is_empty() {
                repair(ctx, mp, false).await?;
            }
        }
        Err(e) => {
            if !ctx.quiet {
                println!(
                    "  {} Health checks failed: {e}. Attempting repair...",
                    "⚠".yellow()
                );
            }
            if fix {
                repair(ctx, mp, true).await?;
            } else {
                return Err(e);
            }
        }
    }

    Ok(())
}

// ── Issue collection ──────────────────────────────────────────────────────────

// ── System check helpers ──────────────────────────────────────────────────────

// ── Repair ───────────────────────────────────────────────────────────────────

/// Attempt to repair detected issues without destroying user data.
///
/// Repairs (in order):
/// 1. Docker not running → `systemctl restart docker`
/// 2. sysbox not registered → `systemctl restart sysbox && restart docker`
/// 3. `/opt/polis` missing or config stale → re-transfer config tarball
///    3.5. Certs missing or expiring → regenerate via `generate_certs_and_secrets`
/// 4. `polis.service` not enabled → `systemctl enable --now polis.service`
/// 5. Restart compose services (compose down first if certs were regenerated)
///
/// When `health_checks_failed` is true, config is always re-transferred from
/// host before any other repair steps (VM state is untrusted — prevents
/// tampered scripts from persisting and running as root during repair).
///
/// # Errors
///
/// Returns an error if the VM is not running or a repair step fails fatally.
#[allow(clippy::too_many_lines)]
pub async fn repair(
    ctx: &OutputContext,
    mp: &(
         impl crate::application::ports::InstanceInspector
         + crate::application::ports::ShellExecutor
         + crate::application::ports::FileTransfer
     ),
    health_checks_failed: bool,
) -> Result<()> {
    use owo_colors::OwoColorize;

    let print_step = |msg: &str| {
        if !ctx.quiet {
            println!("  {} {msg}", "→".cyan());
        }
    };
    let print_ok = |msg: &str| {
        if !ctx.quiet {
            println!("  {} {msg}", "✓".green());
        }
    };
    let print_skip = |msg: &str| {
        if !ctx.quiet {
            println!("  {} {msg}", "–".dimmed());
        }
    };

    if !ctx.quiet {
        println!("\n{}", "Repairing...".cyan());
    }

    // When health checks failed, VM state is untrusted — always re-transfer config
    // from host to prevent tampered scripts from persisting and running as root.
    if health_checks_failed {
        print_step("Re-transferring config (health checks failed, VM state untrusted)...");
        let (assets_dir, _guard) =
            crate::infra::assets::extract_assets().context("extracting embedded assets")?;
        let version = env!("CARGO_PKG_VERSION");
        crate::application::services::vm::provision::transfer_config(mp, &assets_dir, version)
            .await
            .context("re-transferring config to VM")?;
        print_ok("Config re-transferred");
    }

    // 1. Ensure Docker is running
    print_step("Checking Docker daemon...");
    let docker_ok = mp
        .exec(&["docker", "info"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if docker_ok {
        print_skip("Docker already running");
    } else {
        print_step("Restarting Docker...");
        mp.exec(&["sudo", "systemctl", "restart", "docker"])
            .await
            .context("restarting docker")?;
        print_ok("Docker restarted");
    }

    // 2. Ensure sysbox runtime is registered
    print_step("Checking sysbox runtime...");
    let sysbox_ok = mp
        .exec(&["docker", "info", "--format", "{{.Runtimes}}"])
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("sysbox-runc"))
        .unwrap_or(false);
    if sysbox_ok {
        print_skip("sysbox-runc already registered");
    } else {
        print_step("Restarting sysbox and Docker...");
        mp.exec(&["sudo", "systemctl", "restart", "sysbox"])
            .await
            .context("restarting sysbox")?;
        mp.exec(&["sudo", "systemctl", "restart", "docker"])
            .await
            .context("restarting docker after sysbox")?;
        print_ok("sysbox and Docker restarted");
    }

    // 3. Re-transfer config if /opt/polis is missing or .env is absent
    print_step("Checking /opt/polis config...");
    let config_ok = mp
        .exec(&["test", "-f", "/opt/polis/.env"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if config_ok {
        print_skip("/opt/polis config present");
    } else {
        print_step("Re-transferring config...");
        let (assets_dir, _guard) =
            crate::infra::assets::extract_assets().context("extracting embedded assets")?;
        let version = env!("CARGO_PKG_VERSION");
        crate::application::services::vm::provision::transfer_config(mp, &assets_dir, version)
            .await
            .context("re-transferring config to VM")?;
        print_ok("Config re-transferred");
    }

    // 3.5: Ensure certs exist and are not expiring within 7 days
    let mut certs_regenerated = false;
    print_step("Checking certificates...");
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
        print_skip("Certificates present and valid");
    } else {
        if sentinel_ok && !cert_expiry_ok {
            print_step("Certificates expiring soon, forcing regeneration...");
            mp.exec(&["rm", "-f", "/opt/polis/.certs-ready"]).await.ok();
        }
        print_step("Generating certificates and secrets...");
        crate::application::services::vm::provision::generate_certs_and_secrets(mp)
            .await
            .context("generating certificates during repair")?;
        certs_regenerated = true;
        print_ok("Certificates generated");
    }

    // 4. Ensure polis.service is enabled
    print_step("Checking polis.service...");
    let service_enabled = mp
        .exec(&["systemctl", "is-enabled", "polis.service"])
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(false);
    if service_enabled {
        print_skip("polis.service already enabled");
    } else {
        print_step("Enabling polis.service...");
        mp.exec(&["sudo", "systemctl", "enable", "polis.service"])
            .await
            .context("enabling polis.service")?;
        print_ok("polis.service enabled");
    }

    // 5. Restart compose services — use compose down when certs were regenerated
    // to ensure credential consistency (prevents rolling restart mismatch where
    // some containers read new secrets while Valkey still has old ones).
    if certs_regenerated {
        print_step("Stopping services for clean restart...");
        mp.exec(&["bash", "-c", "cd /opt/polis && docker compose down"])
            .await
            .context("stopping services before restart")?;
    }
    print_step("Restarting services...");
    mp.exec(&[
        "bash",
        "-c",
        "cd /opt/polis && docker compose --env-file .env up -d --remove-orphans",
    ])
    .await
    .context("restarting compose services")?;
    print_ok("Services restarted");

    if !ctx.quiet {
        println!(
            "\n{}",
            "Repair complete. Run 'polis doctor' to verify.".green()
        );
    }
    Ok(())
}

/// Blocking probe for prerequisite checks (runs in `spawn_blocking`).
fn probe_prerequisites() -> PrerequisiteChecks {
    use std::process::Command;

    let output = Command::new("multipass").arg("version").output();
    let Ok(output) = output else {
        return PrerequisiteChecks {
            multipass_found: false,
            multipass_version: None,
            multipass_version_ok: false,
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
    // Not needed — assets are extracted to ~/polis/tmp/ which is accessible
    // via the snap home interface.

    PrerequisiteChecks {
        multipass_found: true,
        multipass_version: version_str,
        multipass_version_ok: version_ok,
    }
}

/// Check image cache status, metadata, `POLIS_IMAGE` override, and version drift.
async fn check_image() -> ImageCheckResult {
    let Ok(images_dir) = crate::infra::fs::images_dir() else {
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
        tokio::task::spawn_blocking(crate::infra::image::resolve_latest_image_url),
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
    #[cfg(windows)]
    {
        let out = tokio::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "((Get-PSDrive C).Free / 1GB) -as [int]",
            ])
            .output()
            .await
            .context("powershell failed")?;
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u64>()
            .context("cannot parse disk space from powershell")
    }
    #[cfg(not(windows))]
    {
        // `df -k` is POSIX and works on both Linux and macOS.
        // Column 3 (0-indexed) is "Available" in 1 KiB blocks.
        let out = tokio::process::Command::new("df")
            .args(["-k", "/"])
            .output()
            .await
            .context("df failed")?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .nth(1)
            .and_then(|l| l.split_whitespace().nth(3))
            .and_then(|s| s.parse::<u64>().ok())
            .map(|kb| kb / (1024 * 1024))
            .ok_or_else(|| anyhow::anyhow!("cannot parse df output"))
    }
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

/// Check if sysbox-runc is available inside the multipass VM.
async fn check_process_isolation(mp: &impl crate::application::ports::ShellExecutor) -> bool {
    let output = mp.exec(&["sysbox-runc", "--version"]).await;
    output.map(|o| o.status.success()).unwrap_or(false)
}

/// Check if the gate container is running inside the multipass VM.
async fn check_gate_health(mp: &impl crate::application::ports::ShellExecutor) -> bool {
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

/// Check `ClamAV` database freshness inside the multipass VM.
async fn check_malware_db(mp: &impl crate::application::ports::ShellExecutor) -> (bool, u64) {
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
async fn check_certificates(mp: &impl crate::application::ports::ShellExecutor) -> (bool, i64) {
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
}

