//! Health check domain types and pure diagnostic functions.
//!
//! This module is intentionally free of I/O, async, and external layer imports.
//! All functions take data in and return data out.

use serde::Serialize;

// ── Types ─────────────────────────────────────────────────────────────────────

/// All check categories returned by the doctor command.
#[derive(Debug, Default, Serialize)]
pub struct DiagnosticReport {
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
#[derive(Debug, Default, Serialize)]
pub struct PrerequisiteChecks {
    /// Whether `multipass` is on PATH.
    pub found: bool,
    /// Installed Multipass version string (e.g. `"1.16.1"`), if found.
    pub version: Option<String>,
    /// Whether the installed version meets the minimum (1.16.0).
    pub version_ok: bool,
}

/// Workspace health checks.
#[derive(Debug, Default, Serialize)]
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
    /// Value of `POLIS_IMAGE` env var if set (override active).
    pub polis_image_override: Option<String>,
}

/// Network health checks.
#[derive(Debug, Default, Serialize)]
pub struct NetworkChecks {
    /// Whether internet connectivity is available.
    pub internet: bool,
    /// Whether DNS resolution is working.
    pub dns: bool,
}

/// Result of malware database freshness check.
#[derive(Debug, Default, Serialize)]
pub struct MalwareDbStatus {
    /// Whether the malware scanner database is current.
    pub is_current: bool,
    /// Hours since the malware database was last updated.
    pub age_hours: u64,
}

/// Result of certificate validity check.
#[derive(Debug, Default, Serialize)]
pub struct CertificateStatus {
    /// Whether certificates are valid.
    pub is_valid: bool,
    /// Days until certificate expiry (≤ 0 means expired).
    pub expire_days: i64,
}

/// Security health checks.
#[derive(Debug, Default, Serialize)]
#[allow(clippy::struct_excessive_bools)] // fields are spec-mandated; a bitfield would obscure intent
pub struct SecurityChecks {
    /// Whether process isolation (sysbox) is active.
    pub process_isolation: bool,
    /// Whether traffic inspection is responding.
    pub traffic_inspection: bool,
    /// Malware database status.
    pub malware_db: MalwareDbStatus,
    /// Certificate validity status.
    pub certificates: CertificateStatus,
}

// ── Pure functions ────────────────────────────────────────────────────────────

