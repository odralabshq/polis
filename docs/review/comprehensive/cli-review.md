# Polis CLI Code Review Report

**Review Date:** 2026-02-22
**Reviewer:** Lead Code Assurance Architect
**Scope:** Full CLI codebase (`cli/src/**/*.rs`, ~4,500 LOC)
**Standards:** CODER.md (Rust CLI Coding Standards)

---

## 1. Executive Summary

| Metric | Value |
|--------|-------|
| **Overall Score** | 78/100 |
| **Verdict** | **Conditional Pass** |
| **Critical Issues** | 4 |
| **Major Issues** | 6 |
| **Minor Issues** | 8 |

### Top 3 Risks

1. **Shell Injection Vulnerabilities** — String interpolation in shell commands (`install_pubkey`, `set_env_var`) creates injection vectors
2. **Non-Atomic State Writes** — State file writes don't use temp-file-then-rename pattern, risking corruption on crash
3. **Missing Signal Handling** — No graceful shutdown on Ctrl+C; long operations can leave partial state

### Strengths

- Excellent trait-based abstractions enabling comprehensive testing
- Strong compile-time safety (`unsafe_code = "deny"`, pedantic clippy)
- Proper cryptographic verification for updates (zipsign ed25519)
- Consistent output handling via `OutputContext` pattern
- Good error context propagation throughout

---

## 2. Detailed Findings

### Pillar: Security

#### [Critical] SEC-001: Shell Injection in `install_pubkey`

**Location:** `src/commands/connect.rs:89-96`

**Issue:** The `install_pubkey` function constructs a shell script by interpolating the `pubkey` variable directly into a bash command string. A malformed public key containing shell metacharacters could execute arbitrary commands.

```rust
let script = format!(
    "mkdir -p /home/polis/.ssh && chmod 700 /home/polis/.ssh && \
     grep -qxF '{pubkey}' /home/polis/.ssh/authorized_keys 2>/dev/null || \
     echo '{pubkey}' >> /home/polis/.ssh/authorized_keys && \
     ..."
);
```

**Risk:** If an attacker can control the public key content (e.g., via a compromised key file), they could inject commands like `'; rm -rf / #` into the workspace container.

**Fix:**
```rust
// Option 1: Use separate arguments (preferred)
let status = tokio::process::Command::new(cmd)
    .args([
        "exec", container, "bash", "-c",
        "cat >> /home/polis/.ssh/authorized_keys"
    ])
    .stdin(std::process::Stdio::piped())
    .spawn()?;
// Write pubkey to stdin

// Option 2: Validate pubkey format strictly before use
fn validate_pubkey(key: &str) -> Result<()> {
    anyhow::ensure!(
        key.starts_with("ssh-ed25519 ") || key.starts_with("ssh-rsa "),
        "invalid public key format"
    );
    anyhow::ensure!(
        key.chars().all(|c| c.is_ascii_alphanumeric() || " +/=@.-".contains(c)),
        "public key contains invalid characters"
    );
    Ok(())
}
```

**CODER.md Reference:** §9 Security — "Sanitize before shell execution. When building commands with `std::process::Command`, pass arguments as separate args, never as a concatenated string."

---

#### [Critical] SEC-002: Shell Injection in `set_env_var`

**Location:** `src/commands/update.rs:296-308`

**Issue:** The `set_env_var` function constructs shell commands with string interpolation of `key` and `value` parameters.

```rust
let cmd = if value.is_empty() {
    format!(
        "grep -v '^{key}=' {ENV_PATH} 2>/dev/null > {ENV_PATH}.tmp && ..."
    )
} else {
    format!(
        "{{ grep -v '^{key}=' {ENV_PATH} 2>/dev/null; echo '{key}={value}'; }} > ..."
    )
};
```

**Risk:** While `key` is derived from `image_name_to_env_var()` which produces safe output, `value` comes from the versions manifest. A compromised manifest could inject shell commands.

**Fix:**
```rust
// Validate value before use
fn validate_env_value(value: &str) -> Result<()> {
    anyhow::ensure!(
        value.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)),
        "env value contains invalid characters: {value}"
    );
    Ok(())
}

// Or use a safer approach: write to a temp file and transfer
fn set_env_var_safe(key: &str, value: &str, mp: &impl Multipass) -> Result<()> {
    validate_env_value(value)?;
    // ... existing logic with validated input
}
```

**CODER.md Reference:** §9 Security — "Validate all external input before use."

---

