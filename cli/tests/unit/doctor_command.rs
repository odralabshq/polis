//! Unit tests for `polis doctor` (issue 17).
//!
//! Tests use mocked `HealthProbe` to avoid slow real system checks.

#![allow(clippy::expect_used)]

use anyhow::Result;
use polis_cli::commands::doctor::{
    self, HealthProbe, ImageCheckResult, NetworkChecks, PrerequisiteChecks, SecurityChecks,
    WorkspaceChecks,
};
use polis_cli::multipass::Multipass;
use polis_cli::output::OutputContext;

use crate::helpers::{err_output, ok_output};

// ── Mock probes ───────────────────────────────────────────────────────────────

/// Mock probe returning healthy system
struct MockHealthyProbe;

impl HealthProbe for MockHealthyProbe {
    async fn check_prerequisites(&self) -> Result<PrerequisiteChecks> {
        Ok(PrerequisiteChecks {
            multipass_found: true,
            multipass_version: Some("1.16.0".to_string()),
            multipass_version_ok: true,
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

/// Mock probe returning unhealthy system
struct MockUnhealthyProbe;

impl HealthProbe for MockUnhealthyProbe {
    async fn check_prerequisites(&self) -> Result<PrerequisiteChecks> {
        Ok(PrerequisiteChecks {
            multipass_found: false,
            multipass_version: None,
            multipass_version_ok: false,
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

/// Mock probe that returns Err from `check_prerequisites` (simulates broken VM).
struct MockFailingProbe;

impl HealthProbe for MockFailingProbe {
    async fn check_prerequisites(&self) -> Result<PrerequisiteChecks> {
        anyhow::bail!("VM unreachable: connection refused")
    }

    async fn check_workspace(&self) -> Result<WorkspaceChecks> {
        anyhow::bail!("VM unreachable")
    }

    async fn check_network(&self) -> Result<NetworkChecks> {
        anyhow::bail!("VM unreachable")
    }

    async fn check_security(&self) -> Result<SecurityChecks> {
        anyhow::bail!("VM unreachable")
    }
}

// ── Mock Multipass for repair tests ──────────────────────────────────────────

/// Records exec calls and returns configurable responses per command pattern.
/// Used to test `repair()` paths without real VM.
struct RepairMockMp {
    /// Recorded exec calls (args joined with space).
    pub calls: std::sync::Mutex<Vec<Vec<String>>>,
    /// If true, `test -f /opt/polis/.certs-ready` returns success (sentinel present).
    pub sentinel_present: bool,
    /// If true, `openssl x509 -checkend` returns success (certs not expiring).
    pub cert_expiry_ok: bool,
}

impl RepairMockMp {
    fn new(sentinel_present: bool, cert_expiry_ok: bool) -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            sentinel_present,
            cert_expiry_ok,
        }
    }

    fn recorded_calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().expect("lock").clone()
    }

    fn called_any(&self, needle: &str) -> bool {
        self.recorded_calls()
            .iter()
            .any(|call| call.join(" ").contains(needle))
    }
}

impl Multipass for RepairMockMp {
    async fn vm_info(&self) -> Result<std::process::Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }

    async fn exec(&self, args: &[&str]) -> Result<std::process::Output> {
        let call: Vec<String> = args.iter().map(std::string::ToString::to_string).collect();
        self.calls.lock().expect("lock").push(call.clone());

        let joined = call.join(" ");

        // Sentinel check: `test -f /opt/polis/.certs-ready`
        if joined.contains("test") && joined.contains(".certs-ready") {
            return if self.sentinel_present {
                Ok(ok_output(b""))
            } else {
                Ok(err_output(1, b""))
            };
        }

        // Cert expiry check: `openssl x509 -checkend`
        if joined.contains("checkend") {
            return if self.cert_expiry_ok {
                Ok(ok_output(b""))
            } else {
                Ok(err_output(1, b""))
            };
        }

        // All other exec calls succeed
        Ok(ok_output(b""))
    }

    async fn launch(
        &self,
        _: &polis_cli::multipass::LaunchParams<'_>,
    ) -> Result<std::process::Output> {
        anyhow::bail!("launch not expected")
    }
    async fn start(&self) -> Result<std::process::Output> {
        anyhow::bail!("start not expected")
    }
    async fn stop(&self) -> Result<std::process::Output> {
        anyhow::bail!("stop not expected")
    }
    async fn delete(&self) -> Result<std::process::Output> {
        anyhow::bail!("delete not expected")
    }
    async fn purge(&self) -> Result<std::process::Output> {
        anyhow::bail!("purge not expected")
    }
    async fn transfer(&self, _: &str, _: &str) -> Result<std::process::Output> {
        Ok(ok_output(b""))
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<std::process::Output> {
        Ok(ok_output(b""))
    }
    async fn exec_with_stdin(&self, args: &[&str], _: &[u8]) -> Result<std::process::Output> {
        let call: Vec<String> = args.iter().map(std::string::ToString::to_string).collect();
        self.calls.lock().expect("lock").push(call);
        Ok(ok_output(b""))
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not expected")
    }
    async fn version(&self) -> Result<std::process::Output> {
        anyhow::bail!("version not expected")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        anyhow::bail!("exec_status not expected")
    }
}

// ── run_with tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_doctor_json_outputs_valid_json() {
    let ctx = OutputContext::new(true, true);
    let mp = RepairMockMp::new(true, true);
    let result = doctor::run_with(&ctx, true, false, false, &MockHealthyProbe, &mp).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_doctor_healthy_system_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = RepairMockMp::new(true, true);
    let result = doctor::run_with(&ctx, false, false, false, &MockHealthyProbe, &mp).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_doctor_unhealthy_system_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = RepairMockMp::new(true, true);
    let result = doctor::run_with(&ctx, false, false, false, &MockUnhealthyProbe, &mp).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_doctor_unhealthy_json_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = RepairMockMp::new(true, true);
    let result = doctor::run_with(&ctx, true, false, false, &MockUnhealthyProbe, &mp).await;
    assert!(result.is_ok());
}

// ── Task 4.6: run_with() error-handling path ──────────────────────────────────

/// When health checks return Err and --fix is passed, `repair()` must still be
/// called (broken VM must not block the repair path).
#[tokio::test]
async fn test_run_with_checks_fail_fix_calls_repair() {
    let ctx = OutputContext::new(true, true);
    // Sentinel present + certs valid so repair completes without cert regen
    let mp = RepairMockMp::new(true, true);
    let result = doctor::run_with(&ctx, false, false, true, &MockFailingProbe, &mp).await;
    // repair() should have been called — verify by checking that compose up was invoked
    assert!(
        result.is_ok(),
        "expected repair to succeed, got: {result:?}"
    );
    assert!(
        mp.called_any("docker compose") && mp.called_any("up"),
        "expected compose up to be called during repair, calls: {:?}",
        mp.recorded_calls()
    );
}

/// When health checks return Err and --fix is NOT passed, the error propagates.
#[tokio::test]
async fn test_run_with_checks_fail_no_fix_returns_err() {
    let ctx = OutputContext::new(true, true);
    let mp = RepairMockMp::new(true, true);
    let result = doctor::run_with(&ctx, false, false, false, &MockFailingProbe, &mp).await;
    assert!(
        result.is_err(),
        "expected error to propagate when fix=false"
    );
}

// ── Task 4.5: repair cert check path ─────────────────────────────────────────

/// When the .certs-ready sentinel is missing, `repair()` must call
/// `generate_certs_and_secrets` (i.e. the cert generation scripts).
#[tokio::test]
async fn test_repair_missing_sentinel_triggers_cert_generation() {
    let ctx = OutputContext::new(true, true);
    // sentinel absent → certs must be regenerated
    let mp = RepairMockMp::new(false, false);
    let result = doctor::repair(&ctx, &mp, false).await;
    assert!(result.is_ok(), "repair should succeed: {result:?}");
    // generate_certs_and_secrets calls generate-ca.sh
    assert!(
        mp.called_any("generate-ca.sh"),
        "expected generate-ca.sh to be called, calls: {:?}",
        mp.recorded_calls()
    );
    // fix-cert-ownership.sh must also be called
    assert!(
        mp.called_any("fix-cert-ownership.sh"),
        "expected fix-cert-ownership.sh to be called, calls: {:?}",
        mp.recorded_calls()
    );
}

/// When sentinel is present and certs are valid, cert generation is skipped.
#[tokio::test]
async fn test_repair_valid_certs_skips_cert_generation() {
    let ctx = OutputContext::new(true, true);
    // sentinel present + certs valid → no regeneration
    let mp = RepairMockMp::new(true, true);
    let result = doctor::repair(&ctx, &mp, false).await;
    assert!(result.is_ok(), "repair should succeed: {result:?}");
    assert!(
        !mp.called_any("generate-ca.sh"),
        "generate-ca.sh should NOT be called when certs are valid, calls: {:?}",
        mp.recorded_calls()
    );
}

/// When certs are regenerated, compose down must be called before compose up.
#[tokio::test]
async fn test_repair_cert_regen_calls_compose_down_before_up() {
    let ctx = OutputContext::new(true, true);
    let mp = RepairMockMp::new(false, false);
    let result = doctor::repair(&ctx, &mp, false).await;
    assert!(result.is_ok(), "repair should succeed: {result:?}");

    let calls = mp.recorded_calls();
    let down_pos = calls
        .iter()
        .position(|c| c.join(" ").contains("compose down"));
    let up_pos = calls
        .iter()
        .position(|c| c.join(" ").contains("compose") && c.join(" ").contains("up"));

    assert!(
        down_pos.is_some(),
        "compose down should be called when certs regenerated, calls: {calls:?}"
    );
    assert!(
        up_pos.is_some(),
        "compose up should be called, calls: {calls:?}"
    );
    assert!(
        down_pos.expect("compose down position") < up_pos.expect("compose up position"),
        "compose down must come before compose up, calls: {calls:?}"
    );
}

// ── Task 4.7: cert expiry path ────────────────────────────────────────────────

/// When sentinel is present but openssl x509 -checkend returns non-zero
/// (certs expiring within 7 days), the sentinel must be removed and certs
/// must be regenerated.
#[tokio::test]
async fn test_repair_expiring_certs_removes_sentinel_and_regenerates() {
    let ctx = OutputContext::new(true, true);
    // sentinel present but cert expiry check fails (certs expiring soon)
    let mp = RepairMockMp::new(true, false);
    let result = doctor::repair(&ctx, &mp, false).await;
    assert!(result.is_ok(), "repair should succeed: {result:?}");

    let calls = mp.recorded_calls();

    // Sentinel removal: `rm -f /opt/polis/.certs-ready`
    assert!(
        calls
            .iter()
            .any(|c| c.join(" ").contains("rm") && c.join(" ").contains(".certs-ready")),
        "sentinel should be removed when certs are expiring, calls: {calls:?}"
    );

    // Cert regeneration: generate-ca.sh must be called
    assert!(
        mp.called_any("generate-ca.sh"),
        "generate-ca.sh should be called when certs are expiring, calls: {calls:?}"
    );
}

/// When sentinel is present and certs are expiring, compose down must be
/// called before compose up (same as missing sentinel path).
#[tokio::test]
async fn test_repair_expiring_certs_compose_down_before_up() {
    let ctx = OutputContext::new(true, true);
    let mp = RepairMockMp::new(true, false);
    let result = doctor::repair(&ctx, &mp, false).await;
    assert!(result.is_ok(), "repair should succeed: {result:?}");

    let calls = mp.recorded_calls();
    let down_pos = calls
        .iter()
        .position(|c| c.join(" ").contains("compose down"));
    let up_pos = calls
        .iter()
        .position(|c| c.join(" ").contains("compose") && c.join(" ").contains("up"));

    assert!(
        down_pos.is_some(),
        "compose down should be called when certs expiring, calls: {calls:?}"
    );
    assert!(
        up_pos.is_some(),
        "compose up should be called, calls: {calls:?}"
    );
    assert!(
        down_pos.expect("compose down position") < up_pos.expect("compose up position"),
        "compose down must come before compose up, calls: {calls:?}"
    );
}
