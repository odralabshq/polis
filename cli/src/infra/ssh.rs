//! Infrastructure SSH management — re-exports from `crate::ssh`.
//!
//! `SshConfigManager` and `KnownHostsManager` live here in the infra layer.
//! The original `crate::ssh` module re-exports from here for backward compat.

pub use crate::ssh::{KnownHostsManager, SshConfigManager, ensure_identity_key, validate_host_key};