#### [Critical] SEC-003: Insufficient Input Validation for Version Tags

**Location:** `src/commands/update.rs:67-82`

**Issue:** While `validate_version_tag` exists and is good, it's not called early enough in all code paths. The `compute_container_updates` function validates tags, but `set_env_var` is called later without re-validation.

**Fix:** Add validation at the point of use in `set_env_var`:
```rust
fn set_env_var(key: &str, value: &str, mp: &impl Multipass) -> Result<()> {
    if !value.is_empty() {
        validate_version_tag(value)?;
    }
    // ... rest of function
}
```

---

#### [Critical] SEC-004: Missing Workspace ID Validation

**Location:** `src/state.rs`

**Issue:** `WorkspaceState.workspace_id` is stored and loaded without validation. While it's generated internally, a corrupted or tampered state file could contain malicious content.

**Fix:**
```rust
fn validate_workspace_id(id: &str) -> Result<()> {
    anyhow::ensure!(
        id.starts_with("polis-") && id.len() == 22,
        "invalid workspace ID format"
    );
    anyhow::ensure!(
        id[6..].chars().all(|c| c.is_ascii_hexdigit()),
        "workspace ID contains invalid characters"
    );
    Ok(())
}

impl StateManager {
    pub fn load(&self) -> Result<Option<WorkspaceState>> {
        // ... existing load logic ...
        if let Some(ref state) = state {
            validate_workspace_id(&state.workspace_id)?;
        }
        Ok(state)
    }
}
```

**CODER.md Reference:** §9 Security — "Validate all external input before use."

---

### Pillar: Reliability

#### [Major] REL-001: Non-Atomic State File Writes

**Location:** `src/state.rs:67-82`

**Issue:** The `save` method writes directly to the state file without using the atomic temp-file-then-rename pattern. If the process crashes during write, the state file could be corrupted.

```rust
pub fn save(&self, state: &WorkspaceState) -> Result<()> {
    // ...
    std::fs::write(&self.path, &content)  // NOT ATOMIC
        .with_context(|| ...)?;
    // ...
}
```

**Fix:**
```rust
pub fn save(&self, state: &WorkspaceState) -> Result<()> {
    if let Some(parent) = self.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(state)?;
    
    // Atomic write: temp file then rename
    let temp_path = self.path.with_extension("json.tmp");
    std::fs::write(&temp_path, &content)
        .with_context(|| format!("writing temp file {}", temp_path.display()))?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600))?;
    }
    
    std::fs::rename(&temp_path, &self.path)
        .with_context(|| format!("renaming {} to {}", temp_path.display(), self.path.display()))?;
    
    Ok(())
}
```

**CODER.md Reference:** §8 State Management — "Atomic writes: Write to a temp file, then rename. This prevents corruption on crash."

---

#### [Major] REL-002: Missing Signal Handling

**Location:** `src/main.rs`, `src/commands/start.rs`, `src/workspace/image.rs`

**Issue:** Long-running operations (image download, VM creation, health checks) don't handle Ctrl+C gracefully. Users interrupting operations may leave partial state.

**Fix:**
```rust
// In main.rs
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    
    tokio::select! {
        result = cli.run() => {
            if let Err(e) = result {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted");
            // Cleanup partial state if needed
            std::process::exit(130); // 128 + SIGINT(2)
        }
    }
}

// In image.rs download function
tokio::select! {
    result = download_bytes(&mut reader, &mut file, &pb) => result?,
    _ = tokio::signal::ctrl_c() => {
        pb.finish_and_clear();
        // Partial file is kept for resume
        anyhow::bail!("Download interrupted. Resume with: polis start");
    }
}
```

**CODER.md Reference:** §13 Signal Handling & Graceful Shutdown — "For long-running operations, handle interrupts gracefully."

---

#### [Major] REL-003: Partial Failure Handling in Delete

**Location:** `src/commands/delete.rs:40-55`

**Issue:** If VM deletion succeeds but state clearing fails, the system is left in an inconsistent state where the VM is gone but state file still references it.

**Fix:**
```rust
fn delete_workspace(args: &DeleteArgs, mp: &impl Multipass, quiet: bool) -> Result<()> {
    // ... confirmation logic ...
    
    // Collect errors instead of failing fast
    let mut errors = Vec::new();
    
    if vm::exists(mp) {
        if !quiet { println!("Removing workspace..."); }
        vm::delete(mp);
    }
    
    if let Err(e) = StateManager::new().and_then(|m| m.clear()) {
        errors.push(format!("Failed to clear state: {e}"));
    }
    
    if !errors.is_empty() {
        anyhow::bail!("Delete completed with errors:\n{}", errors.join("\n"));
    }
    
    // ... success message ...
    Ok(())
}
```

