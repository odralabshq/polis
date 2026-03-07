//! Application service — workspace doctor use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::Result;
use tokio::time::{Duration, timeout};

/// Default timeout for individual probe execution.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

use crate::application::ports::{
    CommandRunner, FileTransfer, InstanceInspector, LocalPaths, NetworkProbe, ProgressReporter,
    ShellExecutor,
};
use crate::application::vm::lifecycle::{self as vm, VmState};
use crate::domain::health::{CertificateStatus, DiagnosticReport, MalwareDbStatus};
use crate::domain::workspace::QUERY_SCRIPT;

/// Run the doctor probe/diagnose workflow.
///
/// Accepts port trait bounds so the caller can inject real or mock
/// implementations. The service never touches `OutputContext` or any
/// presentation type — rendering is the caller's responsibility.
///
/// `polis_image_override` is the value of the `POLIS_IMAGE` env var, read by
/// the presentation layer and passed in here (Req 10.5, 14.4).
///
/// # Errors
///
/// Returns an error if any health probe fails to execute.
pub async fn diagnose(
    provisioner: &(impl InstanceInspector + ShellExecutor + FileTransfer),
    reporter: &impl ProgressReporter,
    cmd_runner: &impl CommandRunner,
    network_probe: &impl NetworkProbe,
    paths: &(impl LocalPaths + crate::application::ports::LocalFs),
    polis_image_override: Option<String>,
    network_targets: &NetworkTargets,
) -> Result<DiagnosticReport> {
    // Resolve VM state exactly once (Req 1.1, 1.2, 1.3)
    let vm_state = vm::state(provisioner).await.unwrap_or(VmState::NotFound);
    let vm_running = vm_state == VmState::Running;

    reporter.step("checking prerequisites...");
    let prerequisites = run_probe("prerequisites", probe_prerequisites(cmd_runner), reporter).await;

    reporter.step("checking workspace...");
    let workspace = run_probe(
        "workspace",
        probe_workspace(cmd_runner, paths, polis_image_override, vm_running),
        reporter,
    )
    .await;

    reporter.step("checking network...");
    // probe_network is infallible (returns NetworkChecks directly, not Result),
    // so use a simpler if let timeout check instead of run_probe
    let network = if let Ok(n) =
        timeout(PROBE_TIMEOUT, probe_network(network_probe, network_targets)).await
    {
        n
    } else {
        reporter.warn(&format!(
            "network probe timed out after {}s",
            PROBE_TIMEOUT.as_secs()
        ));
        crate::domain::health::NetworkChecks::default()
    };

    reporter.step("checking security...");
    let security = run_probe(
        "security",
        probe_security(provisioner, vm_running),
        reporter,
    )
    .await;

    reporter.success("diagnostics complete");

    Ok(DiagnosticReport {
        prerequisites,
        workspace,
        network,
        security,
    })
}
/// Configurable network check targets.
///
/// Allows operators in restricted network environments to specify custom
/// targets for TCP connectivity and DNS resolution checks.
#[derive(Debug, Clone)]
pub struct NetworkTargets {
    /// Host for TCP connectivity check (default: "8.8.8.8")
    pub tcp_host: String,
    /// Port for TCP connectivity check (default: 53)
    pub tcp_port: u16,
    /// Hostname for DNS resolution check (default: "dns.google")
    pub dns_hostname: String,
}

impl Default for NetworkTargets {
    fn default() -> Self {
        Self {
            tcp_host: "8.8.8.8".to_string(),
            tcp_port: 53,
            dns_hostname: "dns.google".to_string(),
        }
    }
}

/// Run a fallible probe with a timeout.
///
/// On timeout or error, logs via reporter and returns the type's Default value.
/// This ensures the diagnostic run always completes with a partial report rather
/// than aborting on a single probe failure.
async fn run_probe<T: Default>(
    name: &str,
    future: impl std::future::Future<Output = Result<T>>,
    reporter: &impl ProgressReporter,
) -> T {
    match timeout(PROBE_TIMEOUT, future).await {
        Ok(Ok(val)) => val,
        Ok(Err(e)) => {
            reporter.warn(&format!("{name} probe failed: {e}"));
            T::default()
        }
        Err(_) => {
            reporter.warn(&format!(
                "{name} probe timed out after {}s",
                PROBE_TIMEOUT.as_secs()
            ));
            T::default()
        }
    }
}

// ── Internal probes ───────────────────────────────────────────────────────────

