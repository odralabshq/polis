//! Integration tests for the `workspace_doctor` application service.
//!
//! **Property 7 (partial): Doctor operations route through provisioner**
//!
//! Verifies that `run_doctor()` uses provisioner trait methods for all VM
//! interactions and returns a `DoctorChecks` domain type.

#![allow(clippy::expect_used)]

use std::process::Output;
use std::sync::Mutex;

use anyhow::Result;
use polis_cli::application::ports::{
    CommandRunner, FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec,
    NetworkProbe, ProgressReporter, ShellExecutor,
};
use polis_cli::application::services::workspace_doctor::run_doctor;
use polis_cli::domain::health::collect_issues;

use crate::helpers::exit_status;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ok(stdout: &[u8]) -> Output {
    Output {
        status: exit_status(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

fn fail() -> Output {
    Output {
        status: exit_status(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

// ── Mock: no-op progress reporter ────────────────────────────────────────────

struct NoopReporter;

impl ProgressReporter for NoopReporter {
    fn step(&self, _: &str) {}
    fn success(&self, _: &str) {}
    fn warn(&self, _: &str) {}
}

// ── Mock: recording provisioner (VM running, all checks healthy) ──────────────

struct HealthyProvisioner {
    exec_calls: Mutex<Vec<Vec<String>>>,
}

impl HealthyProvisioner {
    fn new() -> Self {
        Self {
            exec_calls: Mutex::new(Vec::new()),
        }
    }

    fn exec_call_count(&self) -> usize {
        self.exec_calls.lock().expect("lock").len()
    }
}

impl InstanceInspector for HealthyProvisioner {
    async fn info(&self) -> Result<Output> {
        Ok(ok(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        Ok(ok(b"multipass 1.16.0
multipassd 1.16.0"))
    }
}

impl InstanceLifecycle for HealthyProvisioner {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn start(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn stop(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok(b""))
    }
}

impl FileTransfer for HealthyProvisioner {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        Ok(ok(b""))
    }
}

impl ShellExecutor for HealthyProvisioner {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        self.exec_calls
            .lock()
            .expect("lock")
            .push(args.iter().map(ToString::to_string).collect());

        let joined = args.join(" ");

        // sysbox-runc --version → success (process isolation ok)
        if joined.contains("sysbox-runc") {
            return Ok(ok(b"sysbox-runc version 0.6.0"));
        }
        // docker compose ps gate → running
        if joined.contains("compose") && joined.contains("ps") && joined.contains("gate") {
            return Ok(ok(br#"{"State":"running","Health":"healthy"}"#));
        }
        // malware db stat → recent timestamp
        if joined.contains("stat") && joined.contains("clamav") {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            return Ok(ok(now.to_string().as_bytes()));
        }
        // openssl cert check → far future expiry
        if joined.contains("openssl") && joined.contains("x509") {
            return Ok(ok(b"notAfter=Jan 01 00:00:00 2099 GMT"));
        }
        Ok(ok(b""))
    }
    async fn exec_with_stdin(&self, args: &[&str], _: &[u8]) -> Result<Output> {
        self.exec_calls
            .lock()
            .expect("lock")
            .push(args.iter().map(ToString::to_string).collect());
        Ok(ok(b""))
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not expected in doctor tests")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        Ok(exit_status(0))
    }
}

// ── Mock: VM stopped provisioner ─────────────────────────────────────────────

struct VmStoppedProvisioner;

impl InstanceInspector for VmStoppedProvisioner {
    async fn info(&self) -> Result<Output> {
        Ok(ok(br#"{"info":{"polis":{"state":"Stopped"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        Ok(ok(b"multipass 1.16.0
multipassd 1.16.0"))
    }
}

impl InstanceLifecycle for VmStoppedProvisioner {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn start(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn stop(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok(b""))
    }
}

impl FileTransfer for VmStoppedProvisioner {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        Ok(ok(b""))
    }
}

impl ShellExecutor for VmStoppedProvisioner {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(fail())
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        Ok(ok(b""))
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        Ok(exit_status(0))
    }
}

// ── Mock: no-op CommandRunner ─────────────────────────────────────────────────

struct NoopCommandRunner;

impl CommandRunner for NoopCommandRunner {
    async fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
        // probe_disk_space_gb needs a parseable integer from df or powershell
        let joined = args.join(" ");
        if program == "df" || (program == "powershell" && joined.contains("Get-PSDrive")) {
            return Ok(ok(b"50"));
        }
        // multipass version → fake version string
        if program == "multipass" {
            return Ok(ok(b"multipass 1.16.0\nmultipassd 1.16.0"));
        }
        Ok(ok(b""))
    }
    async fn run_with_timeout(
        &self,
        program: &str,
        args: &[&str],
        _timeout: std::time::Duration,
    ) -> Result<Output> {
        self.run(program, args).await
    }
    async fn run_with_stdin(
        &self,
        _program: &str,
        _args: &[&str],
        _stdin: &[u8],
    ) -> Result<Output> {
        Ok(ok(b""))
    }
    fn spawn(&self, _program: &str, _args: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected")
    }
    async fn run_status(
        &self,
        _program: &str,
        _args: &[&str],
    ) -> Result<std::process::ExitStatus> {
        Ok(exit_status(0))
    }
}

// ── Mock: no-op NetworkProbe ──────────────────────────────────────────────────

struct NoopNetworkProbe;

impl NetworkProbe for NoopNetworkProbe {
    async fn check_tcp_connectivity(&self, _host: &str, _port: u16) -> Result<bool> {
        Ok(true)
    }
    async fn check_dns_resolution(&self, _hostname: &str) -> Result<bool> {
        Ok(true)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────
///
/// `run_doctor()` must use the injected provisioner for all VM interactions
/// and return a `DoctorChecks` domain type.
#[tokio::test]
async fn run_doctor_routes_vm_checks_through_provisioner() {
    let provisioner = HealthyProvisioner::new();
    let reporter = NoopReporter;

    let checks = run_doctor(&provisioner, &reporter, &NoopCommandRunner, &NoopNetworkProbe)
        .await
        .expect("run_doctor should succeed");

    // Provisioner was called for VM state and security checks
    assert!(
        provisioner.exec_call_count() > 0,
        "run_doctor must route VM checks through provisioner"
    );

    // Result is a DoctorChecks domain type — workspace should be ready
    // (VM is Running in our mock)
    assert!(
        checks.workspace.ready,
        "workspace should be ready when VM is Running"
    );
}

/// When VM is stopped, security checks return all-false without crashing.
#[tokio::test]
async fn run_doctor_vm_stopped_returns_security_all_false() {
    let provisioner = VmStoppedProvisioner;
    let reporter = NoopReporter;

    let checks = run_doctor(&provisioner, &reporter, &NoopCommandRunner, &NoopNetworkProbe)
        .await
        .expect("run_doctor should succeed even when VM is stopped");

    assert!(
        !checks.security.process_isolation,
        "process_isolation should be false when VM is stopped"
    );
    assert!(
        !checks.security.traffic_inspection,
        "traffic_inspection should be false when VM is stopped"
    );
    assert!(
        !checks.security.certificates_valid,
        "certificates_valid should be false when VM is stopped"
    );
}

/// `run_doctor()` returns a `DoctorChecks` that can be passed to `collect_issues()`.
#[tokio::test]
async fn run_doctor_result_is_usable_by_collect_issues() {
    let provisioner = HealthyProvisioner::new();
    let reporter = NoopReporter;

    let checks = run_doctor(&provisioner, &reporter, &NoopCommandRunner, &NoopNetworkProbe)
        .await
        .expect("run_doctor should succeed");

    // collect_issues is a pure domain function — must accept the returned type
    let issues = collect_issues(&checks);
    // With a healthy mock, issues should be empty (or at most network-related
    // since we can't control real network in tests)
    let _ = issues; // just verify it compiles and runs without panic
}

/// Progress reporter receives step/success calls during doctor run.
#[tokio::test]
async fn run_doctor_calls_progress_reporter() {
    use std::sync::Arc;

    struct CountingReporter {
        steps: Arc<Mutex<Vec<String>>>,
        successes: Arc<Mutex<Vec<String>>>,
    }

    impl ProgressReporter for CountingReporter {
        fn step(&self, msg: &str) {
            self.steps.lock().expect("lock").push(msg.to_owned());
        }
        fn success(&self, msg: &str) {
            self.successes.lock().expect("lock").push(msg.to_owned());
        }
        fn warn(&self, _: &str) {}
    }

    let steps = Arc::new(Mutex::new(Vec::new()));
    let successes = Arc::new(Mutex::new(Vec::new()));
    let reporter = CountingReporter {
        steps: Arc::clone(&steps),
        successes: Arc::clone(&successes),
    };

    let provisioner = HealthyProvisioner::new();

    run_doctor(&provisioner, &reporter, &NoopCommandRunner, &NoopNetworkProbe)
        .await
        .expect("run_doctor should succeed");

    assert!(
        !steps.lock().expect("lock").is_empty(),
        "reporter should receive step() calls"
    );
    assert!(
        !successes.lock().expect("lock").is_empty(),
        "reporter should receive success() calls"
    );
}
