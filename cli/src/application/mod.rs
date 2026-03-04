//! Application layer — port trait definitions and use-case orchestration.
//!
//! This module depends only on `crate::domain` — never on `crate::infra`,
//! `crate::commands`, or `crate::output`.

pub mod ports;
pub mod services;

#[allow(unused_imports)]
pub use ports::{
    CommandRunner, FileTransfer, HealthProbe, InstanceInspector, InstanceLifecycle, InstanceSpec,
    InstanceState, LocalArtifactWriter, POLIS_INSTANCE, ProgressReporter, ShellExecutor,
    VmProvisioner, WorkspaceStateStore,
};