/// # Errors
///
/// This function will return an error if the underlying operations fail.
async fn probe_prerequisites(
    cmd_runner: &impl CommandRunner,
) -> Result<crate::domain::health::PrerequisiteChecks> {
    let output = cmd_runner.run("multipass", &["version"]).await;
    let Ok(output) = output else {
        return Ok(crate::domain::health::PrerequisiteChecks {
            found: false,
            version: None,
            version_ok: false,
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
        .is_some_and(|v| v >= semver::Version::new(1, 16, 0));

    Ok(crate::domain::health::PrerequisiteChecks {
        found: true,
        version: version_str,
        version_ok,
    })
}

/// # Errors
///
/// This function will return an error if the underlying operations fail.
async fn probe_workspace(
    cmd_runner: &impl CommandRunner,
    paths: &(impl LocalPaths + crate::application::ports::LocalFs),
    polis_image_override: Option<String>,
    vm_running: bool,
) -> Result<crate::domain::health::WorkspaceChecks> {
    let disk_space_gb = probe_disk_space_gb(cmd_runner).await?;
    let image = probe_image_cache(paths, polis_image_override);

    Ok(crate::domain::health::WorkspaceChecks {
        ready: vm_running,
        disk_space_gb,
        disk_space_ok: disk_space_gb >= 10,
        image,
    })
}

/// Probe network connectivity.
///
/// Returns `NetworkChecks` directly (not wrapped in `Result`) since network
/// failures are expected conditions the doctor reports, not errors that should
/// abort the diagnostic run.
async fn probe_network(
    network_probe: &impl NetworkProbe,
    targets: &NetworkTargets,
) -> crate::domain::health::NetworkChecks {
    let internet = network_probe
        .check_tcp_connectivity(&targets.tcp_host, targets.tcp_port)
        .await
        .unwrap_or(false);
    let dns = network_probe
        .check_dns_resolution(&targets.dns_hostname)
        .await
        .unwrap_or(false);
    crate::domain::health::NetworkChecks { internet, dns }
}

/// # Errors
///
/// This function will return an error if the underlying operations fail.
async fn probe_security(
    provisioner: &(impl InstanceInspector + ShellExecutor),
    vm_running: bool,
) -> Result<crate::domain::health::SecurityChecks> {
    if !vm_running {
        return Ok(crate::domain::health::SecurityChecks::default());
    }

    let (process_isolation, traffic_inspection, malware_db, certificates) = tokio::join!(
        probe_process_isolation(provisioner),
        probe_gate_health(provisioner),
        probe_malware_db(provisioner),
        probe_certificates(provisioner),
    );

    Ok(crate::domain::health::SecurityChecks {
        process_isolation,
        traffic_inspection,
        malware_db: malware_db?,
        certificates: certificates?,
    })
}

// ── Low-level probe helpers ───────────────────────────────────────────────────

/// # Errors
///
/// This function will return an error if the underlying operations fail.
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
        let text = String::from_utf8_lossy(&out.stdout);
        text.trim()
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("cannot parse disk space output ({text}): {e}"))
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
            .ok_or_else(|| anyhow::anyhow!("cannot parse df output: {text}"))
    }
}

fn probe_image_cache(
    paths: &(impl LocalPaths + crate::application::ports::LocalFs),
    polis_image_override: Option<String>,
) -> crate::domain::health::ImageCheckResult {
    let images_dir = paths.images_dir();
    let cached = paths.exists(&images_dir.join("polis.qcow2"));
    crate::domain::health::ImageCheckResult {
        cached,
        polis_image_override,
    }
}

