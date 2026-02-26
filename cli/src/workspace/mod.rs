//! Workspace lifecycle management.
//!
//! This module provides abstractions for workspace operations:
//! - `image` — Image download, verification, and caching
//! - `vm` — VM lifecycle operations
//! - `health` — Health checks and readiness waiting
//! - `digest` — Image digest verification against the embedded release manifest

pub mod digest;
pub mod health;
pub mod image;
pub mod vm;

/// Path to `docker-compose.yml` inside the VM.
/// MAINT-001: Centralized constant used by status, update, vm, and health modules.
pub const COMPOSE_PATH: &str = "/opt/polis/docker-compose.yml";

/// Docker container name inside the VM.
/// MAINT-002: Centralized constant for container references.
pub const CONTAINER_NAME: &str = "polis-workspace";
