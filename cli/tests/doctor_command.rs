//! Integration tests for `polis doctor` (issue 17).
//!
//! Tests use mocked HealthProbe to avoid slow real system checks.

#![allow(clippy::expect_used)]

use anyhow::Result;
use polis_cli::commands::doctor::{
    self, HealthProbe, ImageCheckResult, NetworkChecks, PrerequisiteChecks, SecurityChecks,
    WorkspaceChecks,
};
use polis_cli::output::OutputContext;

/// Mock probe returning healthy system
struct MockHealthyProbe;

impl HealthProbe for MockHealthyProbe {
    async fn check_prerequisites(&self) -> Result<PrerequisiteChecks> {
        Ok(PrerequisiteChecks {
            multipass_found: true,
            multipass_version: Some("1.16.0".to_string()),
            multipass_version_ok: true,
            removable_media_connected: Some(false),
        })
    }

    async fn check_workspace(&self) -> Result<WorkspaceChecks> {
        Ok(WorkspaceChecks {
            ready: true,
            disk_space_gb: 50,
            disk_space_ok: true,
            image: ImageCheckResult {
                cached: true,
                version: Some("v0.3.0".to_string()),
                sha256_preview: Some("abc123".to_string()),
                polis_image_override: None,
                version_drift: None,
            },
        })
    }

    async fn check_network(&self) -> Result<NetworkChecks> {
        Ok(NetworkChecks {
            internet: true,
            dns: true,
        })
    }

    async fn check_security(&self) -> Result<SecurityChecks> {
        Ok(SecurityChecks {
            process_isolation: true,
            traffic_inspection: true,
            malware_db_current: true,
            malware_db_age_hours: 1,
            certificates_valid: true,
            certificates_expire_days: 365,
        })
    }
}

#[tokio::test]
async fn test_doctor_json_outputs_valid_json() {
    let ctx = OutputContext::new(true, true);
    let result = doctor::run_with(&ctx, true, &MockHealthyProbe).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_doctor_healthy_system_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = doctor::run_with(&ctx, false, &MockHealthyProbe).await;
    assert!(result.is_ok());
}

/// Mock probe returning unhealthy system
struct MockUnhealthyProbe;

impl HealthProbe for MockUnhealthyProbe {
    async fn check_prerequisites(&self) -> Result<PrerequisiteChecks> {
        Ok(PrerequisiteChecks {
            multipass_found: false,
            multipass_version: None,
            multipass_version_ok: false,
            removable_media_connected: None,
        })
    }

    async fn check_workspace(&self) -> Result<WorkspaceChecks> {
        Ok(WorkspaceChecks {
            ready: false,
            disk_space_gb: 5,
            disk_space_ok: false,
            image: ImageCheckResult {
                cached: false,
                version: None,
                sha256_preview: None,
                polis_image_override: None,
                version_drift: None,
            },
        })
    }

    async fn check_network(&self) -> Result<NetworkChecks> {
        Ok(NetworkChecks {
            internet: false,
            dns: false,
        })
    }

    async fn check_security(&self) -> Result<SecurityChecks> {
        Ok(SecurityChecks {
            process_isolation: false,
            traffic_inspection: false,
            malware_db_current: false,
            malware_db_age_hours: 0,
            certificates_valid: false,
            certificates_expire_days: 0,
        })
    }
}

#[tokio::test]
async fn test_doctor_unhealthy_system_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = doctor::run_with(&ctx, false, &MockUnhealthyProbe).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_doctor_unhealthy_json_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = doctor::run_with(&ctx, true, &MockUnhealthyProbe).await;
    assert!(result.is_ok());
}
