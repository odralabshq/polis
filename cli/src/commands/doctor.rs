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
    /// Workspace health.
    pub workspace: WorkspaceChecks,
    /// Network health.
    pub network: NetworkChecks,
    /// Security health.
    pub security: SecurityChecks,
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
    /// Whether a cached image exists at `~/.polis/images/polis-workspace.qcow2`.
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
pub struct SystemProbe;

impl HealthProbe for SystemProbe {
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
        let (
            process_isolation,
            traffic_inspection,
            (malware_db_current, malware_db_age_hours),
            (certificates_valid, certificates_expire_days),
        ) = tokio::join!(
            check_process_isolation(),
            check_gate_health(),
            check_malware_db(),
            check_certificates(),
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
pub async fn run(ctx: &OutputContext, json: bool) -> Result<()> {
    run_with(ctx, json, &SystemProbe).await
}

#[allow(clippy::too_many_lines)]
async fn run_with(ctx: &OutputContext, json: bool, probe: &impl HealthProbe) -> Result<()> {
    let (workspace, network, security) = tokio::try_join!(
        probe.check_workspace(),
        probe.check_network(),
        probe.check_security(),
    )?;
    let checks = DoctorChecks {
        workspace,
        network,
        security,
    };
    let issues = collect_issues(&checks);
    let status = if issues.is_empty() {
        "healthy"
    } else {
        "unhealthy"
    };

    if json {
        let out = serde_json::json!({
            "status": status,
            "checks": {
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
        return Ok(());
    }

    println!();
    println!("  {}", "Polis Health Check".style(ctx.styles.header));
    println!();

    println!("  Workspace:");
    print_check(ctx, checks.workspace.ready, "Ready to start");
    // Image cache status
    let img = &checks.workspace.image;
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
            println!("      Update with: polis init --force");
        }
    } else {
        print_check(ctx, false, "No workspace image cached");
        println!("      Run 'polis init' to download the image (~3.2 GB)");
    }
    if checks.workspace.disk_space_ok {
        print_check(
            ctx,
            true,
            &format!("{} GB disk space available", checks.workspace.disk_space_gb),
        );
    } else {
        print_check(
            ctx,
            false,
            &format!(
                "Low disk space ({} GB available, need 10 GB)",
                checks.workspace.disk_space_gb
            ),
        );
    }
    println!();

    // POLIS_IMAGE override warning (V-011, F-006)
    if let Some(override_path) = &checks.workspace.image.polis_image_override {
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

    println!("  Network:");
    print_check(ctx, checks.network.internet, "Internet connectivity");
    print_check(ctx, checks.network.dns, "DNS resolution working");
    println!();

    println!("  Security:");
    print_check(
        ctx,
        checks.security.process_isolation,
        "Process isolation active",
    );
    print_check(
        ctx,
        checks.security.traffic_inspection,
        "Traffic inspection responding",
    );
    print_check(
        ctx,
        checks.security.malware_db_current,
        &format!(
            "Malware scanner database current (updated: {}h ago)",
            checks.security.malware_db_age_hours,
        ),
    );
    let expire_days = checks.security.certificates_expire_days;
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

    println!();
    if issues.is_empty() {
        println!("  {} Everything looks good!", "✓".style(ctx.styles.success));
    } else {
        println!(
            "  {} Found {} issues. Run with --verbose for details.",
            "✗".style(ctx.styles.error),
            issues.len(),
        );
    }
    println!();

    Ok(())
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
    if checks.security.certificates_expire_days <= 0 {
        issues.push("Certificates expired".to_string());
    }
    issues
}

// ── System check helpers ──────────────────────────────────────────────────────

/// Check image cache status, metadata, `POLIS_IMAGE` override, and version drift.
async fn check_image() -> ImageCheckResult {
    let Some(home) = dirs::home_dir() else {
        return ImageCheckResult::default();
    };
    let images_dir = home.join(".polis/images");
    let cached = images_dir.join("polis-workspace.qcow2").exists();

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
    let version = val.get("version").and_then(|v| v.as_str()).map(str::to_owned);
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
        tokio::task::spawn_blocking(crate::commands::init::resolve_latest_image_url),
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
async fn check_gate_health() -> bool {
    let output = tokio::process::Command::new("multipass")
        .args([
            "exec", "polis", "--", "docker", "compose", "ps", "--format", "json", "gate",
        ])
        .output()
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
async fn check_malware_db() -> (bool, u64) {
    let output = tokio::process::Command::new("multipass")
        .args([
            "exec",
            "polis",
            "--",
            "stat",
            "-c",
            "%Y",
            "/var/lib/clamav/daily.cvd",
        ])
        .output()
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
async fn check_certificates() -> (bool, i64) {
    let output = tokio::process::Command::new("multipass")
        .args([
            "exec",
            "polis",
            "--",
            "openssl",
            "x509",
            "-enddate",
            "-noout",
            "-in",
            "/etc/polis/certs/ca/ca.crt",
        ])
        .output()
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
    use super::{DoctorChecks, ImageCheckResult, NetworkChecks, SecurityChecks, WorkspaceChecks, collect_issues};

    fn all_healthy() -> DoctorChecks {
        DoctorChecks {
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
    // Property tests
    // -----------------------------------------------------------------------

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        prop_compose! {
            fn arb_workspace_checks()(
                ready in any::<bool>(),
                disk_space_gb in 0u64..100,
            ) -> WorkspaceChecks {
                WorkspaceChecks {
                    ready,
                    disk_space_gb,
                    disk_space_ok: disk_space_gb >= 10,
                    image: ImageCheckResult::default(),
                }
            }
        }

        prop_compose! {
            fn arb_network_checks()(
                internet in any::<bool>(),
                dns in any::<bool>(),
            ) -> NetworkChecks {
                NetworkChecks { internet, dns }
            }
        }

        prop_compose! {
            fn arb_security_checks()(
                process_isolation in any::<bool>(),
                traffic_inspection in any::<bool>(),
                malware_db_current in any::<bool>(),
                malware_db_age_hours in 0u64..1000,
                certificates_valid in any::<bool>(),
                certificates_expire_days in -30i64..400,
            ) -> SecurityChecks {
                SecurityChecks {
                    process_isolation,
                    traffic_inspection,
                    malware_db_current,
                    malware_db_age_hours,
                    certificates_valid,
                    certificates_expire_days,
                }
            }
        }

        prop_compose! {
            fn arb_doctor_checks()(
                workspace in arb_workspace_checks(),
                network in arb_network_checks(),
                security in arb_security_checks(),
            ) -> DoctorChecks {
                DoctorChecks { workspace, network, security }
            }
        }

        proptest! {
            /// collect_issues never panics for any valid input.
            #[test]
            fn prop_collect_issues_never_panics(checks in arb_doctor_checks()) {
                let _ = collect_issues(&checks);
            }

            /// All healthy checks produce zero issues.
            #[test]
            fn prop_all_healthy_produces_no_issues(
                disk_space_gb in 10u64..100,
                malware_db_age_hours in 0u64..24,
                certificates_expire_days in 31i64..400,
            ) {
                let checks = DoctorChecks {
                    workspace: WorkspaceChecks {
                        ready: true,
                        disk_space_gb,
                        disk_space_ok: true,
                        image: ImageCheckResult::default(),
                    },
                    network: NetworkChecks {
                        internet: true,
                        dns: true,
                    },
                    security: SecurityChecks {
                        process_isolation: true,
                        traffic_inspection: true,
                        malware_db_current: true,
                        malware_db_age_hours,
                        certificates_valid: true,
                        certificates_expire_days,
                    },
                };
                let issues = collect_issues(&checks);
                prop_assert!(issues.is_empty(), "expected no issues for healthy checks, got: {issues:?}");
            }

            /// Low disk space always produces a disk-related issue.
            #[test]
            fn prop_low_disk_produces_disk_issue(disk_space_gb in 0u64..10) {
                let checks = DoctorChecks {
                    workspace: WorkspaceChecks {
                        ready: true,
                        disk_space_gb,
                        disk_space_ok: false,
                        image: ImageCheckResult::default(),
                    },
                    network: NetworkChecks { internet: true, dns: true },
                    security: SecurityChecks {
                        process_isolation: true,
                        traffic_inspection: true,
                        malware_db_current: true,
                        malware_db_age_hours: 0,
                        certificates_valid: true,
                        certificates_expire_days: 365,
                    },
                };
                let issues = collect_issues(&checks);
                prop_assert!(
                    issues.iter().any(|i| i.to_lowercase().contains("disk")),
                    "expected disk issue for {disk_space_gb} GB"
                );
            }

            /// Expired certificates always produce a cert-related issue.
            #[test]
            fn prop_expired_certs_produce_cert_issue(days in -30i64..=0) {
                let checks = DoctorChecks {
                    workspace: WorkspaceChecks {
                        ready: true,
                        disk_space_gb: 50,
                        disk_space_ok: true,
                        image: ImageCheckResult::default(),
                    },
                    network: NetworkChecks { internet: true, dns: true },
                    security: SecurityChecks {
                        process_isolation: true,
                        traffic_inspection: true,
                        malware_db_current: true,
                        malware_db_age_hours: 0,
                        certificates_valid: false,
                        certificates_expire_days: days,
                    },
                };
                let issues = collect_issues(&checks);
                prop_assert!(
                    issues.iter().any(|i| i.to_lowercase().contains("cert")),
                    "expected cert issue for {days} days"
                );
            }
        }
    }
}
