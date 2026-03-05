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

    #[error("Workspace is running with agent '{active}'. Remove it first:\n  polis agent delete {active}\n  polis agent start {requested}")]
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

    #[error("No active agent. Install one with: polis agent add --path <path>")]
    NoActiveAgent,

    #[error("Invalid agent name '{0}': must match ^[a-z0-9]([a-z0-9-]{{0,61}}[a-z0-9])?$")]
    InvalidName(String),

    #[error("Agent manifest validation failed:\n{0}")]
    ValidationFailed(String),
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
