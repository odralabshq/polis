# Rust CLI Coding Standards & Best Practices

> Comprehensive guide for writing high-quality, high-performance, and secure Rust code for the Polis CLI and future CLI applications.

---

## Table of Contents

1. [Philosophy & Principles](#1-philosophy--principles)
2. [Project Structure](#2-project-structure)
3. [CLI Architecture with Clap](#3-cli-architecture-with-clap)
4. [Error Handling](#4-error-handling)
5. [Async Runtime & Tokio](#5-async-runtime--tokio)
6. [Output & User Experience](#6-output--user-experience)
7. [Configuration Management](#7-configuration-management)
8. [State Management](#8-state-management)
9. [Security](#9-security)
10. [Testing](#10-testing)
11. [Performance](#11-performance)
12. [Logging & Diagnostics](#12-logging--diagnostics)
13. [Signal Handling & Graceful Shutdown](#13-signal-handling--graceful-shutdown)
14. [Dependency Management](#14-dependency-management)
15. [CI/CD & Tooling](#15-cicd--tooling)
16. [Common Pitfalls](#16-common-pitfalls)
17. [Recommended Crate Ecosystem](#17-recommended-crate-ecosystem)

---

## 1. Philosophy & Principles

- **Correctness first, then performance.** Rust's type system is your primary tool — use it to make illegal states unrepresentable.
- **No `unsafe`.** Polis denies `unsafe_code` at the workspace level. This is non-negotiable for CLI code.
- **Errors are values, not panics.** Every fallible operation returns `Result`. Never `unwrap()` or `expect()` in production paths.
- **Explicit over implicit.** Prefer clear, readable code over clever abstractions. A new contributor should understand any module in under 5 minutes.
- **Minimal dependencies.** Every crate you add is attack surface. Justify each dependency.

---

## 2. Project Structure

```
cli/
├── Cargo.toml
├── src/
│   ├── main.rs              # Entry point — parse args, delegate, handle exit
│   ├── lib.rs               # Public modules for integration testing
│   ├── cli.rs               # Clap derive structs (Cli, Command enum)
│   ├── commands/
│   │   ├── mod.rs           # Shared command types (e.g., DeleteArgs)
│   │   ├── start.rs         # One file per command
│   │   ├── stop.rs
│   │   ├── status.rs
│   │   ├── doctor.rs
│   │   └── ...
│   ├── output/
│   │   ├── mod.rs           # OutputContext — centralized output formatting
│   │   ├── styles.rs        # owo-colors stylesheet
│   │   ├── progress.rs      # indicatif progress bars
│   │   └── json.rs          # JSON output helpers
│   ├── workspace/
│   │   ├── mod.rs           # Workspace lifecycle
│   │   ├── image.rs         # Image download, verification, caching
│   │   ├── vm.rs            # VM lifecycle operations
│   │   └── health.rs        # Health checks and readiness
│   ├── multipass.rs          # Trait-based CLI abstraction
│   ├── state.rs              # Workspace state persistence
│   └── ssh.rs                # SSH operations
├── tests/
│   ├── integration/          # Integration tests (binary-level)
│   │   ├── main.rs
│   │   └── *.rs
│   └── unit/                 # Unit tests (module-level)
│       ├── main.rs
│       └── *.rs
```

### Key conventions

- **One command per file** in `commands/`. Each file exports a `run()` function.
- **Trait abstractions** for external tools (see `multipass.rs`). This enables testing without real infrastructure.
- **`lib.rs` re-exports** modules needed by integration tests. Keep `main.rs` minimal.
- **Separate test binaries** for unit and integration tests via `[[test]]` sections in `Cargo.toml`.

---

## 3. CLI Architecture with Clap

Use clap's derive API for argument parsing. It provides compile-time validation, auto-generated help, and shell completions.

### Argument struct pattern

```rust
use clap::{Parser, Subcommand, Args};

#[derive(Parser)]
#[command(
    name = "polis",
    version,
    propagate_version = true,
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    /// Output in JSON format
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}
```

### Rules

- **Global flags** (`--json`, `--quiet`, `--no-color`) go on the root struct with `global = true`.
- **Subcommand-specific args** use separate `Args` structs.
- **Hidden commands** for internal use: `#[command(hide = true, name = "_ssh-proxy")]`.
- **Environment variable fallback**: `#[arg(env = "POLIS_LOG_LEVEL")]` for config that can come from env.
- **Validation**: Use clap's `value_parser` for type-safe argument validation at parse time.

### Command dispatch

Keep `main.rs` thin — parse and delegate:

```rust
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = cli.run().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
```

Each command's `run()` function receives only what it needs (args, trait objects, output context) — never the entire `Cli` struct.

---

## 4. Error Handling

### Strategy: `anyhow` for applications, `thiserror` for libraries

| Layer | Crate | Why |
|-------|-------|-----|
| CLI commands | `anyhow` | Rapid error propagation with context |
| Shared libraries (`polis-common`) | `thiserror` | Typed errors consumers can match on |

### The `?` operator and context

Always add context when propagating errors across boundaries:

```rust
use anyhow::{Context, Result};

fn load_config() -> Result<Config> {
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config from {}", path.display()))?;
    serde_yaml::from_str(&data)
        .context("failed to parse config YAML")
}
```

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | General error |
| `2` | Usage/argument error (clap handles this) |

```rust
if let Err(e) = cli.run().await {
    eprintln!("Error: {e}");
    std::process::exit(1);
}
```

### Rules

- **Never `unwrap()` or `expect()` in production code.** Cargo.toml enforces `clippy::unwrap_used` and `clippy::expect_used` as warnings. Tests may use `expect()` via `#![cfg_attr(test, allow(clippy::expect_used))]`.
- **Never `panic!()` in library code.** Return `Result` instead.
- **Error messages are for humans.** Write lowercase, no trailing period: `"failed to connect to VM"`, not `"Failed to connect to VM."`.
- **Chain context.** The final error message should read like a stack trace: `"failed to start workspace: failed to launch VM: multipass not found"`.

---

## 5. Async Runtime & Tokio

### When to use async

| Use async | Use sync |
|-----------|----------|
| Network I/O (HTTP, SSH) | CPU-bound computation |
| Concurrent operations (parallel health checks) | Simple file reads |
| Long-running operations with cancellation | Quick subprocess calls |

### Tokio configuration

```rust
#[tokio::main]  // defaults to multi-thread runtime
async fn main() { ... }
```

For CLI apps, the multi-thread runtime is fine. If you need minimal overhead for a simple tool, use `#[tokio::main(flavor = "current_thread")]`.

### Patterns

**Concurrent operations with `tokio::join!`:**

```rust
let (status, health) = tokio::join!(
    check_vm_status(&mp),
    check_health(&mp),
);
```

**Timeouts:**

```rust
use tokio::time::{timeout, Duration};

let result = timeout(Duration::from_secs(30), long_operation()).await
    .context("operation timed out")?;
```

**`tokio::select!` for racing operations:**

```rust
tokio::select! {
    result = operation() => handle_result(result),
    _ = tokio::signal::ctrl_c() => {
        eprintln!("Interrupted");
        std::process::exit(130);
    }
}
```

### Rules

- **Don't block the async runtime.** Use `tokio::task::spawn_blocking` for CPU-heavy or blocking I/O work.
- **Prefer `tokio::process::Command`** over `std::process::Command` when inside async context and you need non-blocking execution.
- **Set timeouts on all network operations.** No operation should hang indefinitely.

---

## 6. Output & User Experience

### The `OutputContext` pattern

Centralize all output decisions in a single struct:

```rust
pub struct OutputContext {
    pub styles: Styles,
    pub is_tty: bool,
    pub quiet: bool,
}
```

This struct is passed to commands that produce user-facing output. It handles:
- Color support detection (TTY + `NO_COLOR` env var)
- Quiet mode suppression
- Consistent formatting (success ✓, warning ⚠, error ✗, info ℹ)

### Color handling

Use `owo-colors` with a stylesheet pattern:

```rust
pub struct Styles {
    pub success: Style,  // green
    pub warning: Style,  // yellow
    pub error: Style,    // red
    pub info: Style,     // blue
    pub dim: Style,      // dimmed
    pub header: Style,   // bold cyan
}
```

Respect the `NO_COLOR` environment variable (see [no-color.org](https://no-color.org)):

```rust
let no_color = no_color_flag || std::env::var("NO_COLOR").is_ok();
```

### Progress indicators

Use `indicatif` for long-running operations:

```rust
use indicatif::{ProgressBar, ProgressStyle};

let pb = ProgressBar::new(total_size);
pb.set_style(ProgressStyle::default_bar()
    .template("{spinner:.green} [{bar:40}] {bytes}/{total_bytes} ({eta})")?);
```

Only show progress bars when `OutputContext::show_progress()` returns true (TTY and not quiet).

### JSON output mode

Every command that produces structured output must support `--json`:

```rust
if json {
    println!("{}", serde_json::to_string_pretty(&status)?);
} else {
    ctx.header("Workspace Status");
    ctx.kv("State", &status.state);
}
```

### Rules

- **Errors always go to stderr** (`eprintln!`), never suppressed by `--quiet`.
- **Structured data goes to stdout** so it can be piped.
- **Interactive prompts** (via `dialoguer`) only when TTY is detected. Provide `--yes` flags for non-interactive use.
- **No raw `println!` in commands.** Use `OutputContext` methods.

---

## 7. Configuration Management

### File locations

Follow platform conventions using the `dirs` crate:

| Purpose | Path | Crate |
|---------|------|-------|
| Config | `~/.polis/config.yaml` | `dirs::home_dir()` |
| State | `~/.polis/state.json` | `dirs::home_dir()` |
| Cache | `~/.polis/cache/` | `dirs::home_dir()` |

### Layered configuration

Priority order (highest wins):
1. CLI flags
2. Environment variables (`POLIS_*`)
3. Config file (`~/.polis/config.yaml`)
4. Defaults

### Serialization

Use `serde` with `serde_yaml` for config files and `serde_json` for state:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
}
```

### Rules

- **Use `#[serde(default)]`** for optional fields with sensible defaults.
- **Use `#[serde(alias = "old_name")]`** for backward compatibility when renaming fields.
- **Use `#[serde(skip_serializing_if = "Option::is_none")]`** to keep config files clean.
- **Never store secrets in config files.** Use environment variables or a dedicated secrets manager.

---

## 8. State Management

### The `StateManager` pattern

Encapsulate file-based state behind a manager struct:

```rust
pub struct StateManager {
    path: PathBuf,
}

impl StateManager {
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        Ok(Self::with_path(home.join(".polis").join("state.json")))
    }

    /// Explicit path constructor for testing.
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }
}
```

### Rules

- **Provide a `with_path()` constructor** for testing with `tempfile`.
- **Create parent directories** before writing: `std::fs::create_dir_all(path.parent())`.
- **Atomic writes**: Write to a temp file, then rename. This prevents corruption on crash.
- **Handle missing state gracefully**: Return `Ok(None)` when state file doesn't exist, not an error.

---

## 9. Security

### Compile-time enforcement

The Polis `Cargo.toml` enforces these lints at the workspace level:

```toml
[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
```

These are mandatory for all Polis Rust code. Do not weaken them.

### Type safety as security

Use newtypes to prevent parameter confusion:

```rust
pub struct WorkspaceId(String);
pub struct ImageSha256(String);

// Compiler prevents: delete_workspace(image_hash) — type mismatch
fn delete_workspace(id: &WorkspaceId) -> Result<()> { ... }
```

### Input validation

Validate all external input before use:

```rust
fn validate_workspace_id(id: &str) -> Result<&str> {
    anyhow::ensure!(
        id.chars().all(|c| c.is_alphanumeric() || c == '-'),
        "workspace ID contains invalid characters"
    );
    anyhow::ensure!(id.len() <= 64, "workspace ID too long");
    Ok(id)
}
```

- **Validate CLI arguments** beyond what clap provides (path traversal, length limits, character sets).
- **Sanitize before shell execution.** When building commands with `std::process::Command`, pass arguments as separate args, never as a concatenated string.

```rust
// GOOD: arguments are separate — no injection possible
Command::new("multipass")
    .args(["exec", vm_name, "--", "cat", path])

// BAD: string interpolation — shell injection risk
Command::new("sh")
    .arg("-c")
    .arg(format!("multipass exec {} -- cat {}", vm_name, path))
```

### Secrets handling

- **Never log secrets.** Use `secrecy::SecretString` for API keys if they pass through Rust code.
- **Never embed secrets in binaries.** Use environment variables or config files with restricted permissions.
- **Zeroize sensitive data** when done: the `zeroize` crate overwrites memory on drop.

### Cryptographic operations

Use vetted libraries only:

| Operation | Crate |
|-----------|-------|
| SHA-256 hashing | `sha2` |
| Signature verification | `ed25519-dalek`, `zipsign-api` |
| TLS | `rustls` (via `ureq`) |

Never implement custom cryptography.

### Supply chain security

- Run `cargo audit` regularly to check for known vulnerabilities.
- Use `cargo deny` to enforce license compliance and ban problematic crates.
- Pin dependencies in `Cargo.lock` (committed to version control for binaries).
- Review new dependencies before adding them — check maintenance status, download counts, and audit history.

### Integer overflow protection

Enable overflow checks in release builds:

```toml
[profile.release]
overflow-checks = true
```

Use checked arithmetic for security-critical calculations:

```rust
let total = size.checked_add(offset)
    .ok_or_else(|| anyhow::anyhow!("size overflow"))?;
```

---

## 10. Testing

### Test organization

```
tests/
├── integration/          # Test the compiled binary
│   ├── main.rs           # mod declarations
│   └── cli_tests.rs      # assert_cmd tests
└── unit/                 # Test internal modules
    ├── main.rs
    └── state.rs
```

Declare test binaries in `Cargo.toml`:

```toml
[[test]]
name = "integration"
path = "tests/integration/main.rs"

[[test]]
name = "unit"
path = "tests/unit/main.rs"
```

### Unit testing with trait-based mocking

The Polis pattern: define traits for external dependencies, inject them into command functions.

```rust
// Define the trait
pub trait Multipass {
    fn vm_info(&self) -> Result<Output>;
    fn start(&self) -> Result<Output>;
}

// Production implementation
pub struct MultipassCli;
impl Multipass for MultipassCli { ... }

// Command accepts trait object
pub fn run(args: &StartArgs, mp: &dyn Multipass, quiet: bool) -> Result<()> {
    let info = mp.vm_info()?;
    ...
}
```

In tests, create mock implementations:

```rust
struct MockMultipass {
    info_output: Output,
}

impl Multipass for MockMultipass {
    fn vm_info(&self) -> Result<Output> {
        Ok(self.info_output.clone())
    }
    ...
}
```

Or use `mockall` for complex scenarios:

```rust
use mockall::automock;

#[automock]
pub trait Multipass {
    fn vm_info(&self) -> Result<Output>;
}

#[test]
fn test_start_when_vm_running() {
    let mut mock = MockMultipass::new();
    mock.expect_vm_info()
        .returning(|| Ok(running_vm_output()));
    assert!(run(&args, &mock, false).is_ok());
}
```

### Integration testing with `assert_cmd`

Test the actual binary:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_version_flag() {
    Command::cargo_bin("polis")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("polis"));
}

#[test]
fn test_unknown_command() {
    Command::cargo_bin("polis")
        .unwrap()
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
```

### Testing with `tempfile`

Use temporary directories for state management tests:

```rust
use tempfile::TempDir;

#[test]
fn test_state_roundtrip() {
    let dir = TempDir::new().unwrap();
    let mgr = StateManager::with_path(dir.path().join("state.json"));

    mgr.save(&state).unwrap();
    let loaded = mgr.load().unwrap().unwrap();
    assert_eq!(loaded.workspace_id, state.workspace_id);
}
```

### Rules

- **Test behavior, not implementation.** Test what a function does, not how.
- **Use descriptive test names:** `test_delete_removes_state_file`, not `test_delete`.
- **No network calls in unit tests.** Use trait mocks.
- **Integration tests may call the binary** but should not depend on external services.
- **Run `cargo nextest run`** for faster parallel test execution.

---

## 11. Performance

### Binary size optimization

```toml
[profile.release]
opt-level = "z"          # Optimize for size (or "s" for balanced)
lto = true               # Link-time optimization
codegen-units = 1        # Better optimization, slower compile
strip = true             # Strip debug symbols
panic = "abort"          # Smaller binary, no unwinding
```

For development, keep defaults for fast compilation.

### Compile time optimization

- **Use feature flags** to avoid compiling unused functionality.
- **Minimize proc-macro dependencies** — they're the biggest compile-time cost.
- **Use `cargo build --timings`** to identify slow crates.
- **Consider `sccache`** for shared compilation caching in CI.

### Runtime performance

- **Avoid unnecessary allocations.** Use `&str` over `String` when ownership isn't needed.
- **Use iterators** instead of collecting into intermediate `Vec`s.
- **Profile before optimizing.** Use `cargo flamegraph` or `perf` to find actual bottlenecks.
- **Prefer `std::process::Command`** over spawning shells — it's faster and safer.

### Startup time

CLI tools should start fast. Avoid:
- Heavy initialization in `main()` before argument parsing.
- Loading large config files before knowing which command runs.
- Unnecessary async runtime setup for sync-only commands.

---

## 12. Logging & Diagnostics

### Structured logging with `tracing`

For CLI apps that need logging (beyond user-facing output), use `tracing`:

```rust
use tracing::{info, warn, debug, instrument};

#[instrument(skip(mp))]
async fn check_health(mp: &dyn Multipass) -> Result<HealthStatus> {
    debug!("checking VM health");
    let output = mp.vm_info()?;
    info!(status = %output.status, "health check complete");
    Ok(status)
}
```

### When to use logging vs output

| Scenario | Use |
|----------|-----|
| User-facing status messages | `OutputContext` methods |
| Debugging information | `tracing::debug!` |
| Operational diagnostics | `tracing::info!` / `tracing::warn!` |
| Error details for bug reports | `tracing::error!` |

### Verbosity levels

Support a `--verbose` / `-v` flag that increases log detail:

```rust
// No flag: only errors
// -v: info + warnings
// -vv: debug
// -vvv: trace
```

### Progress + logging coexistence

Use `tracing-indicatif` to prevent progress bars from clobbering log output, or use `indicatif`'s `ProgressBar::println()` method.

---

## 13. Signal Handling & Graceful Shutdown

### Ctrl+C handling

For long-running operations, handle interrupts gracefully:

```rust
tokio::select! {
    result = download_image(url) => result,
    _ = tokio::signal::ctrl_c() => {
        // Clean up partial downloads
        cleanup_temp_files()?;
        eprintln!("\nInterrupted");
        std::process::exit(130); // 128 + SIGINT(2)
    }
}
```

### Cleanup on shutdown

- Remove temporary files.
- Release file locks.
- Stop spawned child processes.
- Save partial state if appropriate.

### Exit codes for signals

Follow Unix convention: `128 + signal_number`.

| Signal | Exit code |
|--------|-----------|
| SIGINT (Ctrl+C) | 130 |
| SIGTERM | 143 |

---

## 14. Dependency Management

### Cargo.toml best practices

```toml
[dependencies]
# Pin major version, allow patch updates
clap = { version = "4.5", features = ["derive", "env", "wrap_help"] }

# Minimize features — only enable what you use
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process"] }

# Use workspace dependencies for multi-crate projects
serde = { version = "1.0", features = ["derive"] }
```

### Feature flags

- **Only enable features you need.** `tokio` has many features — don't use `full`.
- **Use `default-features = false`** when you only need a subset.
- **Document why** each dependency exists with a comment if it's not obvious.

### Auditing

```bash
# Check for known vulnerabilities
cargo audit

# Check licenses and banned crates
cargo deny check

# Find unused dependencies
cargo machete
```

### Updating

```bash
# Check for outdated dependencies
cargo outdated

# Update within semver constraints
cargo update

# Update a specific crate
cargo update -p clap
```

---

## 15. CI/CD & Tooling

### Required CI checks

```yaml
# Formatting
cargo fmt --check

# Linting (treat warnings as errors in CI)
cargo clippy -- -D warnings

# Tests
cargo nextest run

# Security audit
cargo audit

# Build release binary
cargo build --release
```

### Toolchain management

Pin the toolchain in `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

### Code formatting

Configure `rustfmt.toml`:

```toml
edition = "2021"
max_width = 100
```

Run `cargo fmt` before every commit. Configure your editor to format on save.

### Pre-commit checklist

Before submitting a PR:

1. `cargo fmt` — code is formatted
2. `cargo clippy -- -D warnings` — no lint warnings
3. `cargo test` — all tests pass
4. `cargo audit` — no known vulnerabilities
5. `cargo doc --no-deps` — documentation builds

---

## 16. Common Pitfalls

### Pitfall: Unnecessary cloning

```rust
// BAD: clones the entire string
fn process(data: String) {
    let name = data.clone();
    println!("{name}");
}

// GOOD: borrow instead
fn process(data: &str) {
    println!("{data}");
}
```

### Pitfall: Blocking the async runtime

```rust
// BAD: blocks the tokio thread pool
async fn hash_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path)?;  // blocking!
    Ok(hex::encode(sha256(&data)))
}

// GOOD: use spawn_blocking for CPU/IO work
async fn hash_file(path: PathBuf) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let data = std::fs::read(&path)?;
        Ok(hex::encode(sha256(&data)))
    }).await?
}
```

### Pitfall: Ignoring command exit status

```rust
// BAD: ignores whether the command succeeded
let output = Command::new("multipass").args(["start", "polis"]).output()?;
// `output` succeeded in *running* the command, but the command itself may have failed

// GOOD: check exit status
let output = Command::new("multipass").args(["start", "polis"]).output()?;
anyhow::ensure!(output.status.success(), "multipass start failed: {}",
    String::from_utf8_lossy(&output.stderr));
```

### Pitfall: Path handling across platforms

```rust
// BAD: hardcoded separator
let path = format!("{}/config.yaml", home);

// GOOD: use PathBuf
let path = PathBuf::from(home).join("config.yaml");
```

### Pitfall: Swallowing errors in error messages

```rust
// BAD: loses the original error
let data = std::fs::read_to_string(path)
    .map_err(|_| anyhow::anyhow!("failed to read file"))?;

// GOOD: preserves the error chain
let data = std::fs::read_to_string(path)
    .with_context(|| format!("failed to read {}", path.display()))?;
```

### Pitfall: Using `String` where `&str` suffices

```rust
// BAD: forces caller to allocate
fn greet(name: String) { println!("Hello, {name}"); }

// GOOD: accepts both &str and String
fn greet(name: &str) { println!("Hello, {name}"); }
```

### Pitfall: Large enum variants

```rust
// BAD: all variants are as large as the biggest one
enum Command {
    Simple,
    Complex { data: [u8; 4096] },  // forces 4KB for every variant
}

// GOOD: box the large variant
enum Command {
    Simple,
    Complex(Box<ComplexData>),
}
```

---

## 17. Recommended Crate Ecosystem

### CLI framework

| Crate | Purpose | Notes |
|-------|---------|-------|
| `clap` (derive) | Argument parsing | Use derive API with `env` and `wrap_help` features |
| `dialoguer` | Interactive prompts | Confirmations, selections, text input |
| `console` | Terminal utilities | TTY detection, terminal size, cursor control |

### Output & UX

| Crate | Purpose | Notes |
|-------|---------|-------|
| `owo-colors` | Colored output | Lightweight, supports `NO_COLOR` |
| `indicatif` | Progress bars | Spinners, bars, multi-progress |
| `serde_json` | JSON output | For `--json` mode |

### Error handling

| Crate | Purpose | Notes |
|-------|---------|-------|
| `anyhow` | Application errors | Context chaining, downcasting |
| `thiserror` | Library errors | Derive `Error` for typed enums |

### Async

| Crate | Purpose | Notes |
|-------|---------|-------|
| `tokio` | Async runtime | Use minimal feature set |

### Serialization & config

| Crate | Purpose | Notes |
|-------|---------|-------|
| `serde` | Serialization framework | Always with `derive` feature |
| `serde_yaml` | YAML config files | For human-edited config |
| `serde_json` | JSON state files | For machine-generated state |

### File system & paths

| Crate | Purpose | Notes |
|-------|---------|-------|
| `dirs` | Platform directories | Home dir, config dir |
| `tempfile` | Temporary files/dirs | For tests and atomic writes |

### Security & crypto

| Crate | Purpose | Notes |
|-------|---------|-------|
| `sha2` | SHA-256 hashing | Image verification |
| `ed25519-dalek` | Signature verification | Release signing |
| `zipsign-api` | Signed archive verification | Update verification |
| `ureq` | HTTP client | Uses `rustls` (no OpenSSL) |

### Testing

| Crate | Purpose | Notes |
|-------|---------|-------|
| `assert_cmd` | Binary integration tests | Run and assert on CLI output |
| `predicates` | Test assertions | Composable matchers |
| `tempfile` | Isolated test state | Temp dirs for state tests |
| `mockall` | Mock generation | Auto-generate trait mocks |

### Versioning & updates

| Crate | Purpose | Notes |
|-------|---------|-------|
| `semver` | Version parsing | Compare and validate versions |
| `self_update` | Self-update mechanism | GitHub release downloads |
| `chrono` | Date/time handling | Timestamps in state |

---

## Appendix A: Cargo.toml Template

```toml
[workspace]
members = ["."]
resolver = "2"

[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"

[package]
name = "my-cli"
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"

[[bin]]
name = "mycli"
path = "src/main.rs"

[lib]
name = "my_cli"
path = "src/lib.rs"

[dependencies]
clap = { version = "4.5", features = ["derive", "env", "wrap_help"] }
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[dev-dependencies]
assert_cmd = "2.1"
predicates = "3.1"
tempfile = "3"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
panic = "abort"
overflow-checks = true

[lints]
workspace = true
```

---

## Appendix B: Quick Reference Checklist

Use this checklist when reviewing Rust CLI code:

- [ ] No `unsafe` code
- [ ] No `unwrap()` or `expect()` in production paths
- [ ] All errors have context via `.with_context()` or `.context()`
- [ ] External input is validated before use
- [ ] Commands passed as separate args to `std::process::Command` (no shell interpolation)
- [ ] Secrets never logged or embedded in binaries
- [ ] `--json` output supported for scriptable commands
- [ ] `--quiet` suppresses non-error output
- [ ] `NO_COLOR` environment variable respected
- [ ] Progress bars only shown on TTY
- [ ] Interactive prompts have `--yes` bypass
- [ ] Temporary files cleaned up on error paths
- [ ] Tests use trait mocks, not real infrastructure
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo audit` shows no vulnerabilities

---

*Last updated: 2026-02-21*
