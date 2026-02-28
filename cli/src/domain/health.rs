//! Health check domain types and pure diagnostic functions.
//!
//! This module is intentionally free of I/O, async, and external layer imports.
//! All functions take data in and return data out.

use serde::Serialize;

// ── Types ─────────────────────────────────────────────────────────────────────

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
#[allow(clippy::struct_field_names)]
pub struct PrerequisiteChecks {
    /// Whether `multipass` is on PATH.
    pub multipass_found: bool,
    /// Installed Multipass version string (e.g. `"1.16.1"`), if found.
    pub multipass_version: Option<String>,
    /// Whether the installed version meets the minimum (1.16.0).
    pub multipass_version_ok: bool,
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

// ── Pure functions ────────────────────────────────────────────────────────────

/// Collect actionable issues from check results.
///
/// Returns a list of human-readable issue strings for any failing checks.
/// Certificates expiring in 1–30 days are a **warning only** and are NOT
/// included in the returned issues list.
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

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn all_healthy() -> DoctorChecks {
        DoctorChecks {
            prerequisites: PrerequisiteChecks {
                multipass_found: true,
                multipass_version: Some("1.16.1".to_string()),
                multipass_version_ok: true,
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
                malware_db_current: true,
                malware_db_age_hours: 2,
                certificates_valid: true,
                certificates_expire_days: 90,
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
        checks.security.certificates_expire_days = 0;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Certificates expired"));
    }

    #[test]
    fn test_collect_issues_expiring_soon_not_in_issues() {
        // Certs expiring in 1–30 days are a warning only, NOT an issue.
        let mut checks = all_healthy();
        checks.security.certificates_expire_days = 15;
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
        checks.prerequisites.multipass_found = false;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("multipass is not installed"));
    }

    #[test]
    fn test_collect_issues_multipass_version_too_old_returns_issue() {
        let mut checks = all_healthy();
        checks.prerequisites.multipass_version = Some("1.14.0".to_string());
        checks.prerequisites.multipass_version_ok = false;
        let issues = collect_issues(&checks);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("too old"));
    }

    #[test]
    fn test_image_check_result_default_is_not_cached() {
        let result = ImageCheckResult::default();
        assert!(!result.cached);
        assert!(result.version.is_none());
        assert!(result.sha256_preview.is_none());
        assert!(result.polis_image_override.is_none());
        assert!(result.version_drift.is_none());
    }

    #[test]
    fn test_version_drift_fields_accessible() {
        let drift = VersionDrift {
            current: "1.0.0".to_string(),
            latest: "1.1.0".to_string(),
        };
        assert_eq!(drift.current, "1.0.0");
        assert_eq!(drift.latest, "1.1.0");
    }
}
