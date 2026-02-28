//! Pure manifest validation — no I/O, no async.
//!
//! All functions in this module are synchronous and take data in, returning
//! data out. Zero imports from `tokio`, `std::fs`, `crate::infra`,
//! `crate::commands`, or `crate::application`.

#![allow(dead_code)] // Refactor in progress — some functions defined ahead of callers

use anyhow::Result;
use polis_common::agent::AgentManifest;
use regex::Regex;
use std::sync::LazyLock;

use crate::domain::error::AgentError;

/// Same rule enforced by `generate-agent.sh`; checked here before any
/// path interpolation to prevent path-traversal (CWE-22).
pub static AGENT_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Safety: this is a compile-time constant pattern — cannot fail.
    #[allow(clippy::expect_used)]
    Regex::new(r"^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$").expect("valid regex")
});

/// Shell metacharacters that must not appear in runtime.command.
pub static SHELL_METACHAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"[;|&`$()\\<>!#~*\[\]{}]").expect("valid regex")
});

/// Platform-reserved ports that agents must not use.
pub const PLATFORM_PORTS: &[u16] = &[
    80, 443, 8080, 8443, 9090, 9091, 9092, 9093, 3000, 5432, 6379, 27017,
];

/// Allowed prefixes for readWritePaths (same as generate-agent.sh).
pub const ALLOWED_RW_PREFIXES: &[&str] = &["/opt/polis/agents/", "/tmp/", "/var/tmp/"];

/// Validate a parsed `AgentManifest` against the same rules as
/// `generate-agent.sh`. Returns `Ok(())` or an error listing all violations.
///
/// Preserves all ~15 checks from the original implementation:
/// 1. `api_version` == "polis.dev/v1"
/// 2. `kind` == "`AgentPlugin`"
/// 3. `metadata.name` matches `AGENT_NAME_RE`
/// 4. `packaging` == "script"
/// 5. `runtime.command` starts with '/'
/// 6. `runtime.command` has no shell metacharacters
/// 7. `runtime.user` != "root"
/// 8. `spec.install` has no ".." (path traversal)
/// 9. `spec.init` has no ".." (path traversal)
///    10+. Port conflicts with `PLATFORM_PORTS`
///    N+. `readWritePaths` prefix validation against `ALLOWED_RW_PREFIXES`
///
/// Pure function — no I/O, no async.
///
/// # Errors
///
/// Returns an error listing all validation violations if any check fails.
pub fn validate_full_manifest(manifest: &AgentManifest) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    if manifest.api_version != "polis.dev/v1" {
        errors.push("Unsupported apiVersion. Expected polis.dev/v1".to_string());
    }

    if manifest.kind != "AgentPlugin" {
        errors.push("Unsupported kind. Expected AgentPlugin".to_string());
    }

    if !AGENT_NAME_RE.is_match(&manifest.metadata.name) {
        errors.push(format!(
            "metadata.name '{}' must be lowercase alphanumeric with hyphens",
            manifest.metadata.name
        ));
    }

    if manifest.spec.packaging != "script" {
        errors.push("Only 'script' packaging is supported".to_string());
    }

    let cmd = &manifest.spec.runtime.command;
    if !cmd.starts_with('/') {
        errors.push("runtime.command must start with /".to_string());
    }
    if SHELL_METACHAR_RE.is_match(cmd) {
        errors.push("runtime.command contains shell metacharacters".to_string());
    }

    if manifest.spec.runtime.user == "root" {
        errors.push("Agents must run as unprivileged user (not root)".to_string());
    }

    // install/init path escape check
    if manifest.spec.install.contains("..") {
        errors.push("spec.install path escapes agent directory".to_string());
    }
    if let Some(init) = &manifest.spec.init
        && init.contains("..")
    {
        errors.push("spec.init path escapes agent directory".to_string());
    }

    // Port conflict check
    for port_spec in &manifest.spec.ports {
        let port = port_spec.default;
        if PLATFORM_PORTS.contains(&port) {
            errors.push(format!("Port {port} conflicts with platform service"));
        }
    }

    // readWritePaths prefix check
    if let Some(security) = &manifest.spec.security {
        for path in &security.read_write_paths {
            let allowed = ALLOWED_RW_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix));
            if !allowed {
                errors.push(format!(
                    "readWritePaths entry '{path}' is outside allowed prefixes: {}",
                    ALLOWED_RW_PREFIXES.join(", ")
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(AgentError::ValidationFailed(errors.join("\n")).into())
    }
}

/// Returns `true` if `name` is a valid agent name.
///
/// Valid names match `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$` — lowercase
/// alphanumeric with interior hyphens, 1–63 characters total.
///
/// Pure function — no I/O, no async.
pub fn is_valid_agent_name(name: &str) -> bool {
    AGENT_NAME_RE.is_match(name)
}
