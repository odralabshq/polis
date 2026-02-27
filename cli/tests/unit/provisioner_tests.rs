//! Unit and property tests for `MultipassProvisioner`.
//!
//! **Property 5: CLI Argument Construction**
//!
//! These tests verify that `MultipassProvisioner` builds the correct CLI
//! argument lists for `launch()`, `exec()`, and `transfer()`, and that
//! error context is correctly attached.
//!
//! **Validates: Requirements 4.3, 4.4, 4.5, 4.6, 4.8**

use std::process::{ExitStatus, Output};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, bail};
use polis_cli::command_runner::CommandRunner;
use polis_cli::provisioner::{
    FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec, MultipassProvisioner,
    POLIS_INSTANCE, ShellExecutor,
};
use proptest::prelude::*;

// ─── MockCommandRunner ────────────────────────────────────────────────────────

/// A `CommandRunner` that records every `(program, args)` call and returns a
/// configurable canned result.
///
/// Thread-safe via `Arc<Mutex<…>>` so it can be cloned into two runners
/// (`cmd_runner` + `exec_runner`) that share the same call log.
#[derive(Clone)]
struct MockCommandRunner {
    /// All recorded `(program, args)` pairs in call order.
    calls: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    /// The result to return from `run()` / `run_with_timeout()` / `run_with_stdin()`.
    result: Arc<dyn Fn() -> Result<Output> + Send + Sync>,
}

impl MockCommandRunner {
    /// Create a mock that always returns `Ok` with a zero exit status.
    fn new_ok() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            result: Arc::new(|| Ok(success_output())),
        }
    }

    /// Create a mock that always returns the given error message.
    fn new_err(msg: &'static str) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            result: Arc::new(move || bail!("{msg}")),
        }
    }

    /// Return a snapshot of all recorded calls.
    fn recorded_calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().expect("mutex poisoned").clone()
    }
}