---

#### [Major] REL-004: Hardcoded Health Check Timeout

**Location:** `src/workspace/health.rs:32-33`

**Issue:** The health check timeout (60 seconds) is hardcoded and may be insufficient for slow systems or large images.

```rust
let max_attempts = 30;
let delay = Duration::from_secs(2);
```

**Fix:** Make timeout configurable via environment variable:
```rust
fn get_health_timeout() -> (u32, Duration) {
    let timeout_secs: u64 = std::env::var("POLIS_HEALTH_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let delay = Duration::from_secs(2);
    let max_attempts = (timeout_secs / 2) as u32;
    (max_attempts, delay)
}
```

---

### Pillar: Performance

#### [Major] PERF-001: Blocking I/O in Async Context

**Location:** `src/workspace/image.rs:195-207`

**Issue:** `sha256_file` performs blocking file I/O but may be called from async context. This can block the tokio runtime thread pool.

```rust
fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;  // BLOCKING
    // ... blocking read loop ...
}
```

**Fix:**
```rust
async fn sha256_file_async(path: PathBuf) -> Result<String> {
    tokio::task::spawn_blocking(move || sha256_file_sync(&path))
        .await
        .context("hash task panicked")?
}

fn sha256_file_sync(path: &Path) -> Result<String> {
    // ... existing implementation ...
}
```

**CODER.md Reference:** §5 Async Runtime & Tokio — "Don't block the async runtime. Use `tokio::task::spawn_blocking` for CPU-heavy or blocking I/O work."

---

#### [Major] PERF-002: Inconsistent Use of Multipass Trait

**Location:** `src/workspace/vm.rs:127-134`

**Issue:** The `stop` and `delete` functions bypass the `Multipass` trait and call `std::process::Command` directly. This breaks the abstraction and makes these functions untestable.

```rust
pub fn stop(mp: &impl Multipass) -> Result<()> {
    let _ = mp.exec(&["docker", "compose", "-f", COMPOSE_PATH, "stop"]);
    let output = std::process::Command::new("multipass")  // BYPASSES TRAIT
        .args(["stop", "polis"])
        .output()?;
    // ...
}
```

**Fix:** Add `stop` and `delete` methods to the `Multipass` trait:
```rust
pub trait Multipass {
    // ... existing methods ...
    
    fn stop(&self) -> Result<Output>;
    fn delete(&self) -> Result<Output>;
    fn purge(&self) -> Result<Output>;
}

impl Multipass for MultipassCli {
    fn stop(&self) -> Result<Output> {
        Command::new("multipass")
            .args(["stop", VM_NAME])
            .output()
            .context("failed to run multipass stop")
    }
    // ...
}
```

---

### Pillar: Maintainability

#### [Minor] MAINT-001: Duplicated Constants

**Location:** Multiple files

**Issue:** `COMPOSE_PATH` is defined in 4 different files:
- `src/commands/status.rs:14`
- `src/commands/update.rs:178`
- `src/workspace/vm.rs:12`
- `src/workspace/health.rs:10`

**Fix:** Define once in a shared location:
```rust
// src/workspace/mod.rs
pub const COMPOSE_PATH: &str = "/opt/polis/docker-compose.yml";

// Usage in other files
use crate::workspace::COMPOSE_PATH;
```

---

#### [Minor] MAINT-002: Duplicated Color Logic

**Location:** `src/workspace/vm.rs:72-85`, `src/commands/start.rs:72-85`

**Issue:** The `inception_line` function and `print_guarantees` function both define inline color styles that should be centralized in `styles.rs`.

**Fix:** Add inception styles to `Styles`:
```rust
// src/output/styles.rs
impl Styles {
    pub fn inception_l0(&self) -> Style { Style::new().truecolor(107, 33, 168) }
    pub fn inception_l1(&self) -> Style { Style::new().truecolor(93, 37, 163) }
    pub fn inception_l2(&self) -> Style { Style::new().truecolor(64, 47, 153) }
    pub fn inception_l3(&self) -> Style { Style::new().truecolor(46, 53, 147) }
}
```

---

#### [Minor] MAINT-003: Excessive `#[allow(dead_code)]`

