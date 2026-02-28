//! VM lifecycle operations: create, start, stop, delete, restart, state.
//!
//! Re-exports from `crate::workspace::vm` for backward compatibility.
//! The actual implementation lives in `workspace/vm.rs` until task 27.2.

// Re-exports enabled when callers are migrated (task 27.2).
// pub use crate::workspace::vm::{
//     VmState, create, delete, exists, resolve_vm_ip, restart, start, state, stop,
// };
