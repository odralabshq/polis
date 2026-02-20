//! Workspace lifecycle management.
//!
//! This module provides abstractions for workspace operations:
//! - `image` — Image download, verification, and caching
//! - `vm` — VM lifecycle operations
//! - `health` — Health checks and readiness waiting

pub mod health;
pub mod image;
pub mod vm;
