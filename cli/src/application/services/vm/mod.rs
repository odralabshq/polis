//! Application services for VM lifecycle, provisioning, and integrity.
//!
//! These modules decompose the original `workspace/vm.rs` into focused
//! application services. Each module imports only from `crate::domain` and
//! `crate::application::ports`.

pub mod health;
pub mod integrity;
pub mod lifecycle;
pub mod provision;
pub mod services;
