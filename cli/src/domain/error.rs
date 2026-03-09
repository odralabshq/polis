//! Typed domain error enums.
//!
//! This module has zero imports from `crate::infra`, `crate::commands`,
//! `crate::application`, `tokio`, `std::fs`, `std::process`, or `std::net`.
//! All error types implement `thiserror::Error` and convert to `anyhow::Error`
//! via the `?` operator.

use thiserror::Error;

// ── Workspace errors ──────────────────────────────────────────────────────────

/// Errors related to workspace lifecycle and identity.
#[derive(Debug, Error)]
#[allow(dead_code)] // Variants defined ahead of callers
pub enum WorkspaceError {
    #[error("Workspace not found. Run 'polis start' to create one.")]
    NotFound,

    #[error("Workspace is stopped. Run 'polis start' to resume.")]
    Stopped,

    #[error("Workspace is already running.")]
    AlreadyRunning,

    #[error(
        "Workspace is running with agent '{active}'. Remove it first:\n  polis agent remove {active}\n  polis agent activate {requested}"
    )]
    AgentMismatch { active: String, requested: String },

    #[error("Workspace is not running. Start it first:\n  polis start")]
    NotRunning,

    #[error("VM still starting after {0}s. Diagnose:\n  polis doctor")]
    StartTimeout(u64),
}

// ── Agent errors ──────────────────────────────────────────────────────────────

/// Errors related to agent management.
#[derive(Debug, Error)]
#[allow(dead_code)] // Variants defined ahead of callers
pub enum AgentError {
    #[error("Agent '{0}' not found.")]
    NotFound(String),

    #[error("Agent '{0}' already exists. Remove it first: polis agent remove {0}")]
    AlreadyExists(String),

    #[error("No active agent. Install one with: polis agent install --path <path>")]
    NoActiveAgent,

    #[error("Invalid agent name '{0}': must match ^[a-z0-9]([a-z0-9-]{{0,61}}[a-z0-9])?$")]
    InvalidName(String),

    #[error("Agent manifest validation failed:\n{0}")]
    ValidationFailed(String),
}

// ── Swap errors ───────────────────────────────────────────────────────────────

/// Errors related to agent swap operations.
/// Each variant includes recovery information for the user.
#[derive(Debug, Error)]
pub enum SwapError {
    #[error("Failed to stop agent '{agent}'.\n  Recovery: {recovery}")]
    StopFailed { agent: String, recovery: String },

    #[error(
        "Failed to start agent '{agent}' after stopping '{old_agent}'.\n  Old agent was restored.\n  Recovery: {recovery}"
    )]
    StartFailedRolledBack {
        agent: String,
        old_agent: String,
        recovery: String,
    },

    #[error(
        "Failed to start agent '{agent}' after stopping '{old_agent}', and rollback also failed.\n  Original: {original}\n  Rollback: {rollback}\n  Recovery for '{old_agent}': {old_recovery}\n  Recovery for '{agent}': {new_recovery}"
    )]
    StartFailedRollbackFailed {
        agent: String,
        old_agent: String,
        original: String,
        rollback: String,
        old_recovery: String,
        new_recovery: String,
    },
}

// ── Remove errors ─────────────────────────────────────────────────────────────

/// Errors related to agent removal operations.
/// Each variant includes recovery information for the user.
#[derive(Debug, Error)]
pub enum RemoveError {
    #[error("Agent '{agent}' is not installed.")]
    NotInstalled { agent: String },

    #[error("Invalid agent name '{0}'")]
    InvalidName(String),

    #[error("VM is not running. Start it first:\n  polis start")]
    NotRunning,

    #[error("Failed to remove overlay symlink for '{agent}'.\n  Recovery: {recovery}")]
    SymlinkRemovalFailed { agent: String, recovery: String },

    #[error("Failed at step '{step}' while removing agent '{agent}'.\n  Recovery: {recovery}")]
    StepFailed {
        agent: String,
        step: String,
        recovery: String,
    },

    #[error(
        "Failed at step '{step}' while removing agent '{agent}', and compensating action also failed.\n  Original: {original}\n  Compensating: {compensating}\n  Recovery: {recovery}"
    )]
    StepFailedWithCompensatingFailure {
        agent: String,
        step: String,
        original: String,
        compensating: String,
        recovery: String,
    },
}

// ── Config errors ─────────────────────────────────────────────────────────────

/// Errors related to configuration key/value validation.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Unknown setting: {key}\n\nValid settings: {valid}")]
    UnknownKey { key: String, valid: String },

    #[error("Invalid value for {key}: {value}\n\nValid values: {valid}")]
    InvalidValue {
        key: String,
        value: String,
        valid: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_error_stop_failed_formats_correctly() {
        let err = SwapError::StopFailed {
            agent: "old-agent".to_string(),
            recovery: "docker compose down && polis agent activate new-agent".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("old-agent"));
        assert!(msg.contains("stop"));
        assert!(msg.contains("docker compose"));
    }

    #[test]
    fn swap_error_start_failed_rolled_back_formats_correctly() {
        let err = SwapError::StartFailedRolledBack {
            agent: "new-agent".to_string(),
            old_agent: "old-agent".to_string(),
            recovery: "polis agent activate new-agent".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("new-agent"));
        assert!(msg.contains("old-agent"));
        assert!(msg.contains("restored"));
        assert!(msg.contains("polis agent activate"));
    }

    #[test]
    fn swap_error_start_failed_rollback_failed_formats_correctly() {
        let err = SwapError::StartFailedRollbackFailed {
            agent: "new-agent".to_string(),
            old_agent: "old-agent".to_string(),
            original: "Port 8080 already in use".to_string(),
            rollback: "Container not found".to_string(),
            old_recovery: "docker compose -f base.yml -f old.yml up -d".to_string(),
            new_recovery: "docker compose -f base.yml -f new.yml up -d".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("new-agent"));
        assert!(msg.contains("old-agent"));
        assert!(msg.contains("Port 8080"));
        assert!(msg.contains("Container not found"));
        assert!(msg.contains("old.yml"));
        assert!(msg.contains("new.yml"));
    }
}
