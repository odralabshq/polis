# Rust Mocking Quick Reference

**For**: Polis CLI Unit Tests  
**Updated**: 2026-02-21

---

## Decision Tree: Which Mocking Strategy?

```
Does the code touch external systems?
├─ NO → Pure unit test (no mocking needed)
│   Example: src/output/tests.rs
│
└─ YES → What kind of external system?
    ├─ Filesystem
    │   ├─ Need real FS semantics? → Use `tempfile::TempDir`
    │   │   Example: src/state.rs
    │   └─ Just need read/write? → Extract `trait FileSystem` + mock
    │
    ├─ Process execution (multipass, docker)
    │   └─ Extract trait → Manual mock or `mockall`
    │       Example: tests/doctor_command.rs (HealthProbe)
    │
    ├─ Network (HTTP, API calls)
    │   └─ Extract trait → Manual mock
    │       Example: UpdateChecker (to be implemented)
    │
    └─ I/O operations (Read/Write)
        └─ Accept `impl Read`/`impl Write` → Use `std::io::Cursor`
```

---

## Pattern 1: Manual Trait-Based Mock (Simple)

**When**: Trait has <5 methods, simple return types

**Example**: `tests/doctor_command.rs`

```rust
// 1. Define trait (in production code)
pub trait HealthProbe {
    async fn check_prerequisites(&self) -> Result<PrerequisiteChecks>;
    async fn check_workspace(&self) -> Result<WorkspaceChecks>;
}

// 2. Create mock (in test code)
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
        })
    }
}

// 3. Use in test
#[tokio::test]
async fn test_doctor_healthy_system_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = doctor::run_with(&ctx, false, &MockHealthyProbe).await;
    assert!(result.is_ok());
}
```

**Pros**: Simple, explicit, no dependencies  
**Cons**: Boilerplate for complex traits

---

## Pattern 2: `mockall` Auto-Generated Mock (Complex)

**When**: Trait has >5 methods, need call verification

**Setup**: Add to `Cargo.toml`:
```toml
[dev-dependencies]
mockall = "0.13"
```

**Example**:

```rust
use mockall::{automock, predicate::*};

// 1. Annotate trait with #[automock]
#[automock]
pub trait Multipass {
    fn vm_info(&self) -> Result<Output>;
    fn launch(&self, name: &str, image: &str, cpus: &str, mem: &str) -> Result<Output>;
    fn start(&self) -> Result<Output>;
    fn stop(&self) -> Result<Output>;
    fn delete(&self) -> Result<Output>;
    fn exec(&self, args: &[&str]) -> Result<Output>;
}

// 2. Use MockMultipass in tests
#[test]
fn test_stop_calls_multipass_stop_once() {
    let mut mock = MockMultipass::new();
    
    // Set expectations
    mock.expect_vm_info()
        .times(1)
        .returning(|| Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: br#"{"info":{"polis":{"state":"Running"}}}"#.to_vec(),
            stderr: Vec::new(),
        }));
    
    mock.expect_stop()
        .times(1)
        .returning(|| Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }));
    
    // Run test
    let result = stop::run(&mock, true);
    assert!(result.is_ok());
    
    // mockall automatically verifies expectations on drop
}
```

**Advanced features**:
```rust
// Match specific arguments
mock.expect_launch()
    .with(eq("polis"), eq("ubuntu:22.04"), eq("2"), eq("4G"))
    .times(1)
    .returning(|_, _, _, _| Ok(output));

// Return different values on successive calls
mock.expect_vm_info()
    .times(2)
    .returning(|| Ok(stopped_output))
    .returning(|| Ok(running_output));

// Return error
mock.expect_exec()
    .returning(|_| Err(anyhow::anyhow!("connection refused")));
```

**Pros**: Auto-generated, call verification, argument matching  
**Cons**: Adds dependency, more complex API

---

## Pattern 3: Filesystem Isolation with `tempfile`

**When**: Need real filesystem semantics (permissions, atomic writes, etc.)

**Example**: `src/state.rs`

