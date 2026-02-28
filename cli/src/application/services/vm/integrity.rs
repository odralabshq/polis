//! VM integrity operations: config hash writing and image digest verification.
//!
//! Re-exports from `crate::workspace::vm` for backward compatibility.
//! The actual implementation lives in `workspace/vm.rs` until task 27.2.

// Re-exports enabled when callers are migrated (task 27.2).
// pub use crate::workspace::vm::{sha256_file, write_config_hash};
