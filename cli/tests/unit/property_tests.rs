//! Property-based tests for critical validation and generation logic.
//!
//! Uses `proptest` to verify invariants across many random inputs.

#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

use proptest::prelude::*;

use polis_cli::command_runner::{CommandRunner, TokioCommandRunner};
use polis_cli::commands::config::{validate_config_key, validate_config_value};
use polis_cli::commands::start::generate_workspace_id;
use polis_cli::workspace::vm::generate_certs_and_secrets;

use crate::mocks::MultipassExecRecorder;

// ============================================================================
// generate_workspace_id() property tests
// ============================================================================

proptest! {
    /// Generated IDs always have correct format: polis- prefix + 16 hex chars.
    #[test]
    fn prop_workspace_id_has_valid_format(
        major in 0u32..100,
        minor in 0u32..100,
    ) {
        // Use inputs to vary the test run; the real invariant is on the generated ID.
        let _ = (major, minor);
        let id = generate_workspace_id();
        prop_assert!(id.starts_with("polis-"), "missing polis- prefix: {}", id);
        prop_assert!(id.len() == 22, "wrong length: {}", id);
        prop_assert!(id[6..].chars().all(|c| c.is_ascii_hexdigit()), "non-hex chars: {}", id);
    }
}

#[test]
fn test_workspace_id_uniqueness_batch() {
    // Generate 100 IDs and verify all are unique
    let ids: std::collections::HashSet<_> = (0..100).map(|_| generate_workspace_id()).collect();
    assert_eq!(ids.len(), 100, "duplicate IDs generated");
}

// ============================================================================
// validate_config_key() and validate_config_value() property tests
// ============================================================================

proptest! {
    /// Arbitrary keys (not in whitelist) are rejected.
    #[test]
    fn prop_arbitrary_keys_rejected(key in "[a-z]{1,20}\\.[a-z]{1,20}") {
        // Skip the one valid key
        if key != "security.level" {
            prop_assert!(validate_config_key(&key).is_err(), "accepted invalid key: {key}");
        }
    }

    /// Arbitrary values for security.level (not in whitelist) are rejected.
    #[test]
    fn prop_arbitrary_security_values_rejected(value in "[a-z]{1,20}") {
        if value != "balanced" && value != "strict" && value != "relaxed" {
            prop_assert!(
                validate_config_value("security.level", &value).is_err(),
                "accepted invalid value: {value}"
            );
        }
    }
}

#[test]
fn test_config_key_whitelist() {
    assert!(validate_config_key("security.level").is_ok());
    assert!(validate_config_key("unknown.key").is_err());
    assert!(validate_config_key("").is_err());
    assert!(validate_config_key("defaults.agent").is_err());
}

#[test]
fn test_config_value_whitelist() {
    assert!(validate_config_value("security.level", "balanced").is_ok());
    assert!(validate_config_value("security.level", "strict").is_ok());
    assert!(validate_config_value("security.level", "relaxed").is_ok());
    assert!(validate_config_value("security.level", "").is_err());
}