```rust
use tempfile::TempDir;

#[cfg(test)]
mod tests {
    use super::*;
    
    fn mgr(dir: &TempDir) -> StateManager {
        StateManager::with_path(dir.path().join("state.json"))
    }
    
    #[test]
    fn test_state_manager_save_load_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        
        let state = WorkspaceState {
            workspace_id: "polis-test".to_string(),
            created_at: Utc::now(),
            image_sha256: Some("abc123".to_string()),
        };
        
        m.save(&state).expect("save");
        let loaded = m.load().expect("load").expect("state present");
        
        assert_eq!(loaded.workspace_id, state.workspace_id);
        
        // TempDir automatically cleaned up on drop
    }
}
```

**Key points**:
- `TempDir::new()` creates isolated directory
- Automatically cleaned up when `TempDir` is dropped
- Works even if test panics
- Production code must accept path via constructor: `with_path(path: PathBuf)`

**Pros**: Tests real filesystem behavior, automatic cleanup  
**Cons**: Slower than in-memory mocks (~10-50ms per test)

---

## Pattern 4: In-Memory Filesystem Mock

**When**: Don't need real FS semantics, want fast tests

**Example**:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

trait FileSystem {
    fn read(&self, path: &Path) -> Result<String>;
    fn write(&self, path: &Path, content: &str) -> Result<()>;
    fn exists(&self, path: &Path) -> bool;
}

struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).map_err(Into::into)
    }
    
    fn write(&self, path: &Path, content: &str) -> Result<()> {
        std::fs::write(path, content).map_err(Into::into)
    }
    
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

#[cfg(test)]
struct MockFileSystem {
    files: HashMap<PathBuf, String>,
}

#[cfg(test)]
impl FileSystem for MockFileSystem {
    fn read(&self, path: &Path) -> Result<String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("file not found: {}", path.display()))
    }
    
    fn write(&mut self, path: &Path, content: &str) -> Result<()> {
        self.files.insert(path.to_path_buf(), content.to_string());
        Ok(())
    }
    
    fn exists(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }
}

// Usage in production code
pub struct ConfigManager<F: FileSystem> {
    fs: F,
    path: PathBuf,
}

impl<F: FileSystem> ConfigManager<F> {
    pub fn load(&self) -> Result<Config> {
        let content = self.fs.read(&self.path)?;
        serde_yaml::from_str(&content).map_err(Into::into)
    }
}

// Usage in test
#[test]
fn test_config_manager_load_returns_config() {
    let mut fs = MockFileSystem {
        files: HashMap::new(),
    };
    fs.files.insert(
        PathBuf::from("/config.yaml"),
        "security:\n  level: strict\n".to_string(),
    );
    
    let mgr = ConfigManager {
        fs,
        path: PathBuf::from("/config.yaml"),
    };
    
    let config = mgr.load().expect("load should succeed");
    assert_eq!(config.security.level, "strict");
}
```

**Pros**: Fast (<1ms), full control over behavior  
**Cons**: More boilerplate, doesn't test real FS behavior

---

## Pattern 5: `std::io` Trait Reuse

**When**: Working with `Read`/`Write` operations

**Example**:

```rust
use std::io::{Read, Write, Cursor};

// Production code accepts trait
fn parse_config(mut reader: impl Read) -> Result<Config> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;
    serde_yaml::from_str(&content).map_err(Into::into)
}

fn write_config(mut writer: impl Write, config: &Config) -> Result<()> {
    let yaml = serde_yaml::to_string(config)?;
    writer.write_all(yaml.as_bytes())?;
    Ok(())
}

// Test with Cursor (in-memory buffer)
#[test]
fn test_parse_config_valid_yaml_returns_config() {
    let yaml = b"security:\n  level: strict\n";
    let cursor = Cursor::new(yaml);
    
    let config = parse_config(cursor).expect("parse should succeed");
    assert_eq!(config.security.level, "strict");
}

#[test]
fn test_write_config_produces_valid_yaml() {
    let config = Config {
        security: SecurityConfig {
            level: "balanced".to_string(),
        },
    };
    
    let mut buffer = Vec::new();
    write_config(&mut buffer, &config).expect("write should succeed");
    
    let yaml = String::from_utf8(buffer).expect("valid UTF-8");
    assert!(yaml.contains("balanced"));
}
```

**Pros**: No dependencies, idiomatic Rust, fast  
**Cons**: Only works for I/O operations

---

## Pattern 6: Async Trait Mocking

**When**: Mocking async traits

**Setup**: Add to `Cargo.toml`:
```toml
[dev-dependencies]
mockall = "0.13"
async-trait = "0.1"
```

**Example**:

```rust
use async_trait::async_trait;
use mockall::automock;