impl CommandRunner for MockCommandRunner {
    async fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
        self.calls.lock().expect("mutex poisoned").push((
            program.to_owned(),
            args.iter().map(|s| s.to_string()).collect(),
        ));
        (self.result)()
    }

    async fn run_with_timeout(
        &self,
        program: &str,
        args: &[&str],
        _timeout: Duration,
    ) -> Result<Output> {
        self.run(program, args).await
    }

    async fn run_with_stdin(&self, program: &str, args: &[&str], _input: &[u8]) -> Result<Output> {
        self.run(program, args).await
    }

    fn spawn(&self, program: &str, args: &[&str]) -> Result<tokio::process::Child> {
        self.calls.lock().expect("mutex poisoned").push((
            program.to_owned(),
            args.iter().map(|s| s.to_string()).collect(),
        ));
        bail!("spawn not supported in MockCommandRunner")
    }

    async fn run_status(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        self.calls.lock().expect("mutex poisoned").push((
            program.to_owned(),
            args.iter().map(|s| s.to_string()).collect(),
        ));
        bail!("run_status not supported in MockCommandRunner")
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn success_output() -> Output {
    Output {
        status: exit_status(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

/// Build a fake `ExitStatus` with the given code.
/// Uses `std::process::Command` to get a real `ExitStatus` value.
fn exit_status(code: i32) -> ExitStatus {
    // The only stable way to construct an ExitStatus in tests is to run a
    // trivial process and capture its status.
    if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .status()
            .expect("failed to spawn helper process for exit status")
    } else {
        let script = format!("exit {code}");
        std::process::Command::new("sh")
            .args(["-c", &script])
            .status()
            .expect("failed to spawn helper process for exit status")
    }
}

/// Build a `MultipassProvisioner` backed by two independent `MockCommandRunner`s
/// that share the same call log (via `Arc<Mutex<…>>`).
fn make_provisioner(mock: &MockCommandRunner) -> MultipassProvisioner<MockCommandRunner> {
    MultipassProvisioner::new(mock.clone(), mock.clone())
}

/// Build a `MultipassProvisioner` where `cmd_runner` and `exec_runner` are
/// separate mocks so callers can distinguish which runner was used.
fn make_provisioner_split(
    cmd: MockCommandRunner,
    exec: MockCommandRunner,
) -> MultipassProvisioner<MockCommandRunner> {
    MultipassProvisioner::new(cmd, exec)
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

/// `info()` must delegate to `cmd_runner.run("multipass", ["info", "polis", "--format", "json"])`.
///
/// **Validates: Requirement 4.3**
#[tokio::test]
async fn test_info_delegates_to_cmd_runner() {
    let mock = MockCommandRunner::new_ok();
    let mp = make_provisioner(&mock);

    mp.info().await.expect("info should succeed");

    let calls = mock.recorded_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "multipass");
    assert_eq!(calls[0].1, ["info", POLIS_INSTANCE, "--format", "json"]);
}

/// `exec()` must delegate to `exec_runner.run("multipass", ["exec", "polis", "--", ...args])`.
///
/// **Validates: Requirement 4.4**
#[tokio::test]
async fn test_exec_delegates_to_exec_runner() {
    let cmd_mock = MockCommandRunner::new_ok();
    let exec_mock = MockCommandRunner::new_ok();
    let mp = make_provisioner_split(cmd_mock.clone(), exec_mock.clone());

    mp.exec(&["docker", "ps"])
        .await
        .expect("exec should succeed");

    // exec_runner should have received the call
    let exec_calls = exec_mock.recorded_calls();
    assert_eq!(exec_calls.len(), 1);
    assert_eq!(exec_calls[0].0, "multipass");
    assert_eq!(
        exec_calls[0].1,
        ["exec", POLIS_INSTANCE, "--", "docker", "ps"]
    );

    // cmd_runner should NOT have been called
    assert!(cmd_mock.recorded_calls().is_empty());
}

/// `launch()` must build the correct arg list from an `InstanceSpec`, including
/// optional `--cloud-init` and `--timeout` flags.
///
/// **Validates: Requirement 4.5**
#[tokio::test]
async fn test_launch_builds_correct_args_without_optional_fields() {
    let mock = MockCommandRunner::new_ok();
    let mp = make_provisioner(&mock);

    let spec = InstanceSpec {
        image: "24.04",
        cpus: "2",
        memory: "8G",
        disk: "40G",
        cloud_init: None,
        timeout: None,
    };
    mp.launch(&spec).await.expect("launch should succeed");

    let calls = mock.recorded_calls();
    assert_eq!(calls.len(), 1);
    let args = &calls[0].1;
    assert_eq!(args[0], "launch");
    assert_eq!(args[1], "24.04");
    assert!(args.contains(&"--name".to_owned()));
    assert!(args.contains(&POLIS_INSTANCE.to_owned()));
    assert!(args.contains(&"--cpus".to_owned()));
    assert!(args.contains(&"2".to_owned()));
    assert!(args.contains(&"--memory".to_owned()));
    assert!(args.contains(&"8G".to_owned()));
    assert!(args.contains(&"--disk".to_owned()));
    assert!(args.contains(&"40G".to_owned()));
    // Default timeout "600" should be present
    assert!(args.contains(&"--timeout".to_owned()));
    assert!(args.contains(&"600".to_owned()));
    // No --cloud-init
    assert!(!args.contains(&"--cloud-init".to_owned()));
}

/// `launch()` with `cloud_init` and `timeout` set must include both optional flags.
///
/// **Validates: Requirement 4.5**
#[tokio::test]
async fn test_launch_includes_optional_cloud_init_and_timeout() {
    let mock = MockCommandRunner::new_ok();
    let mp = make_provisioner(&mock);

    let spec = InstanceSpec {
        image: "22.04",
        cpus: "4",
        memory: "16G",
        disk: "80G",
        cloud_init: Some("/tmp/cloud-init.yaml"),
        timeout: Some("900"),
    };
    mp.launch(&spec).await.expect("launch should succeed");

    let calls = mock.recorded_calls();
    let args = &calls[0].1;
    assert!(args.contains(&"--cloud-init".to_owned()));
    assert!(args.contains(&"/tmp/cloud-init.yaml".to_owned()));
    assert!(args.contains(&"--timeout".to_owned()));
    assert!(args.contains(&"900".to_owned()));
}

/// `transfer()` must format the destination as `polis:<remote_path>`.
///
/// **Validates: Requirement 4.6**
#[tokio::test]
async fn test_transfer_formats_destination_as_polis_colon_path() {
    let mock = MockCommandRunner::new_ok();
    let mp = make_provisioner(&mock);

    mp.transfer("/local/file.txt", "/remote/file.txt")
        .await
        .expect("transfer should succeed");

    let calls = mock.recorded_calls();
    assert_eq!(calls.len(), 1);
    let args = &calls[0].1;
    assert_eq!(args[0], "transfer");
    assert_eq!(args[1], "/local/file.txt");
    assert_eq!(args[2], "polis:/remote/file.txt");
}

/// When the runner returns an error, `info()` error chain must contain
/// "failed to run multipass info".
///
/// **Validates: Requirement 4.8**
#[tokio::test]
async fn test_info_error_context() {
    let mock = MockCommandRunner::new_err("runner error");
    let mp = make_provisioner(&mock);

    let err = mp.info().await.expect_err("info should fail");
    let chain = format!("{err:#}");
    assert!(
        chain.contains("failed to run multipass info"),
        "error chain was: {chain}"
    );
}

/// When the runner returns an error, `exec()` error chain must contain
/// "failed to run multipass exec".
///
/// **Validates: Requirement 4.8**
#[tokio::test]
async fn test_exec_error_context() {
    let mock = MockCommandRunner::new_err("runner error");
    let mp = make_provisioner(&mock);

    let err = mp.exec(&["ls"]).await.expect_err("exec should fail");
    let chain = format!("{err:#}");
    assert!(
        chain.contains("failed to run multipass exec"),
        "error chain was: {chain}"
    );
}

/// When the runner returns an error, `launch()` error chain must contain
/// "failed to run multipass launch".
///
/// **Validates: Requirement 4.8**
#[tokio::test]
async fn test_launch_error_context() {
    let mock = MockCommandRunner::new_err("runner error");
    let mp = make_provisioner(&mock);

    let spec = InstanceSpec {
        image: "24.04",
        cpus: "2",
        memory: "8G",
        disk: "40G",
        cloud_init: None,
        timeout: None,
    };
    let err = mp.launch(&spec).await.expect_err("launch should fail");
    let chain = format!("{err:#}");
    assert!(
        chain.contains("failed to run multipass launch"),
        "error chain was: {chain}"
    );
}

/// When the runner returns an error, `transfer()` error chain must contain
/// "failed to run multipass transfer".
///
/// **Validates: Requirement 4.8**
#[tokio::test]
async fn test_transfer_error_context() {
    let mock = MockCommandRunner::new_err("runner error");
    let mp = make_provisioner(&mock);

    let err = mp
        .transfer("/local/file.txt", "/remote/file.txt")
        .await
        .expect_err("transfer should fail");
    let chain = format!("{err:#}");
    assert!(
        chain.contains("failed to run multipass transfer"),
        "error chain was: {chain}"
    );
}

// ─── Property-based tests ─────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// **Validates: Requirements 4.5**
    ///
    /// Property 5a: `launch()` builds the correct arg list from an `InstanceSpec`.
    ///
    /// For any `InstanceSpec` (with and without `cloud_init` and `timeout`),
    /// `MultipassProvisioner::launch()` must pass the correct argument list to the
    /// runner, including optional `--cloud-init` and `--timeout` flags.
    #[test]
    fn prop_launch_builds_correct_args(
        image in "[a-z0-9.]{1,10}",
        cpus in "[1-9][0-9]?",
        memory in "[1-9][0-9]?[GM]",
        disk in "[1-9][0-9]?G",
        cloud_init in proptest::option::of("[/a-z0-9._-]{1,30}"),
        timeout in proptest::option::of("[1-9][0-9]{1,3}"),
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let mock = MockCommandRunner::new_ok();
            let mp = make_provisioner(&mock);

            let ci = cloud_init.as_deref();
            let to = timeout.as_deref();
            let spec = InstanceSpec {
                image: &image,
                cpus: &cpus,
                memory: &memory,
                disk: &disk,
                cloud_init: ci,
                timeout: to,
            };
            mp.launch(&spec).await.expect("launch should succeed");

            let calls = mock.recorded_calls();
            prop_assert_eq!(calls.len(), 1);
            let args = &calls[0].1;

            // Program
            prop_assert_eq!(&calls[0].0, "multipass");

            // Subcommand and image
            prop_assert_eq!(&args[0], "launch");
            prop_assert_eq!(&args[1], &image);

            // Required flags
            prop_assert!(args.contains(&"--name".to_owned()));
            prop_assert!(args.contains(&POLIS_INSTANCE.to_owned()));
            prop_assert!(args.contains(&"--cpus".to_owned()));
            prop_assert!(args.contains(&cpus));
            prop_assert!(args.contains(&"--memory".to_owned()));
            prop_assert!(args.contains(&memory));
            prop_assert!(args.contains(&"--disk".to_owned()));
            prop_assert!(args.contains(&disk));
            prop_assert!(args.contains(&"--timeout".to_owned()));

            // Timeout value: explicit or default "600"
            let expected_timeout = to.unwrap_or("600");
            prop_assert!(args.contains(&expected_timeout.to_owned()));

            // Optional --cloud-init
            if let Some(ci_path) = ci {
                prop_assert!(args.contains(&"--cloud-init".to_owned()));
                prop_assert!(args.contains(&ci_path.to_owned()));
            } else {
                prop_assert!(!args.contains(&"--cloud-init".to_owned()));
            }

            Ok(())
        })?;
    }

    /// **Validates: Requirements 4.4**
    ///
    /// Property 5b: `exec()` prepends `["exec", "polis", "--"]` to the args.
    ///
    /// For any slice of exec args, `MultipassProvisioner::exec()` must delegate to
    /// the runner with `["exec", "polis", "--", ...args]` as the argument list.
    #[test]
    fn prop_exec_prepends_exec_polis_separator(
        user_args in proptest::collection::vec("[a-z][a-z0-9_-]{0,15}", 0..8),
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let mock = MockCommandRunner::new_ok();
            let mp = make_provisioner(&mock);

            let refs: Vec<&str> = user_args.iter().map(String::as_str).collect();
            mp.exec(&refs).await.expect("exec should succeed");

            let calls = mock.recorded_calls();
            prop_assert_eq!(calls.len(), 1);
            let args = &calls[0].1;

            // Must start with the fixed prefix
            prop_assert!(args.len() >= 3);
            prop_assert_eq!(&args[0], "exec");
            prop_assert_eq!(&args[1], POLIS_INSTANCE);
            prop_assert_eq!(&args[2], "--");

            // Remaining args must match user_args exactly
            let tail: Vec<&str> = args[3..].iter().map(String::as_str).collect();
            prop_assert_eq!(tail, refs);

            Ok(())
        })?;
    }

    /// **Validates: Requirements 4.6**
    ///
    /// Property 5c: `transfer()` formats the destination as `polis:<remote_path>`.
    ///
    /// For any remote path string, `MultipassProvisioner::transfer()` must pass
    /// `polis:<remote_path>` as the destination argument to the runner.
    #[test]
    fn prop_transfer_formats_destination_as_polis_colon_path(
        remote_path in "/[a-z][a-z0-9/_.-]{0,40}",
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let mock = MockCommandRunner::new_ok();
            let mp = make_provisioner(&mock);

            mp.transfer("/local/src", &remote_path)
                .await
                .expect("transfer should succeed");

            let calls = mock.recorded_calls();
            prop_assert_eq!(calls.len(), 1);
            let args = &calls[0].1;

            // args: ["transfer", local_path, "polis:<remote_path>"]
            prop_assert_eq!(args.len(), 3);
            prop_assert_eq!(&args[0], "transfer");
            prop_assert_eq!(&args[1], "/local/src");
            let expected_dest = format!("{POLIS_INSTANCE}:{remote_path}");
            prop_assert_eq!(&args[2], &expected_dest);

            Ok(())
        })?;
    }

}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// **Validates: Requirements 4.8**
    ///
    /// Property 6: Error Context Propagation
    ///
    /// For each trait method, injecting a `CommandRunner` error must produce an
    /// error chain containing "failed to run multipass {subcommand}".
    ///
    /// Uses a fixed error message and iterates over all methods in the test body.
    #[test]
    fn prop_error_context_propagation(_dummy in proptest::bool::ANY) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            // Each entry: (method name for diagnostics, expected context string)
            // We run each method with a failing mock and check the error chain.

            // info()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp.info().await.expect_err("info should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass info"),
                    "info() error chain was: {chain}"
                );
            }

            // version()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp.version().await.expect_err("version should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass version"),
                    "version() error chain was: {chain}"
                );
            }

            // start()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp.start().await.expect_err("start should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass start"),
                    "start() error chain was: {chain}"
                );
            }

            // stop()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp.stop().await.expect_err("stop should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass stop"),
                    "stop() error chain was: {chain}"
                );
            }

            // delete()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp.delete().await.expect_err("delete should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass delete"),
                    "delete() error chain was: {chain}"
                );
            }

            // purge()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp.purge().await.expect_err("purge should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass purge"),
                    "purge() error chain was: {chain}"
                );
            }

            // exec()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp.exec(&["ls"]).await.expect_err("exec should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass exec"),
                    "exec() error chain was: {chain}"
                );
            }

            // transfer()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp
                    .transfer("/local/file", "/remote/file")
                    .await
                    .expect_err("transfer should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass transfer"),
                    "transfer() error chain was: {chain}"
                );
            }

            // transfer_recursive()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp
                    .transfer_recursive("/local/dir", "/remote/dir")
                    .await
                    .expect_err("transfer_recursive should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass transfer"),
                    "transfer_recursive() error chain was: {chain}"
                );
            }

            // exec_with_stdin()
            {
                let mock = MockCommandRunner::new_err("injected runner error");
                let mp = make_provisioner(&mock);
                let err = mp
                    .exec_with_stdin(&["tee", "/tmp/file"], b"data")
                    .await
                    .expect_err("exec_with_stdin should fail");
                let chain = format!("{err:#}");
                prop_assert!(
                    chain.contains("failed to run multipass exec"),
                    "exec_with_stdin() error chain was: {chain}"
                );
            }

            Ok(())
        })?;
    }
}