// ============================================================================
// generate_certs_and_secrets() property tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// **Validates: Requirements 3.2, 3.3**
    ///
    /// Property: `generate_certs_and_secrets()` always calls all 5 scripts in
    /// the correct order, regardless of any pre-existing cert file state.
    ///
    /// The Rust function is unconditional — idempotency is handled inside the
    /// scripts themselves. The proptest inputs simulate arbitrary combinations
    /// of pre-existing cert files (present/absent) but the function's behaviour
    /// must be invariant across all of them.
    #[test]
    fn prop_generate_certs_always_calls_5_scripts_in_order(
        // Simulate arbitrary combinations of pre-existing cert files
        // (the Rust function doesn't check these — scripts do)
        _ca_exists in proptest::bool::ANY,
        _valkey_certs_exist in proptest::bool::ANY,
        _valkey_secrets_exist in proptest::bool::ANY,
        _toolbox_certs_exist in proptest::bool::ANY,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let mp = MultipassExecRecorder::new();
            generate_certs_and_secrets(&mp).await.expect("should succeed");
            let calls = mp.recorded_calls();

            // Always exactly 6 calls (5 scripts + 1 logger)
            prop_assert_eq!(calls.len(), 6);

            // Script calls must be in correct order
            let script_names = [
                "generate-ca.sh",
                "generate-certs.sh",
                "generate-secrets.sh",
                "generate-certs.sh",  // toolbox
                "fix-cert-ownership.sh",
            ];
            for (i, name) in script_names.iter().enumerate() {
                let cmd = calls[i].get(3).map_or("", String::as_str);
                prop_assert!(cmd.contains(name), "call {} should contain {}, got: {}", i, name, cmd);
            }

            Ok(())
        })?;
    }

}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// **Validates: Requirements 3.2, 3.3**
    ///
    /// Property: all script paths are rooted at `/opt/polis/` and use
    /// `sudo bash -c` as the invocation pattern.
    #[test]
    fn prop_generate_certs_all_script_paths_rooted_at_opt_polis(
        _any_bool in proptest::bool::ANY,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let mp = MultipassExecRecorder::new();
            generate_certs_and_secrets(&mp).await.expect("should succeed");
            let calls = mp.recorded_calls();

            // All 5 script calls must use sudo bash -c and paths rooted at /opt/polis/
            for (i, call) in calls.iter().take(5).enumerate() {
                prop_assert_eq!(call.first().map(String::as_str), Some("sudo"),
                    "call {} should start with sudo", i);
                prop_assert_eq!(call.get(1).map(String::as_str), Some("bash"),
                    "call {} second arg should be bash", i);
                prop_assert_eq!(call.get(2).map(String::as_str), Some("-c"),
                    "call {} third arg should be -c", i);
                let cmd = call.get(3).map_or("", String::as_str);
                prop_assert!(cmd.starts_with("/opt/polis/"),
                    "call {} path should start with /opt/polis/, got: {}", i, cmd);
            }

            Ok(())
        })?;
    }
}

// ============================================================================
// TokioCommandRunner bounded completion property tests
// ============================================================================

/// Returns a slow command that runs for ~60 seconds on the current platform.
/// On Windows: `timeout /t 60 /nobreak`
/// On Unix: `sleep 60`
#[cfg(windows)]
fn slow_command() -> (&'static str, Vec<&'static str>) {
    ("timeout", vec!["/t", "60", "/nobreak"])
}

#[cfg(not(windows))]
fn slow_command() -> (&'static str, Vec<&'static str>) {
    ("sleep", vec!["60"])
}

// ============================================================================
// TokioCommandRunner output preservation property tests
// ============================================================================

/// Returns a command that echoes the given content to stdout.
/// On Windows: `cmd /C echo <content>`
/// On Unix: `sh -c echo <content>`
#[cfg(windows)]
fn echo_stdout_command(content: &str) -> (&'static str, Vec<String>) {
    ("cmd", vec!["/C".to_string(), format!("echo {content}")])
}

#[cfg(not(windows))]
fn echo_stdout_command(content: &str) -> (&'static str, Vec<String>) {
    ("sh", vec!["-c".to_string(), format!("echo {content}")])
}

/// Returns a command that writes the given content to stderr.
/// On Windows: `cmd /C echo <content> 1>&2`
/// On Unix: `sh -c echo <content> >&2`
#[cfg(windows)]
fn echo_stderr_command(content: &str) -> (&'static str, Vec<String>) {
    (
        "cmd",
        vec!["/C".to_string(), format!("echo {content} 1>&2")],
    )
}

