//! Application building block for VM lifecycle, provisioning, and integrity.
//!
//! These modules decompose the original `workspace/vm.rs` into focused
//! application building blocks. Each module imports only from `crate::domain` and
//! `crate::application::ports`.

pub mod compose;
pub mod health;
pub mod integrity;
pub mod lifecycle;
pub mod provision;
pub mod pull;

#[cfg(test)]
pub(crate) mod test_support;