async fn probe_process_isolation(mp: &impl ShellExecutor) -> bool {
    mp.exec(&["sysbox-runc", "--version"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn probe_gate_health(mp: &impl ShellExecutor) -> bool {
    #[derive(serde::Deserialize)]
    struct GateEntry {
        #[serde(rename = "State")]
        state: String,
    }

    #[derive(serde::Deserialize)]
    struct HealthResponse {
        gate: Vec<GateEntry>,
    }

    let output = mp.exec(&[QUERY_SCRIPT, "health"]).await;
    let Ok(output) = output else { return false };
    if !output.status.success() {
        return false;
    }

    serde_json::from_slice::<HealthResponse>(&output.stdout)
        .ok()
        .and_then(|res| res.gate.first().map(|entry| entry.state == "running"))
        .unwrap_or(false)
}

/// Truncate a string to at most `max_len` bytes, appending "..." if truncated.
fn truncate_payload(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

async fn probe_malware_db(mp: &impl ShellExecutor) -> Result<MalwareDbStatus> {
    #[derive(serde::Deserialize)]
    struct MalwareResponse {
        daily_cvd_mtime: u64,
    }

    let output = mp.exec(&[QUERY_SCRIPT, "malware-db"]).await;
    let Ok(output) = output else {
        return Ok(MalwareDbStatus::default());
    };
    if !output.status.success() {
        return Ok(MalwareDbStatus::default());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let res = serde_json::from_slice::<MalwareResponse>(&output.stdout).map_err(|e| {
        let truncated = truncate_payload(&raw, 512);
        anyhow::anyhow!("failed to deserialize malware-db response ({truncated}): {e}")
    })?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age_hours = now.saturating_sub(res.daily_cvd_mtime) / 3600;
    Ok(MalwareDbStatus {
        is_current: age_hours <= 24,
        age_hours,
    })
}

async fn probe_certificates(mp: &impl ShellExecutor) -> Result<CertificateStatus> {
    #[derive(serde::Deserialize)]
    struct CertResponse {
        ca_expiry: String,
    }

    let output = mp.exec(&[QUERY_SCRIPT, "cert-expiry"]).await;
    let Ok(output) = output else {
        return Ok(CertificateStatus::default());
    };
    if !output.status.success() {
        return Ok(CertificateStatus::default());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let res = serde_json::from_slice::<CertResponse>(&output.stdout).map_err(|e| {
        let truncated = truncate_payload(&raw, 512);
        anyhow::anyhow!("failed to deserialize cert-expiry response ({truncated}): {e}")
    })?;

    let date_str = res.ca_expiry.trim();
    // Normalize whitespace: collapse multiple spaces into single space
    let normalized = date_str.split_whitespace().collect::<Vec<_>>().join(" ");
    let expiry = chrono::NaiveDateTime::parse_from_str(&normalized, "%b %d %H:%M:%S %Y GMT")
        .map_err(|e| anyhow::anyhow!("cannot parse certificate date {date_str:?}: {e}"))?;
    let now = chrono::Utc::now().naive_utc();
    let days = (expiry - now).num_days();
    Ok(CertificateStatus {
        is_valid: days > 0,
        expire_days: days,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::process::Output;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use anyhow::Result;

    use super::*;
    use crate::application::ports::{
        CommandRunner, FileTransfer, InstanceInspector, LocalFs, LocalPaths, NetworkProbe,
        ProgressReporter, ShellExecutor,
    };
    use crate::application::vm::test_support::{impl_shell_executor_stubs, ok_output};

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Mock provisioner that tracks `info()` call count and can be configured to fail.
    struct MockProvisioner {
        info_call_count: AtomicUsize,
        info_fails: bool,
    }

    impl MockProvisioner {
        fn new(info_fails: bool) -> Self {
            Self {
                info_call_count: AtomicUsize::new(0),
                info_fails,
            }
        }

        fn info_call_count(&self) -> usize {
            self.info_call_count.load(Ordering::SeqCst)
        }
    }

    impl InstanceInspector for MockProvisioner {
        async fn info(&self) -> Result<Output> {
            self.info_call_count.fetch_add(1, Ordering::SeqCst);
            if self.info_fails {
                anyhow::bail!("VM info failed")
            }
            // Return valid JSON for a running VM
            Ok(ok_output(
                br#"{"info":{"polis":{"state":"Running","ipv4":[]}}}"#,
            ))
        }

        async fn version(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }

    impl ShellExecutor for MockProvisioner {
        impl_shell_executor_stubs!(exec, exec_with_stdin, exec_spawn, exec_status);
    }

    impl FileTransfer for MockProvisioner {
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }

    /// Mock command runner that returns successful multipass version output.
    struct MockCommandRunner;

    impl CommandRunner for MockCommandRunner {
        async fn run(&self, program: &str, _args: &[&str]) -> Result<Output> {
            match program {
                "multipass" => Ok(ok_output(b"multipass 1.16.1\nmultipassd 1.16.1\n")),
                "df" => Ok(ok_output(b"Filesystem     1K-blocks      Used Available Use% Mounted on\n/dev/sda1      100000000  50000000  50000000  50% /\n")),
                _ => anyhow::bail!("unexpected program: {program}"),
            }
        }

        async fn run_with_timeout(
            &self,
            program: &str,
            args: &[&str],
            _timeout: std::time::Duration,
        ) -> Result<Output> {
            self.run(program, args).await
        }

        async fn run_with_stdin(&self, _: &str, _: &[&str], _: &[u8]) -> Result<Output> {
            anyhow::bail!("not expected")
        }

        fn spawn(&self, _: &str, _: &[&str]) -> Result<tokio::process::Child> {
            anyhow::bail!("not expected")
        }

        async fn run_status(&self, _: &str, _: &[&str]) -> Result<std::process::ExitStatus> {
            anyhow::bail!("not expected")
        }
    }

    /// Mock network probe that returns success for all checks.
    struct MockNetworkProbe;

    impl NetworkProbe for MockNetworkProbe {
        async fn check_tcp_connectivity(&self, _: &str, _: u16) -> Result<bool> {
            Ok(true)
        }

        async fn check_dns_resolution(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
    }

    /// Mock local paths and filesystem.
    struct MockLocalFs;

    impl LocalPaths for MockLocalFs {
        fn images_dir(&self) -> std::path::PathBuf {
            std::path::PathBuf::from("/tmp/images")
        }

        fn polis_dir(&self) -> Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from("/tmp/.polis"))
        }
    }

    impl LocalFs for MockLocalFs {
        fn exists(&self, _: &std::path::Path) -> bool {
            false
        }

        fn create_dir_all(&self, _: &std::path::Path) -> Result<()> {
            Ok(())
        }

        fn remove_dir_all(&self, _: &std::path::Path) -> Result<()> {
            Ok(())
        }

        fn remove_file(&self, _: &std::path::Path) -> Result<()> {
            Ok(())
        }

        fn write(&self, _: &std::path::Path, _: String) -> Result<()> {
            Ok(())
        }

        fn read_to_string(&self, _: &std::path::Path) -> Result<String> {
            Ok(String::new())
        }

        fn set_permissions(&self, _: &std::path::Path, _: u32) -> Result<()> {
            Ok(())
        }

        fn is_dir(&self, _: &std::path::Path) -> bool {
            false
        }
    }

    /// Silent progress reporter for tests.
    struct SilentReporter;

    impl ProgressReporter for SilentReporter {
        fn step(&self, _: &str) {}
        fn success(&self, _: &str) {}
        fn warn(&self, _: &str) {}
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    /// Test: When VM state resolution fails, diagnose still succeeds with VM treated as not running.
    ///
    /// Property 1: VM state resolved exactly once per diagnostic run
    /// Validates: Requirements 1.1, 1.2, 1.3
    #[tokio::test]
    async fn diagnose_succeeds_when_vm_state_resolution_fails() {
        // Arrange: Mock provisioner that fails on info() call
        let provisioner = MockProvisioner::new(true); // info_fails = true
        let cmd_runner = MockCommandRunner;
        let network_probe = MockNetworkProbe;
        let local_fs = MockLocalFs;
        let reporter = SilentReporter;
        let network_targets = NetworkTargets::default();

        // Act: Call diagnose
        let result = diagnose(
            &provisioner,
            &reporter,
            &cmd_runner,
            &network_probe,
            &local_fs,
            None,
            &network_targets,
        )
        .await;

        // Assert: diagnose succeeds
        assert!(
            result.is_ok(),
            "diagnose should succeed even when VM state resolution fails"
        );

        let report = result.unwrap();

        // Assert: VM is treated as not running (workspace.ready should be false)
        assert!(
            !report.workspace.ready,
            "workspace.ready should be false when VM state resolution fails"
        );

        // Assert: VM state was resolved exactly once (Property 1)
        assert_eq!(
            provisioner.info_call_count(),
            1,
            "vm::lifecycle::state() should be invoked exactly once per diagnostic run"
        );
    }

    /// Test: When VM state resolution succeeds, VM state is still resolved exactly once.
    ///
    /// Property 1: VM state resolved exactly once per diagnostic run
    /// Validates: Requirements 1.1, 1.2
    #[tokio::test]
    async fn diagnose_resolves_vm_state_exactly_once_on_success() {
        // Arrange: Mock provisioner that succeeds on info() call
        let provisioner = MockProvisioner::new(false); // info_fails = false
        let cmd_runner = MockCommandRunner;
        let network_probe = MockNetworkProbe;
        let local_fs = MockLocalFs;
        let reporter = SilentReporter;
        let network_targets = NetworkTargets::default();

        // Act: Call diagnose
        let result = diagnose(
            &provisioner,
            &reporter,
            &cmd_runner,
            &network_probe,
            &local_fs,
            None,
            &network_targets,
        )
        .await;

        // Assert: diagnose succeeds
        assert!(result.is_ok(), "diagnose should succeed");

        let report = result.unwrap();

        // Assert: VM is treated as running (workspace.ready should be true)
        assert!(
            report.workspace.ready,
            "workspace.ready should be true when VM state resolution succeeds and VM is running"
        );

        // Assert: VM state was resolved exactly once (Property 1)
        assert_eq!(
            provisioner.info_call_count(),
            1,
            "vm::lifecycle::state() should be invoked exactly once per diagnostic run"
        );
    }

    /// Mock network probe that always returns errors.
    struct FailingNetworkProbe;

    impl NetworkProbe for FailingNetworkProbe {
        async fn check_tcp_connectivity(&self, _: &str, _: u16) -> Result<bool> {
            Err(anyhow::anyhow!("TCP connectivity check failed"))
        }

        async fn check_dns_resolution(&self, _: &str) -> Result<bool> {
            Err(anyhow::anyhow!("DNS resolution check failed"))
        }
    }

    /// Test: When network probes fail, `probe_network` returns false values instead of propagating errors.
    ///
    /// Property 3: Network probe graceful degradation
    /// Validates: Requirements 8.1, 8.2, 8.3
    #[tokio::test]
    async fn probe_network_returns_false_when_checks_fail() {
        // Arrange: Mock network probe that returns Err for both checks
        let network_probe = FailingNetworkProbe;
        let network_targets = NetworkTargets::default();

        // Act: Call probe_network
        let result = probe_network(&network_probe, &network_targets).await;

        // Assert: Both checks should be false (graceful degradation, not error propagation)
        assert!(
            !result.internet,
            "internet should be false when TCP connectivity check fails"
        );
        assert!(
            !result.dns,
            "dns should be false when DNS resolution check fails"
        );
    }

    /// Mock network probe that records the arguments passed to it.
    struct RecordingNetworkProbe {
        tcp_host: std::sync::Mutex<Option<String>>,
        tcp_port: std::sync::Mutex<Option<u16>>,
        dns_hostname: std::sync::Mutex<Option<String>>,
    }

    impl RecordingNetworkProbe {
        fn new() -> Self {
            Self {
                tcp_host: std::sync::Mutex::new(None),
                tcp_port: std::sync::Mutex::new(None),
                dns_hostname: std::sync::Mutex::new(None),
            }
        }

        fn recorded_tcp_host(&self) -> Option<String> {
            self.tcp_host.lock().unwrap().clone()
        }

        fn recorded_tcp_port(&self) -> Option<u16> {
            *self.tcp_port.lock().unwrap()
        }

        fn recorded_dns_hostname(&self) -> Option<String> {
            self.dns_hostname.lock().unwrap().clone()
        }
    }

    impl NetworkProbe for RecordingNetworkProbe {
        async fn check_tcp_connectivity(&self, host: &str, port: u16) -> Result<bool> {
            *self.tcp_host.lock().unwrap() = Some(host.to_string());
            *self.tcp_port.lock().unwrap() = Some(port);
            Ok(true)
        }

        async fn check_dns_resolution(&self, hostname: &str) -> Result<bool> {
            *self.dns_hostname.lock().unwrap() = Some(hostname.to_string());
            Ok(true)
        }
    }

    /// Test: Custom `NetworkTargets` values are correctly passed to network probe methods.
    ///
    /// Property 9: Configurable network targets are used
    /// Validates: Requirements 19.1, 19.3
    #[tokio::test]
    async fn probe_network_uses_configured_targets() {
        // Arrange: Recording mock and custom network targets
        let network_probe = RecordingNetworkProbe::new();
        let custom_targets = NetworkTargets {
            tcp_host: "192.168.1.1".to_string(),
            tcp_port: 8080,
            dns_hostname: "custom.dns.server".to_string(),
        };

        // Act: Call probe_network with custom targets
        let _result = probe_network(&network_probe, &custom_targets).await;

        // Assert: Configured values were passed to check_tcp_connectivity
        assert_eq!(
            network_probe.recorded_tcp_host(),
            Some("192.168.1.1".to_string()),
            "tcp_host should be passed to check_tcp_connectivity"
        );
        assert_eq!(
            network_probe.recorded_tcp_port(),
            Some(8080),
            "tcp_port should be passed to check_tcp_connectivity"
        );

        // Assert: Configured dns_hostname was passed to check_dns_resolution
        assert_eq!(
            network_probe.recorded_dns_hostname(),
            Some("custom.dns.server".to_string()),
            "dns_hostname should be passed to check_dns_resolution"
        );
    }

    /// Unit test for `NetworkTargets::default()` values
    /// Validates: Requirement 19.2
    #[test]
    fn network_targets_default_returns_expected_values() {
        // Act: Create default NetworkTargets
        let targets = NetworkTargets::default();

        // Assert: Verify default values match expected Google DNS targets
        assert_eq!(
            targets.tcp_host, "8.8.8.8",
            "default tcp_host should be Google DNS IP"
        );
        assert_eq!(
            targets.tcp_port, 53,
            "default tcp_port should be DNS port 53"
        );
        assert_eq!(
            targets.dns_hostname, "dns.google",
            "default dns_hostname should be dns.google"
        );
    }
}