// ─── Property 9: Timeout Isolation ───────────────────────────────────────────

/// Returns a slow command that runs for ~60 seconds on the current platform.
/// On Windows: `timeout /t 60 /nobreak`
/// On Unix: `sleep 60`
#[cfg(windows)]
fn slow_command_60s() -> (&'static str, Vec<&'static str>) {
    ("timeout", vec!["/t", "60", "/nobreak"])
}

#[cfg(not(windows))]
fn slow_command_60s() -> (&'static str, Vec<&'static str>) {
    ("sleep", vec!["60"])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(5))]

    /// **Validates: Requirements 4.1, 11.5**
    ///
    /// Property 9: Timeout Isolation
    ///
    /// `cmd_runner` and `exec_runner` in `MultipassProvisioner` have independent
    /// timeout configurations. A slow command run through one runner does not
    /// consume the timeout budget of the other.
    ///
    /// Approach: create two `TokioCommandRunner` instances with different timeouts
    /// (cmd_timeout_ms < exec_timeout_ms), run a slow command through each
    /// directly, and verify:
    ///   1. Each runner returns `Err` containing "timed out"
    ///   2. Each runner completes within its own timeout + 2s slack
    ///   3. The cmd runner completes before the exec runner's timeout would fire
    ///      (proving the timeouts are independent — one doesn't affect the other)
    #[test]
    fn prop_timeout_isolation(
        cmd_timeout_ms in 50u64..=200u64,
        exec_timeout_ms in 201u64..=400u64,
    ) {
        use std::time::Instant;
        use polis_cli::command_runner::TokioCommandRunner;

        let cmd_timeout = Duration::from_millis(cmd_timeout_ms);
        let exec_timeout = Duration::from_millis(exec_timeout_ms);
        // Allow generous slack for process spawn overhead and scheduler jitter
        let slack = Duration::from_secs(2);

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let cmd_runner = TokioCommandRunner::new(cmd_timeout);
            let exec_runner = TokioCommandRunner::new(exec_timeout);

            let (program, args) = slow_command_60s();

            // ── Run slow command through cmd_runner ──────────────────────────
            let cmd_start = Instant::now();
            let cmd_result = cmd_runner.run(program, &args).await;
            let cmd_elapsed = cmd_start.elapsed();

            // Must return Err containing "timed out"
            prop_assert!(
                cmd_result.is_err(),
                "cmd_runner: expected Err but got Ok (timeout={}ms)",
                cmd_timeout_ms
            );
            let cmd_err = format!("{:#}", cmd_result.unwrap_err());
            prop_assert!(
                cmd_err.contains("timed out"),
                "cmd_runner: error does not contain 'timed out': {cmd_err}"
            );

            // Must complete within cmd_timeout + slack
            prop_assert!(
                cmd_elapsed <= cmd_timeout + slack,
                "cmd_runner took {}ms, expected <= {}ms (timeout={}ms + 2s slack)",
                cmd_elapsed.as_millis(),
                (cmd_timeout + slack).as_millis(),
                cmd_timeout_ms,
            );

            // ── Run slow command through exec_runner ─────────────────────────
            let exec_start = Instant::now();
            let exec_result = exec_runner.run(program, &args).await;
            let exec_elapsed = exec_start.elapsed();

            // Must return Err containing "timed out"
            prop_assert!(
                exec_result.is_err(),
                "exec_runner: expected Err but got Ok (timeout={}ms)",
                exec_timeout_ms
            );
            let exec_err = format!("{:#}", exec_result.unwrap_err());
            prop_assert!(
                exec_err.contains("timed out"),
                "exec_runner: error does not contain 'timed out': {exec_err}"
            );

            // Must complete within exec_timeout + slack
            prop_assert!(
                exec_elapsed <= exec_timeout + slack,
                "exec_runner took {}ms, expected <= {}ms (timeout={}ms + 2s slack)",
                exec_elapsed.as_millis(),
                (exec_timeout + slack).as_millis(),
                exec_timeout_ms,
            );

            // ── Independence assertion ────────────────────────────────────────
            // The cmd_runner completed before exec_timeout would have fired,
            // proving the two timeouts are independent. If they shared a timeout,
            // cmd_elapsed would be >= exec_timeout_ms.
            prop_assert!(
                cmd_elapsed < exec_timeout,
                "cmd_runner took {}ms which is >= exec_timeout {}ms — timeouts may not be independent",
                cmd_elapsed.as_millis(),
                exec_timeout_ms,
            );

            Ok(())
        })?;
    }
}