/// Collect actionable issues from check results.
///
/// Returns a list of human-readable issue strings for any failing checks.
/// Certificates expiring in 1–30 days are a **warning only** and are NOT
/// included in the returned issues list.
#[must_use]
pub fn collect_issues(checks: &DiagnosticReport) -> Vec<String> {
    let mut issues = Vec::new();
    if !checks.prerequisites.found {
        issues.push("multipass is not installed".to_string());
    } else if !checks.prerequisites.version_ok {
        let ver = checks.prerequisites.version.as_deref().unwrap_or("unknown");
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
    if !checks.security.malware_db.is_current {
        issues.push(format!(
            "Malware scanner database stale (updated: {}h ago)",
            checks.security.malware_db.age_hours
        ));
    }
    if checks.security.certificates.expire_days <= 0 {
        issues.push("Certificates expired".to_string());
    }
    issues
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn all_healthy() -> DiagnosticReport {
        DiagnosticReport {
            prerequisites: PrerequisiteChecks {
                found: true,
                version: Some("1.16.1".to_string()),
                version_ok: true,
            },
            workspace: WorkspaceChecks {
                ready: true,
                disk_space_gb: 50,
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
                malware_db: MalwareDbStatus {
                    is_current: true,
                    age_hours: 2,
                },
                certificates: CertificateStatus {
                    is_valid: true,
                    expire_days: 90,
                },
            },
        }
    }

    #[test]
    fn test_collect_issues_all_healthy_returns_empty() {
        assert!(collect_issues(&all_healthy()).is_empty());
    }

    #[test]
    fn test_collect_issues_low_disk_returns_disk_issue() {
        let mut checks = all_healthy();
        checks.workspace.disk_space_gb = 5;
        checks.workspace.disk_space_ok = false;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Low disk space"));
        assert!(issues[0].contains("5 GB"));
    }

    #[test]
    fn test_collect_issues_dns_failed_returns_dns_issue() {
        let mut checks = all_healthy();
        checks.network.dns = false;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("DNS resolution failed"));
    }

    #[test]
    fn test_collect_issues_traffic_inspection_failed_returns_issue() {
        let mut checks = all_healthy();
        checks.security.traffic_inspection = false;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Traffic inspection not responding"));
    }

    #[test]
    fn test_collect_issues_expired_certs_returns_issue() {
        let mut checks = all_healthy();
        checks.security.certificates.expire_days = 0;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Certificates expired"));
    }

    #[test]
    fn test_collect_issues_expiring_soon_not_in_issues() {
        // Certs expiring in 1–30 days are a warning only, NOT an issue.
        let mut checks = all_healthy();
        checks.security.certificates.expire_days = 15;
        assert!(collect_issues(&checks).is_empty());
    }

    #[test]
    fn test_collect_issues_multiple_failures_all_collected() {
        let mut checks = all_healthy();
        checks.workspace.disk_space_gb = 3;
        checks.workspace.disk_space_ok = false;
        checks.network.dns = false;
        checks.security.traffic_inspection = false;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 3);
    }

    #[test]
    fn test_collect_issues_multipass_not_found_returns_issue() {
        let mut checks = all_healthy();
        checks.prerequisites.found = false;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("multipass is not installed"));
    }

    #[test]
    fn test_collect_issues_multipass_version_too_old_returns_issue() {
        let mut checks = all_healthy();
        checks.prerequisites.version = Some("1.14.0".to_string());
        checks.prerequisites.version_ok = false;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("too old"));
    }

    #[test]
    fn test_image_check_result_default_is_not_cached() {
        let result = ImageCheckResult::default();
        assert!(!result.cached);
        assert!(result.polis_image_override.is_none());
    }

    // ── Exit code logic tests (Property 6) ────────────────────────────────────

    /// **Validates: Requirements 15.1, 15.2**
    ///
    /// Property 6: Exit code reflects issue presence.
    /// When `collect_issues` returns empty, the command handler should return
    /// `ExitCode::SUCCESS`. This test verifies the healthy report → empty issues
    /// relationship.
    #[test]
    fn test_exit_code_logic_healthy_report_yields_empty_issues() {
        // A fully healthy report should produce no issues
        let healthy = all_healthy();
        let issues = collect_issues(&healthy);

        // Empty issues → ExitCode::SUCCESS in command handler
        assert!(
            issues.is_empty(),
            "Healthy report should yield empty issues (ExitCode::SUCCESS)"
        );
    }

    /// **Validates: Requirements 15.1, 15.2, 15.3, 15.4**
    ///
    /// Property 6: Exit code reflects issue presence.
    /// When `collect_issues` returns non-empty, the command handler should return
    /// `ExitCode::FAILURE`. This test verifies unhealthy report → non-empty issues
    /// relationship.
    #[test]
    fn test_exit_code_logic_unhealthy_report_yields_non_empty_issues() {
        // Test various failure conditions that should produce issues

        // Case 1: Multipass not found
        let mut checks = all_healthy();
        checks.prerequisites.found = false;
        let issues = collect_issues(&checks);
        assert!(
            !issues.is_empty(),
            "Missing multipass should yield non-empty issues (ExitCode::FAILURE)"
        );

        // Case 2: Version too old
        let mut checks = all_healthy();
        checks.prerequisites.version_ok = false;
        let issues = collect_issues(&checks);
        assert!(
            !issues.is_empty(),
            "Old multipass version should yield non-empty issues (ExitCode::FAILURE)"
        );

        // Case 3: Low disk space
        let mut checks = all_healthy();
        checks.workspace.disk_space_ok = false;
        let issues = collect_issues(&checks);
        assert!(
            !issues.is_empty(),
            "Low disk space should yield non-empty issues (ExitCode::FAILURE)"
        );

        // Case 4: DNS failure
        let mut checks = all_healthy();
        checks.network.dns = false;
        let issues = collect_issues(&checks);
        assert!(
            !issues.is_empty(),
            "DNS failure should yield non-empty issues (ExitCode::FAILURE)"
        );

        // Case 5: Traffic inspection failure
        let mut checks = all_healthy();
        checks.security.traffic_inspection = false;
        let issues = collect_issues(&checks);
        assert!(
            !issues.is_empty(),
            "Traffic inspection failure should yield non-empty issues (ExitCode::FAILURE)"
        );

        // Case 6: Stale malware database
        let mut checks = all_healthy();
        checks.security.malware_db.is_current = false;
        let issues = collect_issues(&checks);
        assert!(
            !issues.is_empty(),
            "Stale malware DB should yield non-empty issues (ExitCode::FAILURE)"
        );

        // Case 7: Expired certificates
        let mut checks = all_healthy();
        checks.security.certificates.expire_days = 0;
        let issues = collect_issues(&checks);
        assert!(
            !issues.is_empty(),
            "Expired certificates should yield non-empty issues (ExitCode::FAILURE)"
        );
    }

    /// **Validates: Requirements 15.1, 15.2**
    ///
    /// Property 6: Exit code reflects issue presence.
    /// Verifies that the default `DiagnosticReport` (all fields at default values)
    /// produces issues, since defaults represent an unhealthy state.
    #[test]
    fn test_exit_code_logic_default_report_yields_issues() {
        // Default report has found=false, version_ok=false, disk_space_ok=false, etc.
        let default_report = DiagnosticReport::default();
        let issues = collect_issues(&default_report);

        // Default state is unhealthy → should have issues → ExitCode::FAILURE
        assert!(
            !issues.is_empty(),
            "Default report should yield non-empty issues (ExitCode::FAILURE)"
        );
    }
}
