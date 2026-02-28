//! VM provisioning operations: config transfer, env generation, cert/secret generation.
//!
//! Re-exports from `crate::workspace::vm` for backward compatibility.
//! The actual implementation lives in `workspace/vm.rs` until task 27.2.

// Re-exports enabled when callers are migrated (task 27.2).
// pub use crate::workspace::vm::{
//     generate_certs_and_secrets, generate_env_content, transfer_config, validate_tarball_paths,
// };