// ─── Property 7: VM State Parsing Preservation ───────────────────────────────
//
// Generate valid `multipass info` JSON payloads with various state strings,
// verify `vm::state()` produces the correct `VmState` mapping.
// Also verify `vm::exists()` returns `true` iff info call succeeds.
//
// **Validates: Requirements 8.1, 11.3**

use polis_cli::workspace::vm::{VmState, exists, state};

/// A minimal `InstanceInspector` that returns a canned `Output`.
struct StubInspector {
    output: std::process::Output,
}

impl StubInspector {
    fn ok_with_state(state_str: &str) -> Self {
        let json = format!(r#"{{"info":{{"polis":{{"state":"{}"}}}}}}"#, state_str);
        Self {
            output: std::process::Output {
                status: success_output().status,
                stdout: json.into_bytes(),
                stderr: Vec::new(),
            },
        }
    }

    fn fail() -> Self {
        Self {
            output: std::process::Output {
                status: {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        std::process::ExitStatus::from_raw(1 << 8)
                    }
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::ExitStatusExt;
                        std::process::ExitStatus::from_raw(1)
                    }
                },
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        }
    }
}

impl InstanceInspector for StubInspector {
    async fn info(&self) -> Result<std::process::Output> {
        Ok(std::process::Output {
            status: self.output.status,
            stdout: self.output.stdout.clone(),
            stderr: self.output.stderr.clone(),
        })
    }
    async fn version(&self) -> Result<std::process::Output> {
        bail!("not expected")
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// Property 7a: Known state strings map to the correct VmState variant.
    #[test]
    fn prop_vm_state_known_strings_map_correctly(
        state_str in prop_oneof![
            Just("Running"),
            Just("Stopped"),
            Just("Starting"),
        ]
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let stub = StubInspector::ok_with_state(state_str);
            let result = state(&stub).await.expect("state should succeed");
            let expected = match state_str {
                "Running" => VmState::Running,
                "Starting" => VmState::Starting,
                _ => VmState::Stopped,
            };
            prop_assert_eq!(result, expected,
                "state string '{}' should map to {:?}", state_str, expected);
            Ok(())
        })?;
    }

    /// Property 7b: Unknown state strings map to VmState::Stopped (catch-all).
    #[test]
    fn prop_vm_state_unknown_strings_map_to_stopped(
        state_str in "[A-Za-z]{4,12}"
            .prop_filter("exclude known states", |s| {
                s != "Running" && s != "Starting" && s != "Stopped"
            })
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let stub = StubInspector::ok_with_state(&state_str);
            let result = state(&stub).await.expect("state should succeed");
            prop_assert_eq!(result, VmState::Stopped,
                "unknown state '{}' should map to Stopped", state_str);
            Ok(())
        })?;
    }

    /// Property 7c: exists() returns true iff info() succeeds with success status.
    #[test]
    fn prop_vm_exists_iff_info_succeeds(
        state_str in prop_oneof![Just("Running"), Just("Stopped"), Just("Starting")]
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let stub = StubInspector::ok_with_state(state_str);
            prop_assert!(exists(&stub).await, "exists should be true when info succeeds");
            Ok(())
        })?;
    }
}

