# Rust CLI Testing Guide

Comprehensive reference for writing unit, integration, property-based, and snapshot tests for Polis CLI and future Rust CLI applications.

---

## Table of Contents

- [Philosophy & Test Pyramid](#philosophy--test-pyramid)
- [Project Layout](#project-layout)
- [Frameworks & Crates](#frameworks--crates)
- [Unit Testing](#unit-testing)
- [Integration Testing](#integration-testing)
- [Snapshot Testing](#snapshot-testing)
- [Property-Based Testing](#property-based-testing)
- [Fuzz Testing](#fuzz-testing)
- [Test Coverage](#test-coverage)
- [Test Runners](#test-runners)
- [Common Problems & Solutions](#common-problems--solutions)
- [CI Integration](#ci-integration)
- [Polis-Specific Patterns](#polis-specific-patterns)
- [Quick Reference](#quick-reference)

---

## Philosophy & Test Pyramid

```
        ╱ E2E ╲            Slow, expensive, few
       ╱────────╲
      ╱Integration╲        assert_cmd, real binary
     ╱──────────────╲
    ╱  Unit + Property ╲    Fast, isolated, many
   ╱────────────────────╲
```

**Guiding principles:**

1. **70% unit tests** — fast, deterministic, test logic via trait mocks
2. **20% integration tests** — spawn the real binary with `assert_cmd`, verify CLI contract
3. **10% E2E / snapshot tests** — full workflows, catch regressions in output format

Rust's compiler eliminates entire classes of bugs (null, data races, use-after-free). Tests focus on **logic errors**, **business rules**, and **CLI contract** — not memory safety. `#[cfg(test)]` keeps test code out of production binaries.

---

## Project Layout

```
cli/
├── src/
│   ├── main.rs              # Binary entry point
│   ├── lib.rs               # Library root — exposes modules for test access
│   ├── cli.rs               # clap derive definitions
│   ├── multipass.rs          # Trait + production impl (DI boundary)
│   ├── state.rs              # StateManager::with_path() for test isolation
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── start.rs          # Accepts &dyn Multipass — testable
│   │   └── ...
│   └── output/
├── tests/
│   ├── unit/
│   │   ├── main.rs           # [[test]] binary — uses library, mocked deps
│   │   └── status_command.rs
│   └── integration/
│       ├── main.rs           # [[test]] binary — spawns real polis binary
│       └── cli_tests.rs
└── Cargo.toml                # Separate [[test]] entries for unit + integration
```

Separate `[[test]]` binaries in `Cargo.toml`:

```toml
[[test]]
name = "unit"
path = "tests/unit/main.rs"

[[test]]
name = "integration"
path = "tests/integration/main.rs"
```

Run independently:

```bash
cargo test --test unit          # fast, no I/O
cargo test --test integration   # spawns binary, slower
```

---

## Frameworks & Crates

| Crate | Purpose | When to Use |
|-------|---------|-------------|
| `assert_cmd` | Spawn CLI binary, assert exit/stdout/stderr | Integration tests |
| `predicates` | Composable assertions (contains, regex, etc.) | With assert_cmd |
| `tempfile` | Isolated temp directories, auto-cleanup | State/config tests |
| `mockall` | Auto-generate mock structs from traits | Complex trait mocking |
| `proptest` | Property-based testing with shrinking | Argument parsing, validation |
| `insta` | Snapshot testing with review workflow | Output format regression |
| `insta-cmd` | Snapshot testing for CLI command output | CLI output regression |
| `trycmd` | Bulk CLI snapshot tests from `.toml`/`.md` files | Large command suites |
| `assert_fs` | Filesystem assertions and fixtures | File I/O testing |
| `cargo-nextest` | Faster parallel test runner | CI and local dev |
| `cargo-tarpaulin` | Code coverage (Linux) | Coverage reports |
| `cargo-llvm-cov` | LLVM-based code coverage | Precise coverage |
| `cargo-fuzz` | Fuzz testing with libFuzzer | Security, parser robustness |

```toml
[dev-dependencies]
assert_cmd = "2.1"
predicates = "3.1"
tempfile = "3.25"
mockall = "0.13"
proptest = "1.6"
insta = { version = "1.42", features = ["yaml"] }
insta-cmd = "0.6"
```

---

## Unit Testing

### Basics

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_state_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let mgr = StateManager::with_path(dir.path().join("state.json"));

        let state = WorkspaceState {
            workspace_id: "polis-abc123".into(),
            created_at: Utc::now(),
            image_sha256: Some("deadbeef".into()),
            image_source: None,
        };

        mgr.save(&state).unwrap();
        let loaded = mgr.load().unwrap().unwrap();
        assert_eq!(loaded.workspace_id, "polis-abc123");
    }
}
```

**Naming convention:** `test_<unit>_<scenario>_<expected>` — e.g., `test_status_no_workspace_returns_ok`.

Use `-> anyhow::Result<()>` for tests with `?` instead of `#[should_panic]`:

```rust
#[test]
fn test_parse_config_rejects_invalid_yaml() -> anyhow::Result<()> {
    let result = parse_config("not: [valid: yaml");
    assert!(result.is_err());
    Ok(())
}
```

### Trait-Based Dependency Injection

The **core pattern** for testable Rust CLI code. Define behavior as a trait, depend on the trait, swap in mocks during tests.

```rust
// 1. Define the trait (behavior contract)
pub trait Multipass {
    fn vm_info(&self) -> Result<Output>;
    fn start(&self) -> Result<Output>;
    fn exec(&self, args: &[&str]) -> Result<Output>;
}

// 2. Production implementation shells out to the real binary
pub struct MultipassCli;
impl Multipass for MultipassCli {
    fn vm_info(&self) -> Result<Output> {
        Command::new("multipass")
            .args(["info", "polis", "--format", "json"])
            .output()
            .context("failed to run multipass info")
    }
    // ...
}

// 3. Command functions accept the trait, not the concrete type
pub async fn run(ctx: &OutputContext, json: bool, mp: &dyn Multipass) -> Result<()> {
    let info = mp.vm_info()?;
    // ... business logic
}
```

**When to use which approach:**

| Situation | Approach |
|-----------|----------|
| External process (multipass, docker, git) | Trait wrapping `Command` |
| Filesystem state (config, state files) | `with_path()` constructor + `tempfile` |
| HTTP clients | Trait or `mockito`/`wiremock` |
| Time-dependent logic | Trait wrapping `Utc::now()` |
| Pure functions | No mocking needed — test directly |

### Manual Mock Structs

For simple cases, hand-written mocks are clearer than auto-generated ones:

```rust
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

fn ok_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

fn err_output(code: i32, stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: stderr.to_vec(),
    }
}

struct MockNotFound;
impl Multipass for MockNotFound {
    fn vm_info(&self) -> Result<Output> {
        Ok(err_output(1, b"instance \"polis\" does not exist"))
    }
    fn start(&self) -> Result<Output> { anyhow::bail!("not expected") }
    fn exec(&self, _: &[&str]) -> Result<Output> { Ok(err_output(1, b"")) }
    // ... other methods bail with "not expected"
}

#[tokio::test]
async fn test_status_no_workspace_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = status::run(&ctx, false, &MockNotFound).await;
    assert!(result.is_ok());
}
```

**Tips:**
- `anyhow::bail!("not expected")` in unused methods catches unexpected interactions
- Name mocks after the scenario: `MockNotFound`, `MockStopped`, `MockRunning`
- Use manual mocks for traits with ≤5 methods

### Auto-Mocking with mockall

For large traits or when you need call-count verification:

```rust
use mockall::automock;

#[automock]
pub trait Multipass {
    fn vm_info(&self) -> Result<Output>;
    fn start(&self) -> Result<Output>;
    fn launch(&self, image: &str, cpus: &str, mem: &str, disk: &str) -> Result<Output>;
    fn transfer(&self, local: &str, remote: &str) -> Result<Output>;
    fn exec(&self, args: &[&str]) -> Result<Output>;
    fn version(&self) -> Result<Output>;
}

#[test]
fn test_start_calls_launch_when_no_vm() {
    let mut mock = MockMultipass::new();
    mock.expect_vm_info()
        .times(1)
        .returning(|| Ok(err_output(1, b"does not exist")));
    mock.expect_launch()
        .times(1)
        .returning(|_, _, _, _| Ok(ok_output(b"Launched")));

    let result = start::run(&args, &mock, false);
    assert!(result.is_ok());
    // mockall verifies expectations on drop
}
```

**Rule of thumb:** Manual mocks for simple traits, `mockall` when you need call verification or the trait is large.

### Testing Async Code

```rust
#[tokio::test]
async fn test_status_running_shows_services() {
    let ctx = OutputContext::new(true, true);
    let result = status::run(&ctx, false, &MockRunning).await;
    assert!(result.is_ok());
}
```

**Pitfalls:**
- `#[tokio::test]` creates a single-threaded runtime by default; use `#[tokio::test(flavor = "multi_thread")]` if needed
- Don't mix `#[test]` with `.block_on()` — use `#[tokio::test]` consistently
- For timeouts: `tokio::time::timeout(Duration::from_secs(5), fut).await`

### Filesystem Isolation with tempfile

Never write to real paths in tests:

```rust
#[test]
fn test_state_save_and_load() {
    let dir = tempfile::TempDir::new().unwrap();
    let mgr = StateManager::with_path(dir.path().join("state.json"));

    mgr.save(&some_state).unwrap();
    let loaded = mgr.load().unwrap().unwrap();
    assert_eq!(loaded.workspace_id, "test-123");
    // TempDir auto-deletes on drop
}
```

**Important:** Bind `TempDir` to a named variable (not `_`) so it lives for the test's duration.

---

## Integration Testing

Integration tests spawn the real compiled binary and verify behavior from the outside.

### assert_cmd + predicates

```rust
use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

#[test]
fn test_version_command() {
    polis()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("polis 0.1.0"));
}

#[test]
fn test_no_args_shows_help() {
    polis()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn test_unknown_command_fails() {
    polis()
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
```

### Predicate Combinators

```rust
// Contains substring
predicate::str::contains("error")

// Regex match
predicate::str::is_match(r"^polis \d+\.\d+\.\d+\n$").unwrap()

// Negation — hidden commands should NOT appear in help
predicate::str::contains("_ssh-proxy").not()

// Multiple conditions
predicate::str::contains("Usage:").and(predicate::str::contains("Commands:"))

// Empty output
predicate::str::is_empty()
```

### Environment Isolation

```rust
#[test]
fn test_config_show_with_clean_env() {
    let dir = tempfile::TempDir::new().unwrap();
    polis()
        .args(["config", "show"])
        .env("HOME", dir.path())       // isolate config
        .env("NO_COLOR", "1")          // disable ANSI codes
        .env_remove("POLIS_CONFIG")    // remove overrides
        .assert()
        .success();
}
```

**Always set in integration tests:**
- `NO_COLOR=1` — prevents ANSI escape codes from polluting assertions
- `HOME` to a temp dir — prevents reading/writing real user config

### Helper Functions

Keep helpers focused and scenario-specific:

```rust
fn polis_isolated() -> Command {
    let mut cmd = polis();
    cmd.env("NO_COLOR", "1");
    cmd
}

fn assert_fails_with(args: &[&str], expected_error: &str) {
    polis_isolated()
        .args(args)
        .assert()
        .failure()
        .stderr(predicate::str::contains(expected_error));
}
```

**Anti-pattern:** Don't create a single mega-helper that hides what's being tested. Each test should clearly show setup, action, and assertion.

---

## Snapshot Testing

Snapshot tests capture output and compare against a stored reference. When output changes, you review and accept or reject the diff.

### insta + insta-cmd

```rust
use insta_cmd::{assert_cmd_snapshot, get_cargo_bin};
use std::process::Command;

fn cli() -> Command {
    Command::new(get_cargo_bin("polis"))
}

#[test]
fn test_version_snapshot() {
    assert_cmd_snapshot!(cli().arg("version"), @r"
    polis 0.1.0
    ");
}

#[test]
fn test_help_snapshot() {
    assert_cmd_snapshot!(cli().arg("--help"));
    // First run creates snapshots/test_help_snapshot.snap
    // Subsequent runs compare against it
}
```

**Workflow:**

```bash
cargo install cargo-insta          # one-time setup
cargo test                         # tests fail on first run (no snapshots yet)
cargo insta review                 # interactive review: accept/reject each diff
cargo test                         # now passes
```

**When to use snapshots:**
- Help text and version output (catch unintended changes)
- JSON output format (structural regression)
- Error messages (consistent user experience)
- Doctor command output (complex multi-line output)

### trycmd for Bulk CLI Tests

`trycmd` lets you define CLI tests as `.toml` or `.md` files — great for large command suites:

```rust
// tests/cli_snapshots.rs
#[test]
fn cli_tests() {
    trycmd::TestCases::new().case("tests/cmd/*.toml");
}
```

```toml
# tests/cmd/version.toml
bin.name = "polis"
args = ["version"]
status.code = 0
stdout = "polis 0.1.0\n"
```

```toml
# tests/cmd/unknown-command.toml
bin.name = "polis"
args = ["nonexistent"]
status.code = 2
stderr.contains = ["error"]
```

**Advantages:** Test cases are data, not code. Easy to add new cases. Can be embedded in documentation with mdbook.

---

## Property-Based Testing

Property-based testing generates random inputs and verifies invariants hold. `proptest` automatically shrinks failing cases to minimal reproductions.

### Testing Argument Parsing

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_config_set_rejects_empty_key(value in "\\PC+") {
        // Empty key should always be rejected
        let result = validate_config_key("");
        prop_assert!(result.is_err());
    }

    #[test]
    fn test_config_key_roundtrip(key in "[a-z][a-z0-9_.]{0,30}", value in "\\PC{0,100}") {
        let dir = tempfile::TempDir::new().unwrap();
        let mgr = ConfigManager::with_path(dir.path().join("config.yaml"));
        // If set succeeds, get should return the same value
        if mgr.set(&key, &value).is_ok() {
            let retrieved = mgr.get(&key).unwrap();
            prop_assert_eq!(retrieved.as_deref(), Some(value.as_str()));
        }
    }
}
```

### Testing Data Serialization

```rust
proptest! {
    #[test]
    fn test_workspace_state_serde_roundtrip(
        id in "[a-z0-9-]{1,50}",
        sha in proptest::option::of("[0-9a-f]{64}")
    ) {
        let state = WorkspaceState {
            workspace_id: id.clone(),
            created_at: Utc::now(),
            image_sha256: sha,
            image_source: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: WorkspaceState = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(parsed.workspace_id, id);
    }
}
```

### Custom Strategies

```rust
fn arb_workspace_state() -> impl Strategy<Value = WorkspaceState> {
    (
        "[a-z0-9-]{1,50}",
        proptest::option::of("[0-9a-f]{64}"),
    ).prop_map(|(id, sha)| WorkspaceState {
        workspace_id: id,
        created_at: Utc::now(),
        image_sha256: sha,
        image_source: None,
    })
}

proptest! {
    #[test]
    fn test_state_persistence(state in arb_workspace_state()) {
        let dir = tempfile::TempDir::new().unwrap();
        let mgr = StateManager::with_path(dir.path().join("state.json"));
        mgr.save(&state).unwrap();
        let loaded = mgr.load().unwrap().unwrap();
        prop_assert_eq!(loaded.workspace_id, state.workspace_id);
    }
}
```

**What to property-test in a CLI:**
- Config key/value parsing and roundtrips
- Serialization/deserialization of state files
- Argument validation (valid inputs accepted, invalid rejected)
- Version string parsing

---

## Fuzz Testing

Fuzz testing feeds random/mutated bytes to find crashes, panics, and undefined behavior. Use `cargo-fuzz` with libFuzzer.

### Setup

```bash
cargo install cargo-fuzz
cargo fuzz init                    # creates fuzz/ directory
cargo fuzz add parse_config        # creates a fuzz target
```

### Writing a Fuzz Target

```rust
// fuzz/fuzz_targets/parse_config.rs
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Should never panic, regardless of input
        let _ = polis_cli::config::parse_config(s);
    }
});
```

### Structure-Aware Fuzzing with Arbitrary

```rust
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    key: String,
    value: String,
}

fuzz_target!(|input: FuzzInput| {
    let _ = validate_config_key(&input.key);
});
```

```bash
cargo fuzz run parse_config -- -max_total_time=300   # run for 5 minutes
```

**What to fuzz in a CLI:**
- Config file parsing (YAML, TOML, JSON)
- Argument parsing edge cases
- Any function that processes untrusted input (update manifests, signatures)

---

## Test Coverage

### cargo-tarpaulin (Linux)

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --test unit --out html     # unit test coverage
cargo tarpaulin --out lcov                 # for CI upload
```

### cargo-llvm-cov (cross-platform, more accurate)

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --test unit --html          # HTML report
cargo llvm-cov --test unit --lcov --output-path lcov.info   # for CI
```

**Coverage targets:**
- Aim for **80%+ line coverage** on business logic
- Don't chase 100% — focus on critical paths and error handling
- Exclude generated code and trivial getters from coverage metrics

---

## Test Runners

### cargo test (built-in)

```bash
cargo test                              # all tests
cargo test --test unit                  # only unit tests
cargo test --test integration           # only integration tests
cargo test test_status                  # filter by name
cargo test -- --test-threads=1          # sequential (for tests sharing resources)
cargo test -- --nocapture               # show println! output
cargo test -- --ignored                 # run #[ignore] tests
```

### cargo-nextest (recommended for CI)

Faster parallel execution, better output, per-test timeouts:

```bash
cargo install cargo-nextest
cargo nextest run                       # all tests
cargo nextest run --test unit           # only unit tests
cargo nextest run -E 'test(status)'     # filter expression
cargo nextest run --retries 2           # retry flaky tests
```

Configure in `.config/nextest.toml`:

```toml
[profile.default]
retries = 0
slow-timeout = { period = "30s", terminate-after = 2 }
fail-fast = false

[profile.ci]
retries = 2
fail-fast = true
```

---

## Common Problems & Solutions

### 1. Colored Output Breaks Assertions

**Problem:** CLI uses `owo-colors`/`console`/`indicatif` — ANSI escape codes appear in test output.

**Solution:**
```rust
// Integration tests: disable color via env
polis().env("NO_COLOR", "1").arg("status").assert().success();

// Unit tests: use OutputContext with no_color=true
let ctx = OutputContext::new(/*no_color=*/ true, /*quiet=*/ false);
```

### 2. Tests Interfere with Each Other (Shared State)

**Problem:** Tests read/write `~/.polis/state.json` and step on each other.

**Solution:** Always use isolated paths:
```rust
let dir = tempfile::TempDir::new().unwrap();
let mgr = StateManager::with_path(dir.path().join("state.json"));
```

For integration tests, set `HOME`:
```rust
polis().env("HOME", dir.path()).arg("config").arg("show").assert().success();
```

### 3. Flaky Async Tests

**Problem:** Tests pass locally but fail in CI due to timing.

**Solution:**
- Use `tokio::time::timeout()` with generous limits
- Never `sleep()` to wait for conditions — use channels or condition variables
- Run with `--test-threads=1` to isolate if needed, then fix the root cause

### 4. Testing Code That Calls `std::process::Command`

**Problem:** Can't unit-test functions that shell out to `multipass`, `docker`, etc.

**Solution:** Wrap external commands behind a trait (the Multipass pattern):
```rust
pub trait Multipass {
    fn vm_info(&self) -> Result<Output>;
    // ...
}

// Production: shells out to multipass binary
// Test: returns canned Output structs
```

### 5. `ExitStatus` Construction in Tests

**Problem:** `ExitStatus` has no public constructor on all platforms.

**Solution (Unix):**
```rust
use std::os::unix::process::ExitStatusExt;

// Exit code 0 (success)
ExitStatus::from_raw(0)

// Exit code 1 (the raw value is code << 8 on Unix)
ExitStatus::from_raw(1 << 8)
```

### 6. Testing Error Messages with anyhow

**Problem:** `anyhow::Error` doesn't implement `PartialEq`.

**Solution:** Convert to string and assert on the message:
```rust
let err = some_function().unwrap_err();
assert!(err.to_string().contains("not found"));

// Or with the chain:
let root = err.root_cause().to_string();
assert!(root.contains("connection refused"));
```

### 7. Tests That Need stdin Input

**Problem:** Commands like `delete` prompt for confirmation.

**Solution:**
```rust
// Integration: pipe input
polis()
    .args(["delete", "--all"])
    .write_stdin("y\n")
    .assert()
    .success();

// Or use --yes flag to skip prompts
polis()
    .args(["delete", "--all", "--yes"])
    .assert()
    .success();
```

### 8. Slow Integration Tests

**Problem:** Each integration test compiles and spawns the binary.

**Solution:**
- The binary is compiled once per `cargo test` run — the spawn overhead is minimal
- Use `#[ignore]` for tests requiring real infrastructure (VMs, network)
- Run unit and integration tests separately in CI for faster feedback
- Use `cargo-nextest` for parallel execution

### 9. Testing JSON Output

**Problem:** Need to verify structured JSON output without brittle string matching.

**Solution:**
```rust
#[test]
fn test_version_json_structure() {
    let output = polis()
        .args(["version", "--json"])
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["version"], "0.1.0");
    assert!(json.get("version").is_some());
}
```

### 10. Mocking Trait Methods That Return References

**Problem:** `mockall` struggles with methods returning `&str` or `&T`.

**Solution:** Design traits to return owned types (`String`, `Vec<T>`) instead of references. If you must return references, use `mockall`'s `#[automock]` with explicit lifetime annotations, or switch to manual mocks.

---

## CI Integration

### GitHub Actions Example

```yaml
name: Tests
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@nextest

      - name: Unit tests
        run: cargo nextest run --test unit

      - name: Integration tests
        run: cargo nextest run --test integration

      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings

      - name: Coverage
        run: |
          cargo install cargo-llvm-cov
          cargo llvm-cov --test unit --lcov --output-path lcov.info

      - name: Upload coverage
        uses: codecov/codecov-action@v4
        with:
          files: lcov.info
```

### Test Ordering in CI

1. `cargo clippy` — catch lint issues first (fastest)
2. `cargo test --test unit` — fast unit tests
3. `cargo test --test integration` — slower binary tests
4. `cargo test -- --ignored` — infrastructure-dependent tests (optional)
5. Coverage report — run last, uploads to dashboard

---

## Polis-Specific Patterns

### The Multipass Trait Pattern

All commands that interact with the VM accept `&dyn Multipass`, making them fully testable without a real Multipass installation:

```rust
// src/cli.rs — production wiring
Command::Start(args) => {
    let mp = crate::multipass::MultipassCli;  // real impl
    commands::start::run(&args, &mp, quiet)
}

// tests/unit/start_stop_delete.rs — test wiring
struct MockMultipass { /* canned responses */ }
impl Multipass for MockMultipass { /* ... */ }

#[test]
fn test_start_creates_vm() {
    let mock = MockMultipass;
    let result = start::run(&args, &mock, false);
    assert!(result.is_ok());
}
```

### StateManager Test Isolation

`StateManager::with_path()` exists specifically for tests:

```rust
// Production: reads ~/.polis/state.json
let mgr = StateManager::new()?;

// Test: reads from isolated temp directory
let dir = tempfile::TempDir::new().unwrap();
let mgr = StateManager::with_path(dir.path().join("state.json"));
```

### OutputContext for Controlled Rendering

Suppress colors and progress bars in tests:

```rust
let ctx = OutputContext::new(
    true,   // no_color — disable ANSI
    true,   // quiet — suppress non-error output
);
let result = status::run(&ctx, /*json=*/ false, &mock).await;
```

### Testing New Commands Checklist

When adding a new command to Polis CLI:

1. **Accept traits for external dependencies** — don't call `Command::new()` directly in business logic
2. **Accept `&OutputContext`** — so tests can suppress colors/progress
3. **Use `StateManager::with_path()`** — for any state file access
4. **Write unit tests first** — mock all I/O boundaries
5. **Add integration tests** — spawn binary, verify exit code + output
6. **Add snapshot tests** — for help text and structured output

### Test File Organization

```
tests/unit/main.rs          — mod declarations for all unit test files
tests/unit/status_command.rs — unit tests for status command
tests/unit/start_stop_delete.rs — unit tests for lifecycle commands
tests/unit/doctor_command.rs — unit tests for doctor command
tests/unit/output.rs         — unit tests for output formatting

tests/integration/main.rs          — mod declarations
tests/integration/cli_tests.rs     — CLI structure and arg parsing
tests/integration/config_command.rs — config subcommand tests
tests/integration/update_command.rs — update command tests
```

---

## Quick Reference

### Running Tests

```bash
cargo test                          # everything
cargo test --test unit              # unit only
cargo test --test integration       # integration only
cargo test test_status              # filter by name
cargo nextest run                   # parallel runner
cargo nextest run -E 'test(status)' # nextest filter
```

### Writing a Unit Test

```rust
#[tokio::test]
async fn test_<command>_<scenario>_<expected>() {
    let ctx = OutputContext::new(true, true);
    let mock = Mock<Scenario>;
    let result = <command>::run(&ctx, &mock).await;
    assert!(result.is_ok());
}
```

### Writing an Integration Test

```rust
#[test]
fn test_<command>_<scenario>() {
    polis()
        .args(["<command>", "<args>"])
        .env("NO_COLOR", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("<expected>"));
}
```

### Adding a New Mock

```rust
struct Mock<Scenario>;
impl Multipass for Mock<Scenario> {
    fn vm_info(&self) -> Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: br#"{"info":{"polis":{"state":"Running"}}}"#.to_vec(),
            stderr: Vec::new(),
        })
    }
    // ... other methods: bail!("not expected") for unused ones
}
```

### Coverage

```bash
cargo llvm-cov --test unit --html              # HTML report
cargo tarpaulin --test unit --out html         # alternative
```

### Snapshot Review

```bash
cargo insta test                    # run tests, generate pending snapshots
cargo insta review                  # interactive accept/reject
```