#[cfg(not(windows))]
fn echo_stderr_command(content: &str) -> (&'static str, Vec<String>) {
    ("sh", vec!["-c".to_string(), format!("echo {content} >&2")])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// **Validates: Requirements 1.2, 11.3**
    ///
    /// Property 3: Output Preservation
    ///
    /// For any safe ASCII string (no shell metacharacters), running a command
    /// that echoes that string to stdout must produce an `Output` where:
    ///   1. `stdout` contains the expected content (trimming trailing newline)
    ///   2. `status` indicates success (exit code 0)
    #[test]
    fn prop_output_preservation(
        // Require at least one non-space character to avoid Windows `cmd echo`
        // outputting "ECHO is on." for whitespace-only input.
        content in "[a-zA-Z0-9][a-zA-Z0-9 _-]{0,39}",
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let runner = TokioCommandRunner::new(Duration::from_secs(10));
            let (program, args) = echo_stdout_command(&content);
            let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

            let result = runner.run(program, &args_refs).await;
            prop_assert!(result.is_ok(), "command failed: {:?}", result.err());

            let output = result.unwrap();
            prop_assert!(output.status.success(), "exit status was not success: {:?}", output.status);

            let stdout_str = String::from_utf8_lossy(&output.stdout);
            let stdout_trimmed = stdout_str.trim_end_matches(['\n', '\r']);
            // On Windows, cmd echo adds a trailing space before newline — trim that too
            let stdout_trimmed = stdout_trimmed.trim_end();
            let content_trimmed = content.trim_end();

            prop_assert_eq!(
                stdout_trimmed,
                content_trimmed,
                "stdout mismatch: expected {:?}, got {:?}",
                content_trimmed,
                stdout_trimmed,
            );

            Ok(())
        })?;
    }

    /// **Validates: Requirements 1.2, 11.3**
    ///
    /// Property 3: Output Preservation (stderr)
    ///
    /// For any safe ASCII string (no shell metacharacters), running a command
    /// that writes that string to stderr must produce an `Output` where
    /// `stderr` contains the expected content.
    #[test]
    fn prop_stderr_preservation(
        // Require at least one non-space character to avoid Windows `cmd echo`
        // outputting "ECHO is on." for whitespace-only input.
        content in "[a-zA-Z0-9][a-zA-Z0-9 _-]{0,39}",
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let runner = TokioCommandRunner::new(Duration::from_secs(10));
            let (program, args) = echo_stderr_command(&content);
            let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

            let result = runner.run(program, &args_refs).await;
            prop_assert!(result.is_ok(), "command failed: {:?}", result.err());

            let output = result.unwrap();

            let stderr_str = String::from_utf8_lossy(&output.stderr);
            let stderr_trimmed = stderr_str.trim_end_matches(['\n', '\r']);
            let stderr_trimmed = stderr_trimmed.trim_end();
            let content_trimmed = content.trim_end();

            prop_assert_eq!(
                stderr_trimmed,
                content_trimmed,
                "stderr mismatch: expected {:?}, got {:?}",
                content_trimmed,
                stderr_trimmed,
            );

            Ok(())
        })?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// **Validates: Requirements 1.3, 5.5, 11.1**
    ///
    /// Property 1: Bounded Completion
    ///
    /// For any timeout in [1ms, 500ms], `run_with_timeout` on a slow command
    /// (sleep 60) must:
    ///   1. Return `Err` containing "timed out"
    ///   2. Complete within the timeout + a generous slack (2s) to account for
    ///      process spawn overhead and scheduler jitter
    #[test]
    fn prop_run_with_timeout_always_returns_err_within_bound(
        timeout_ms in 1u64..=500u64,
    ) {
        let timeout = Duration::from_millis(timeout_ms);
        // Allow generous slack for process spawn overhead and scheduler jitter
        let max_wall_time = timeout + Duration::from_secs(2);

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let runner = TokioCommandRunner::new(timeout);
            let (program, args) = slow_command();

            let start = Instant::now();
            let result = runner.run_with_timeout(program, &args, timeout).await;
            let elapsed = start.elapsed();

            // Must return an error
            prop_assert!(result.is_err(), "expected Err but got Ok for timeout={}ms", timeout_ms);

            // Error must mention "timed out"
            let err_msg = format!("{:#}", result.unwrap_err());
            prop_assert!(
                err_msg.contains("timed out"),
                "error does not contain 'timed out': {err_msg}"
            );

            // Must complete within timeout + slack
            prop_assert!(
                elapsed <= max_wall_time,
                "took {}ms, expected <= {}ms (timeout={}ms + 2s slack)",
                elapsed.as_millis(),
                max_wall_time.as_millis(),
                timeout_ms,
            );

            Ok(())
        })?;
    }
}

// ============================================================================
// TokioCommandRunner unit tests
// ============================================================================

/// Returns a simple echo command for the current platform.
/// On Windows: `cmd /C echo hello`
/// On Unix: `sh -c echo hello`
#[cfg(windows)]
fn echo_hello_command() -> (&'static str, Vec<&'static str>) {
    ("cmd", vec!["/C", "echo hello"])
}

#[cfg(not(windows))]
fn echo_hello_command() -> (&'static str, Vec<&'static str>) {
    ("sh", vec!["-c", "echo hello"])
}

/// Returns a slow command for timeout testing (5 seconds).
/// On Windows: `timeout /t 5 /nobreak`
/// On Unix: `sleep 5`
#[cfg(windows)]
fn slow_command_5s() -> (&'static str, Vec<&'static str>) {
    ("timeout", vec!["/t", "5", "/nobreak"])
}

#[cfg(not(windows))]
fn slow_command_5s() -> (&'static str, Vec<&'static str>) {
    ("sleep", vec!["5"])
}