#[tokio::test]
async fn vm_state_not_found_when_info_fails() {
    let stub = StubInspector::fail();
    assert_eq!(state(&stub).await.expect("state"), VmState::NotFound);
}

#[tokio::test]
async fn vm_exists_false_when_info_fails() {
    let stub = StubInspector::fail();
    assert!(!exists(&stub).await);
}

// ─── Property 4: Trait Compatibility ─────────────────────────────────────────
//
// Verify that migrated test doubles implement only the sub-traits they need,
// and that `MultipassProvisioner` satisfies the full `VmProvisioner` composite.
//
// The original `Multipass` god trait had 13 methods. Each ISP sub-trait has:
//   InstanceLifecycle  — 5 methods (launch, start, stop, delete, purge)
//   InstanceInspector  — 2 methods (info, version)
//   FileTransfer       — 2 methods (transfer, transfer_recursive)
//   ShellExecutor      — 4 methods (exec, exec_with_stdin, exec_spawn, exec_status)
//
// A mock that implements only one sub-trait exposes far fewer methods than 13.
// These tests are compile-time proofs: if the mock doesn't satisfy the bound,
// the test won't compile. The runtime assertion is trivially true.
//
// **Validates: Requirements 8.6, 11.4**

use polis_cli::provisioner::VmProvisioner;

