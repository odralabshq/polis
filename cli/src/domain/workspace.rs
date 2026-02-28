//! Workspace domain types and pure validation functions.
//!
//! This module is intentionally free of I/O, async, and external layer imports.
//! All functions take data in and return data out.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::error::WorkspaceError;

/// Workspace state persisted to `~/.polis/state.json`.
///
/// The `created_at` field accepts the legacy `started_at` name for backward
/// compatibility with older state files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
    /// Workspace identifier (e.g., "polis-abc123def456").
    pub workspace_id: String,
    /// When workspace was created (accepts legacy `"started_at"` field).
    #[serde(alias = "started_at")]
    pub created_at: DateTime<Utc>,
    /// Image SHA256 used to create workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_sha256: Option<String>,
    /// Custom image source (path or URL) used to create workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_source: Option<String>,
    /// Currently active agent name, or None for control-plane-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_agent: Option<String>,
}

/// SEC-004: Validates workspace ID format.
///
/// A valid workspace ID is `polis-` followed by exactly 16 lowercase hex characters.
///
/// # Errors
///
/// Returns an error if the ID doesn't match the expected format.
pub fn validate_workspace_id(id: &str) -> Result<()> {
    if !id.starts_with("polis-") || id.len() != 22 {
        return Err(WorkspaceError::InvalidId(id.to_string()).into());
    }
    if !id[6..].chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(WorkspaceError::InvalidId(id.to_string()).into());
    }
    Ok(())
}

/// Check that the host architecture is amd64.
///
/// Sysbox (the container runtime used by Polis) does not support arm64 as of v0.6.7.
///
/// # Errors
///
/// Returns an error if the host is arm64 / aarch64.
#[allow(dead_code)] // Called from workspace_start service — not yet wired to binary
pub fn check_architecture() -> Result<()> {
    if std::env::consts::ARCH == "aarch64" {
        anyhow::bail!(
            "Polis requires an amd64 host. \
Sysbox (the container runtime used by Polis) does not support arm64 as of v0.6.7. \
Please use an amd64 machine."
        );
    }
    Ok(())
}

/// Path to `docker-compose.yml` inside the VM.
/// MAINT-001: Centralized constant used by status, update, vm, and health modules.
pub const COMPOSE_PATH: &str = "/opt/polis/docker-compose.yml";

/// Docker container name inside the VM.
/// MAINT-002: Centralized constant for container references.
pub const CONTAINER_NAME: &str = "polis-workspace";

/// Path to the polis project root inside the VM.
pub const VM_ROOT: &str = "/opt/polis";

/// Encode bytes as lowercase hex string.
///
/// Pure utility used by update signature verification and image digest computation.
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[(b >> 4) as usize]));
        out.push(char::from(HEX[(b & 0xf) as usize]));
    }
    out
}

/// Generate a unique workspace identifier.
///
/// Format: `polis-` followed by 16 lowercase hex characters.
/// Entropy sources: nanosecond timestamp and two independent `RandomState` hashes.
#[must_use]
#[allow(dead_code)] // Called from workspace_start service — not yet wired to binary
pub fn generate_workspace_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    hasher.write_u64(RandomState::new().build_hasher().finish());
    hasher.write_u64(RandomState::new().build_hasher().finish());
    format!("polis-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Valid 22-character workspace ID for tests (polis- + 16 hex chars).
    const TEST_WORKSPACE_ID: &str = "polis-0123456789abcdef";

    #[test]
    fn test_validate_workspace_id_valid_format() {
        assert!(validate_workspace_id(TEST_WORKSPACE_ID).is_ok());
        assert!(validate_workspace_id("polis-aaaaaaaaaaaaaaaa").is_ok());
        assert!(validate_workspace_id("polis-AAAAAAAAAAAAAAAA").is_ok());
    }

    #[test]
    fn test_validate_workspace_id_rejects_short_id() {
        assert!(validate_workspace_id("polis-abc123").is_err());
        assert!(validate_workspace_id("polis-test").is_err());
    }

    #[test]
    fn test_validate_workspace_id_rejects_wrong_prefix() {
        assert!(validate_workspace_id("other-0123456789abcdef").is_err());
    }

    #[test]
    fn test_validate_workspace_id_rejects_non_hex_chars() {
        assert!(validate_workspace_id("polis-ghijklmnopqrstuv").is_err());
    }

    #[test]
    fn test_hex_encode_empty_returns_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_single_byte() {
        assert_eq!(hex_encode(&[0x00]), "00");
        assert_eq!(hex_encode(&[0xff]), "ff");
        assert_eq!(hex_encode(&[0xab]), "ab");
    }

    #[test]
    fn test_hex_encode_multiple_bytes() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn check_architecture_passes_on_non_arm64() {
        if std::env::consts::ARCH == "aarch64" {
            let err = check_architecture().expect_err("expected Err on arm64");
            let msg = err.to_string();
            assert!(msg.contains("amd64"), "error should mention amd64: {msg}");
        } else {
            assert!(
                check_architecture().is_ok(),
                "check_architecture() should succeed on non-arm64 host"
            );
        }
    }
}
