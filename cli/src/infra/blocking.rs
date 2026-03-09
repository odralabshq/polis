//! Async wrapper for blocking I/O with consistent error handling.
//!
//! Provides [`spawn_blocking_io`] — a single entry point for offloading
//! synchronous work to the Tokio blocking thread pool. Replaces 8+
//! inconsistent `spawn_blocking` + error-handling patterns across the
//! infra layer with one canonical implementation.
//!
//! **Dependency rule:** This module has zero intra-infra dependencies.
//! It depends only on `tokio` and `anyhow`.

use anyhow::{Context, Result};

/// Runs a blocking closure on the Tokio blocking thread pool and flattens
/// the nested `Result<Result<T>>` into a single `Result<T>`.
///
/// # Errors
///
/// * If the closure returns `Err(e)`, the error is propagated as-is.
/// * If the spawned task panics, the returned error contains
///   `"{operation} task panicked"`.
///
/// # Examples
///
/// ```ignore
/// let content = spawn_blocking_io("state load", || {
///     std::fs::read_to_string("/some/path").map_err(Into::into)
/// }).await?;
/// ```
pub async fn spawn_blocking_io<F, T>(operation: &str, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .with_context(|| format!("{operation} task panicked"))?
}