/// Accepts any `InstanceInspector` — proves the mock satisfies the bound.
fn assert_inspector<T: InstanceInspector>(_: &T) {}

/// Accepts any `ShellExecutor` — proves the mock satisfies the bound.
fn assert_shell_executor<T: ShellExecutor>(_: &T) {}

/// Accepts any `InstanceLifecycle` — proves the mock satisfies the bound.
fn assert_lifecycle<T: InstanceLifecycle>(_: &T) {}

/// Accepts any `FileTransfer` — proves the mock satisfies the bound.
fn assert_file_transfer<T: FileTransfer>(_: &T) {}

/// Accepts any `VmProvisioner` — proves the type satisfies the full composite.
fn assert_vm_provisioner<T: VmProvisioner>(_: &T) {}

/// `MultipassProvisioner<MockCommandRunner>` must satisfy `VmProvisioner`
/// (all four sub-traits). This is the primary production-code assertion.
#[test]
fn prop_multipass_provisioner_satisfies_vm_provisioner() {
    let mock = MockCommandRunner::new_ok();
    let mp = make_provisioner(&mock);
    // Compile-time proof: mp satisfies all four sub-traits via VmProvisioner.
    assert_vm_provisioner(&mp);
    assert_inspector(&mp);
    assert_shell_executor(&mp);
    assert_lifecycle(&mp);
    assert_file_transfer(&mp);
}

