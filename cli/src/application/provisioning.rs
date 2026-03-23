//! Checkpoint-based provisioning runner (saga pattern).
//!
//! This module implements a multi-step provisioning workflow with resume
//! capability. If a step fails, the checkpoint is persisted so that the next
//! invocation can skip already-completed steps and resume from the failure
//! point.
//!
//! # Layer compliance
//! Imports only from `crate::domain` and `crate::application::ports`.
//! No I/O primitives, no infrastructure types, no presentation concerns.
//!
//! # Requirements
//! 4.2, 4.3, 4.4, 4.5, 4.6, 9.2

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;

use crate::application::ports::{ProgressReporter, WorkspaceStateStore};
use crate::domain::workspace::{ProvisioningCheckpoint, WorkspaceState};

// ── ProvisioningContext ───────────────────────────────────────────────────────

/// Shared references passed to every [`ProvisioningStep`] during execution.
///
/// The three type parameters correspond to the provisioner, asset extractor,
/// and file hasher port traits respectively. Using generics (rather than
/// `dyn Trait`) keeps the context zero-cost and avoids object-safety
/// constraints on the step trait.
pub struct ProvisioningContext<'a, P, A, H> {
    /// VM provisioner (implements `VmProvisioner`).
    pub provisioner: &'a P,
    /// Asset extractor (implements `AssetExtractor`).
    pub assets: &'a A,
    /// File hasher (implements `FileHasher`).
    pub hasher: &'a H,
}

// ── ProvisioningStep trait ────────────────────────────────────────────────────

/// A single idempotent step in a provisioning workflow.
///
/// Each step must have a stable, unique `id()` that is used to track
/// completion in the [`ProvisioningCheckpoint`]. Steps are expected to be
/// idempotent — re-running a completed step should be safe.
///
/// The `R` type parameter is the concrete [`ProgressReporter`] implementation.
/// This allows the trait to be object-safe (dyn-compatible) while still
/// accepting a reporter without boxing.
pub trait ProvisioningStep<P, A, H, R: ProgressReporter> {
    /// Stable identifier for this step (e.g. `"launch-vm"`, `"pull-images"`).
    ///
    /// Must be unique within a workflow and must not change between releases
    /// (it is persisted in the checkpoint).
    fn id(&self) -> &'static str;

    /// Execute this step.
    ///
    /// Returns a boxed future so that the trait is dyn-compatible and steps
    /// can be stored in a `&[&dyn ProvisioningStep<…>]` slice.
    fn execute<'a>(
        &'a self,
        ctx: &'a ProvisioningContext<'a, P, A, H>,
        reporter: &'a R,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>;
}

// ── ProvisioningRunner ────────────────────────────────────────────────────────

/// Saga runner for multi-step provisioning workflows.
///
/// The runner loads the current [`WorkspaceState`] (or creates a default),
/// extracts (or initialises) the [`ProvisioningCheckpoint`] for the given
/// `workflow_id`, and iterates over the provided steps:
///
/// - Steps whose IDs already appear in `completed_steps` are **skipped**.
/// - Steps that succeed have their ID **appended** to `completed_steps` and
///   the checkpoint is **persisted** immediately.
/// - If a step **fails**, `failed_step` is set, the checkpoint is persisted,
///   and the error is propagated to the caller.
/// - When **all** steps complete, the checkpoint is cleared by setting
///   `state.provisioning = None` and persisting the state.
///
/// This guarantees monotonic progress: `completed_steps` only ever grows
/// within a single run (Requirement 4.6).
pub struct ProvisioningRunner<'a, S: WorkspaceStateStore> {
    workflow_id: &'static str,
    state_mgr: &'a S,
}

impl<'a, S: WorkspaceStateStore> ProvisioningRunner<'a, S> {
    /// Create a new runner for the given workflow.
    ///
    /// - `workflow_id` — stable identifier for this workflow type (e.g.
    ///   `"create-v1"`). Used to match checkpoints across invocations.
    /// - `state_mgr` — port for loading and persisting [`WorkspaceState`].
    pub fn new(workflow_id: &'static str, state_mgr: &'a S) -> Self {
        Self {
            workflow_id,
            state_mgr,
        }
    }

    /// Execute the provisioning workflow.
    ///
    /// # Algorithm
    ///
    /// 1. Load current state (or use a default `WorkspaceState`).
    /// 2. Extract existing checkpoint for this `workflow_id`, or create a
    ///    fresh one.
    /// 3. For each step:
    ///    a. If `step.id()` is in `checkpoint.completed_steps` → skip.
    ///    b. Otherwise execute the step.
    ///    c. On success: append `step.id()` to `completed_steps`, persist.
    ///    d. On failure: set `failed_step`, persist, return `Err`.
    /// 4. All steps complete: set `state.provisioning = None`, persist.
    ///
    /// # Errors
    ///
    /// Returns the first step error encountered, after persisting the failure
    /// checkpoint. Also returns errors from state load/save operations.
    pub async fn run<P, A, H, R>(
        &self,
        steps: &[&dyn ProvisioningStep<P, A, H, R>],
        ctx: &ProvisioningContext<'_, P, A, H>,
        reporter: &R,
    ) -> Result<()>
    where
        R: ProgressReporter,
    {
        // Step 1: load current state (or default).
        let mut state: WorkspaceState = self.state_mgr.load_async().await?.unwrap_or_default();

        // Step 2: get or create checkpoint for this workflow.
        let mut checkpoint = state
            .provisioning
            .take()
            .filter(|cp| cp.workflow_id == self.workflow_id)
            .unwrap_or_else(|| ProvisioningCheckpoint {
                workflow_id: self.workflow_id.to_owned(),
                completed_steps: Vec::new(),
                failed_step: None,
            });

        // Step 3: iterate steps.
        for step in steps {
            let step_id = step.id();

            // 3a. Skip already-completed steps (resume support — Req 4.5).
            if checkpoint.is_step_done(step_id) {
                continue;
            }

            // 3b. Execute the step.
            match step.execute(ctx, reporter).await {
                Ok(()) => {
                    // 3c. Success: append to completed_steps (monotonic — Req 4.6).
                    checkpoint.completed_steps.push(step_id.to_owned());
                    // Clear any previous failed_step marker now that we're progressing.
                    checkpoint.failed_step = None;
                    // Persist checkpoint after each successful step (Req 4.2).
                    state.provisioning = Some(checkpoint.clone());
                    self.state_mgr.save_async(&state).await?;
                }
                Err(err) => {
                    // 3d. Failure: record failed step, persist, propagate (Req 4.3).
                    checkpoint.failed_step = Some(step_id.to_owned());
                    state.provisioning = Some(checkpoint);
                    self.state_mgr.save_async(&state).await?;
                    return Err(err);
                }
            }
        }

        // Step 4: all steps complete — clear checkpoint (Req 4.4).
        state.provisioning = None;
        self.state_mgr.save_async(&state).await?;

        Ok(())
    }
}