/// Returns a stdin-echo command for the current platform.
/// On Windows: `findstr /R ".*"` (echoes stdin lines to stdout)
/// On Unix: `cat` (echoes stdin to stdout)
#[cfg(windows)]
fn stdin_echo_command() -> (&'static str, Vec<&'static str>) {
    ("findstr", vec!["/R", ".*"])
}

#[cfg(not(windows))]
fn stdin_echo_command() -> (&'static str, Vec<&'static str>) {
    ("cat", vec![])
}

/// Test: `run()` returns correct `Output` when command completes before timeout.
///
/// Requirements: 1.2
#[tokio::test]
async fn test_run_returns_output_on_success() {
    let runner = TokioCommandRunner::new(Duration::from_secs(10));
    let (program, args) = echo_hello_command();

    let result = runner.run(program, &args).await;
    assert!(result.is_ok(), "expected Ok but got: {:?}", result.err());

    let output = result.unwrap();
    assert!(
        output.status.success(),
        "expected success exit status, got: {:?}",
        output.status
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout_trimmed = stdout.trim();
    assert!(
        stdout_trimmed.contains("hello"),
        "expected stdout to contain 'hello', got: {:?}",
        stdout_trimmed
    );
}

/// Test: `run()` returns `Err` containing "timed out" when timeout fires.
///
/// Requirements: 1.3
#[tokio::test]
async fn test_run_returns_timed_out_error_on_timeout() {
    // Use a 100ms timeout against a 5-second command
    let runner = TokioCommandRunner::new(Duration::from_millis(100));
    let (program, args) = slow_command_5s();

    let start = Instant::now();
    let result = runner.run(program, &args).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "expected Err but got Ok");

    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(
        err_msg.contains("timed out"),
        "error does not contain 'timed out': {err_msg}"
    );

    // Should complete well within 2 seconds (100ms timeout + overhead)
    assert!(
        elapsed < Duration::from_secs(2),
        "took too long: {}ms",
        elapsed.as_millis()
    );
}

/// Test: `run_with_stdin()` delivers stdin and returns output.
///
/// Requirements: 1.6, 1.7
#[tokio::test]
async fn test_run_with_stdin_delivers_input_and_returns_output() {
    let runner = TokioCommandRunner::new(Duration::from_secs(10));
    let (program, args) = stdin_echo_command();
    let input = b"hello from stdin\n";

    let result = runner.run_with_stdin(program, &args, input).await;
    assert!(result.is_ok(), "expected Ok but got: {:?}", result.err());

    let output = result.unwrap();
    assert!(
        output.status.success(),
        "expected success exit status, got: {:?}",
        output.status
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello from stdin"),
        "expected stdout to contain 'hello from stdin', got: {:?}",
        stdout
    );
}

/// Test: `spawn()` returns a live child with piped stdin/stdout.
///
/// Requirements: 1.7
#[tokio::test]
async fn test_spawn_returns_live_child_with_piped_io() {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let runner = TokioCommandRunner::new(Duration::from_secs(10));
    let (program, args) = stdin_echo_command();

    let mut child = runner.spawn(program, &args).expect("spawn should succeed");

    // stdin and stdout should be piped
    assert!(child.stdin.is_some(), "expected piped stdin");
    assert!(child.stdout.is_some(), "expected piped stdout");

    // Write to stdin and read from stdout
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    stdin.write_all(b"ping\n").await.expect("write to stdin");
    drop(stdin); // close stdin so the child can exit

    let mut buf = Vec::new();
    stdout
        .read_to_end(&mut buf)
        .await
        .expect("read from stdout");

    let output = String::from_utf8_lossy(&buf);
    assert!(
        output.contains("ping"),
        "expected stdout to contain 'ping', got: {:?}",
        output
    );

    let status = child.wait().await.expect("wait for child");
    assert!(status.success(), "expected success exit status");
}

/// Test: spawn failure returns `Err` with "failed to spawn {program}".
///
/// Requirements: 1.9
#[tokio::test]
async fn test_spawn_failure_returns_err_with_program_name() {
    let runner = TokioCommandRunner::new(Duration::from_secs(10));
    let program = "__nonexistent_program_xyz__";

    let result = runner.spawn(program, &[]);
    assert!(result.is_err(), "expected Err for nonexistent program");

    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(
        err_msg.contains("failed to spawn"),
        "error does not contain 'failed to spawn': {err_msg}"
    );
    assert!(
        err_msg.contains(program),
        "error does not contain program name '{program}': {err_msg}"
    );
}