/// `MultipassVmNotFound` (from mocks.rs) implements all four sub-traits
/// (needed by consumers that take `&impl VmProvisioner`), but the test
/// verifies it can be used as each individual sub-trait independently.
#[test]
fn prop_mock_vm_not_found_satisfies_sub_traits() {
    use crate::mocks::MultipassVmNotFound;
    let mock = MultipassVmNotFound;
    assert_inspector(&mock);
    assert_shell_executor(&mock);
    assert_lifecycle(&mock);
    assert_file_transfer(&mock);
}

/// `MultipassVmRunning` satisfies all four sub-traits.
#[test]
fn prop_mock_vm_running_satisfies_sub_traits() {
    use crate::mocks::MultipassVmRunning;
    let mock = MultipassVmRunning;
    assert_inspector(&mock);
    assert_shell_executor(&mock);
    assert_lifecycle(&mock);
    assert_file_transfer(&mock);
}

/// `MultipassVmStopped` satisfies all four sub-traits.
#[test]
fn prop_mock_vm_stopped_satisfies_sub_traits() {
    use crate::mocks::MultipassVmStopped;
    let mock = MultipassVmStopped;
    assert_inspector(&mock);
    assert_shell_executor(&mock);
    assert_lifecycle(&mock);
    assert_file_transfer(&mock);
}