**Location:** Multiple files

**Issue:** Several `#[allow(dead_code)]` annotations exist without clear justification:
- `src/main.rs:10` — `ssh` module
- `src/state.rs:57` — `load` method
- `src/workspace/image.rs:47-48` — `is_cached`, `cached_path`
- `src/commands/status.rs:3` — entire module

**Fix:** Either:
1. Remove unused code
2. Add `// Used by: <feature/test>` comments explaining future use
3. Gate behind feature flags if truly optional

---

#### [Minor] MAINT-004: Long Functions

**Location:** `src/commands/update.rs`, `src/workspace/image.rs`

**Issue:** Several functions exceed recommended complexity:
- `run` in update.rs (~50 lines)
- `ensure_default` in image.rs (~30 lines)
- `do_download` in image.rs (~45 lines)

**Fix:** Extract helper functions:
```rust
// Before
pub async fn run(...) -> Result<()> {
    // 50 lines of mixed concerns
}

// After
pub async fn run(...) -> Result<()> {
    let cli_update = check_cli_update(checker, current)?;
    let image_update = check_image_update();
    display_update_status(ctx, current, &cli_update, image_update.as_ref());
    
    if args.check { return Ok(()); }
    
    apply_cli_update(ctx, checker, cli_update)?;
    apply_container_updates(ctx, mp)?;
    Ok(())
}
```

---

#### [Minor] MAINT-005: Missing `#[must_use]` Annotations

**Location:** Various pure functions

**Issue:** Several pure functions that return values should have `#[must_use]` to prevent accidental ignored results:
- `format_uptime` in status.rs
- `workspace_state_display` in status.rs
- `ghcr_ref` in update.rs

**Note:** Some of these already have `#[must_use]` — good! Ensure consistency.

---

### Pillar: Correctness

#### [Minor] CORR-001: Potential Duplicate Workspace IDs

**Location:** `src/commands/start.rs:95-105`

**Issue:** `generate_workspace_id` uses `unwrap_or(0)` for system time, which could theoretically produce duplicate IDs if called at the exact same nanosecond or if system time is unavailable.

```rust
hasher.write_u128(
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0),  // Could produce duplicates
);
```

**Fix:** Add additional entropy:
```rust
fn generate_workspace_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    hasher.write_u64(std::process::id() as u64);  // Add PID
    hasher.write_u64(rand::random());  // Add random entropy
    format!("polis-{:016x}", hasher.finish())
}
```

---

#### [Minor] CORR-002: Unbounded Input in `confirm`

**Location:** `src/commands/delete.rs:82-89`

**Issue:** The `confirm` function reads a line without length limit, potentially allowing memory exhaustion with malicious input.

**Fix:**
```rust
fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{BufRead, Write};
    print!("{prompt} [y/N]: ");
    std::io::stdout().flush()?;
    
    let mut line = String::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    
    // Limit read to 10 bytes (more than enough for y/n)
    handle.take(10).read_line(&mut line)?;
    
    anyhow::ensure!(!line.is_empty(), "no input provided");
    Ok(line.trim().eq_ignore_ascii_case("y"))
}
```

---

## 3. Code Quality Metrics

| Metric | Rating | Notes |
|--------|--------|-------|
| **Readability** | High | Clear naming, good module structure, consistent patterns |
| **Testability** | High | Excellent trait-based abstractions, comprehensive test coverage |
| **Complexity** | Medium | Some long functions, but generally well-structured |
| **Documentation** | High | Good `///` doc comments, clear error messages |
| **Type Safety** | High | Strong use of enums, newtypes for some values |

### Test Coverage Assessment

| Module | Unit Tests | Integration Tests | Coverage |
|--------|------------|-------------------|----------|
| `state.rs` | ✓ Comprehensive | ✓ | High |
| `ssh.rs` | ✓ Comprehensive | - | High |
| `multipass.rs` | - | ✓ Via mocks | Medium |
| `commands/*` | ✓ Most commands | ✓ | High |
| `workspace/*` | ✓ Basic | - | Medium |
| `output/*` | - | - | Low |

---

## 4. CODER.md Compliance Summary