#[automock]
#[async_trait]
pub trait UpdateChecker {
    async fn check_for_updates(&self, current: &str) -> Result<Option<UpdateInfo>>;
    async fn download(&self, url: &str, dest: &Path) -> Result<()>;
}

#[tokio::test]
async fn test_update_no_new_version() {
    let mut mock = MockUpdateChecker::new();
    
    mock.expect_check_for_updates()
        .with(eq("v0.1.0"))
        .times(1)
        .returning(|_| Box::pin(async { Ok(None) }));
    
    let result = update::run(&mock).await;
    assert!(result.is_ok());
}
```

**Note**: `mockall` with async requires `Box::pin(async { ... })` for return values.

---

## Anti-Patterns to Avoid

### ❌ Don't: Use `unimplemented!()` in mocks
```rust
impl Multipass for MockNotFound {
    fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
        unimplemented!()  // ❌ Panics if accidentally called
    }
}
```

**Fix**: Use `mockall` or implement all methods (even if they just return errors).

---

### ❌ Don't: Test implementation details
```rust
#[test]
fn test_config_manager_internal_cache_size() {
    let mgr = ConfigManager::new();
    assert_eq!(mgr.cache.len(), 0);  // ❌ Testing private field
}
```

**Fix**: Test observable behavior through public API.

---

### ❌ Don't: Use real filesystem without isolation
```rust
#[test]
fn test_save_config() {
    let mgr = ConfigManager::new();  // Uses ~/.polis/config.yaml
    mgr.save(&config).unwrap();      // ❌ Writes to real home dir
}
```

**Fix**: Use `tempfile` or inject path via constructor.

---

### ❌ Don't: Spawn real processes in unit tests
```rust
#[test]
fn test_multipass_list() {
    let output = Command::new("multipass")  // ❌ Real process
        .arg("list")
        .output()
        .unwrap();
    assert!(output.status.success());
}
```

**Fix**: Extract trait and mock it.

---

### ❌ Don't: Make real network calls in unit tests
```rust
#[test]
fn test_check_for_updates() {
    let checker = GitHubUpdateChecker::new();
    let result = checker.check_for_updates("v0.1.0").unwrap();  // ❌ Real HTTP
}
```

**Fix**: Extract trait and mock it.

---

## Checklist: Is This a Good Unit Test?

- [ ] Runs in <50ms
- [ ] No real filesystem I/O (except via `tempfile`)
- [ ] No real network calls
- [ ] No real process execution
- [ ] No shared mutable state between tests
- [ ] Can run in parallel with other tests
- [ ] Tests behavior, not implementation
- [ ] Has exactly one reason to fail
- [ ] Has a descriptive name: `test_<unit>_<scenario>_<expected_behavior>`
- [ ] Covers at least one error path
- [ ] Uses trait-based mocking for external dependencies

---

## Quick Commands

```bash
# Run only unit tests (fast)
cargo test --lib

# Run specific test
cargo test test_config_manager_load

# Run tests with output
cargo test -- --nocapture

# Run tests sequentially (debug flaky tests)
cargo test -- --test-threads=1

# Check test coverage
cargo tarpaulin --workspace --out html

# Lint test code
cargo clippy --workspace --tests -- -D warnings
```

---

## Examples in This Codebase

| Pattern | File | Status |
|---------|------|--------|
| Manual trait mock | `tests/doctor_command.rs` | ✅ Excellent |
| Manual trait mock | `tests/status_command.rs` | ✅ Good |
| `tempfile` isolation | `src/state.rs` | ✅ Excellent |
| `tempfile` isolation | `src/ssh.rs` | ✅ Good |
| Pure unit test | `src/output/tests.rs` | ✅ Excellent |
| `mockall` | None yet | ⚠️ To be added |
| In-memory FS mock | None yet | ⚠️ To be added |

---

**Last updated**: 2026-02-21  
**Maintainer**: Senior Rust SDET, Odra Labs