/// `MultipassExecRecorder` satisfies all four sub-traits.
#[test]
fn prop_mock_exec_recorder_satisfies_sub_traits() {
    use crate::mocks::MultipassExecRecorder;
    let mock = MultipassExecRecorder::new();
    assert_inspector(&mock);
    assert_shell_executor(&mock);
    assert_lifecycle(&mock);
    assert_file_transfer(&mock);
}

/// `StubInspector` (defined in this file) satisfies `InstanceInspector`.
/// It does NOT need to implement the other three sub-traits — this is the
/// ISP win: a consumer that only needs `InstanceInspector` can use a stub
/// with just 2 methods instead of 13.
#[test]
fn prop_stub_inspector_satisfies_only_inspector_trait() {
    let stub = StubInspector::ok_with_state("Running");
    // Compile-time proof: stub satisfies InstanceInspector.
    assert_inspector(&stub);
    // Runtime assertion is trivially true — the value of this test is that
    // it compiles, proving the ISP split is working correctly.
    assert!(true);
}

/// `MockCommandRunner` (defined in this file) satisfies `CommandRunner`.
/// It does NOT implement any domain sub-traits — it lives at the process layer.
/// This verifies the two-layer separation: process layer vs domain layer.
#[test]
fn prop_mock_command_runner_is_process_layer_only() {
    use polis_cli::command_runner::CommandRunner;
    fn assert_command_runner<T: CommandRunner>(_: &T) {}
    let mock = MockCommandRunner::new_ok();
    // Compile-time proof: mock satisfies CommandRunner (process layer).
    assert_command_runner(&mock);
    // It does NOT implement InstanceInspector, ShellExecutor, etc. —
    // that separation is enforced by the type system.
    assert!(true);
}
