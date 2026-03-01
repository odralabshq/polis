//! Workspace domain types and pure validation functions.
//!
//! This module is intentionally free of I/O, async, and external layer imports.
//! All functions take data in and return data out.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Workspace state persisted to `~/.polis/state.json`.
///
/// The `created_at` field accepts the legacy `started_at` name for backward
/// compatibility with older state files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
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

/// Path to the guest query script inside the VM.
/// Used by status and doctor services to gather system info via a single exec call,
/// avoiding Multipass Windows pipe/buffer issues with piped commands.
pub const QUERY_SCRIPT: &str = "/opt/polis/scripts/polis-query.sh";

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

#[cfg(test)]
mod tests {
    use super::*;

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