| Requirement | Status | Notes |
|-------------|--------|-------|
| No `unsafe` code | ✅ Pass | `unsafe_code = "deny"` in Cargo.toml |
| No `unwrap()`/`expect()` in production | ⚠️ Partial | Templates in progress.rs use `expect` (justified) |
| Error context via `.context()` | ✅ Pass | Consistent throughout |
| Commands as separate args | ❌ Fail | Shell interpolation in 2 locations |
| Atomic state writes | ❌ Fail | Direct write, no temp+rename |
| Signal handling | ❌ Fail | No Ctrl+C handling |
| `--json` output support | ✅ Pass | All relevant commands support it |
| `--quiet` suppresses output | ✅ Pass | Consistent implementation |
| `NO_COLOR` respected | ✅ Pass | Checked in `OutputContext::new` |
| Progress bars only on TTY | ✅ Pass | `show_progress()` checks TTY |
| Trait-based mocking | ✅ Pass | Excellent abstractions |
| Separate test binaries | ✅ Pass | `unit` and `integration` |

---

## 5. Final Recommendations

### Blocking (Must Fix Before Release)

1. **Fix shell injection vulnerabilities** in `install_pubkey` and `set_env_var`
2. **Implement atomic state writes** using temp-file-then-rename pattern
3. **Add input validation** for workspace IDs loaded from state file

### High Priority (Fix Soon)

4. **Add signal handling** for graceful Ctrl+C shutdown
5. **Wrap blocking I/O** in `spawn_blocking` for `sha256_file`
6. **Extend Multipass trait** to include `stop` and `delete` methods

### Medium Priority (Technical Debt)

7. **Consolidate duplicated constants** (`COMPOSE_PATH`)
8. **Centralize color definitions** in `styles.rs`
9. **Remove or justify `#[allow(dead_code)]`** annotations
10. **Make health check timeout configurable**

### Low Priority (Nice to Have)

11. **Add `#[must_use]` annotations** to remaining pure functions
12. **Refactor long functions** in update.rs and image.rs
13. **Add entropy to workspace ID generation**
14. **Limit input length in `confirm` function**

---

## Appendix A: Files Reviewed

```
cli/src/
├── main.rs              ✓
├── lib.rs               ✓
├── cli.rs               ✓
├── state.rs             ✓
├── multipass.rs         ✓
├── ssh.rs               ✓
├── commands/
│   ├── mod.rs           ✓
│   ├── start.rs         ✓
│   ├── stop.rs          ✓
│   ├── delete.rs        ✓
│   ├── status.rs        ✓
│   ├── connect.rs       ✓
│   ├── config.rs        ✓
│   ├── doctor.rs        ✓
│   ├── update.rs        ✓
│   ├── version.rs       ✓
│   └── internal.rs      ✓
├── output/
│   ├── mod.rs           ✓
│   ├── styles.rs        ✓
│   ├── progress.rs      ✓
│   └── json.rs          ✓
└── workspace/
    ├── mod.rs           ✓
    ├── image.rs         ✓
    ├── vm.rs            ✓
    └── health.rs        ✓
```

---

## Appendix B: Quick Reference Fixes

### SEC-001 Fix (install_pubkey)

```rust
// In src/commands/connect.rs
async fn install_pubkey(pubkey: &str) -> Result<()> {
    // Validate pubkey format first
    anyhow::ensure!(
        pubkey.starts_with("ssh-ed25519 ") || pubkey.starts_with("ssh-rsa "),
        "invalid public key format"
    );
    anyhow::ensure!(
        pubkey.chars().all(|c| c.is_ascii_alphanumeric() || " +/=@.-\n".contains(c)),
        "public key contains invalid characters"
    );
    
    // Use stdin instead of shell interpolation
    let mut child = tokio::process::Command::new(cmd)
        .args(args_for_backend)
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(pubkey.as_bytes()).await?;
    }
    
    let status = child.wait().await?;
    anyhow::ensure!(status.success(), "failed to install public key");
    Ok(())
}
```

### REL-001 Fix (atomic writes)

```rust
// In src/state.rs
pub fn save(&self, state: &WorkspaceState) -> Result<()> {
    if let Some(parent) = self.path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    
    let content = serde_json::to_string_pretty(state).context("serializing state")?;
    let temp_path = self.path.with_extension("json.tmp");
    
    std::fs::write(&temp_path, &content)
        .with_context(|| format!("writing temp file {}", temp_path.display()))?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("setting permissions on {}", temp_path.display()))?;
    }
    
    std::fs::rename(&temp_path, &self.path)
        .with_context(|| format!("finalizing state file {}", self.path.display()))?;
    
    Ok(())
}
```

---

*Report generated: 2026-02-22*
*Next review recommended: After addressing Critical and Major issues*
