//! Application service — workspace start use-case.
//!
//! Handles only workspace lifecycle concerns (no agent activation, no SSH).
//! Imports only from `crate::domain` and `crate::application::ports`.
//!
//! # Requirements
//! 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 5.8, 5.9, 5.10, 9.1, 9.3, 9.4, 9.5, 9.6, 9.7, 13.7

use std::future::Future;
use std::pin::Pin;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::application::ports::{
    AssetExtractor, FileHasher, ProgressReporter, VmProvisioner, WorkspaceStateStore,
};
use crate::application::provisioning::{ProvisioningContext, ProvisioningRunner, ProvisioningStep};
use crate::application::vm::{
    compose::{set_active_overlay, set_ready_marker},
    health::wait_ready,
    integrity::{verify_image_digests, write_config_hash},
    lifecycle::{self as vm, VmState},
    provision::{generate_certs_and_secrets, persist_vm_ip, transfer_config},
    pull::pull_images,
};
use crate::domain::error::WorkspaceError;
use crate::domain::workspace::{StartAction, WorkspaceState, resolve_action};

// ── StartOptions ──────────────────────────────────────────────────────────────

/// Options for the `start` use-case.
///
/// No `agent` field — agent activation is handled by `agent_activate.rs`.
/// No `ssh` or `local_fs` — SSH provisioning is handled by `ssh.rs`.
pub struct StartOptions<'a, R: ProgressReporter> {
    pub reporter: &'a R,
    pub assets_dir: &'a std::path::Path,
    pub version: &'a str,
    pub start_timeout: std::time::Duration,
}

// ── StartOutcome ──────────────────────────────────────────────────────────────

/// Outcome of the `start` use-case.
#[derive(Debug)]
pub enum StartOutcome {
    /// Workspace was already running with no incomplete provisioning.
    AlreadyRunning { active_agent: Option<String> },
    /// Workspace was freshly created and started.
    Created { active_agent: Option<String> },
    /// A stopped workspace was restarted.
    Restarted { active_agent: Option<String> },
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Start the workspace, creating it if needed.
///
/// Uses `resolve_action` for the domain decision, then dispatches to the
/// appropriate path. No agent logic, no SSH logic.
///
/// # Errors
///
/// Returns an error if any step of the provisioning workflow fails, or if the
/// VM remains in `Starting` state after `opts.start_timeout`.
pub async fn start<P, S, A, H, R>(
    provisioner: &P,
    state_mgr: &S,
    assets: &A,
    hasher: &H,
    opts: StartOptions<'_, R>,
) -> Result<StartOutcome>
where
    P: VmProvisioner,
    S: WorkspaceStateStore,
    A: AssetExtractor,
    H: FileHasher,
    R: ProgressReporter,
{
    crate::domain::workspace::check_architecture()?;

    let vm_state = vm::state(provisioner).await?;
    let persisted = state_mgr.load_async().await?;
    let has_incomplete = persisted
        .as_ref()
        .and_then(|s| s.provisioning.as_ref())
        .is_some();

    let action = resolve_action(vm_state, has_incomplete);

    match action {
        StartAction::Create | StartAction::ResumeProvisioning => {
            run_provisioning(provisioner, state_mgr, assets, hasher, &opts).await?;
            Ok(StartOutcome::Created { active_agent: None })
        }

        StartAction::Restart => {
            restart_workspace(provisioner, state_mgr, assets, &opts, persisted).await
        }

        StartAction::WaitThenResolve => {
            wait_then_resolve(provisioner, state_mgr, assets, hasher, opts, has_incomplete).await
        }

        StartAction::AlreadyRunning => {
            let active_agent = persisted.and_then(|s| s.active_agent);
            Ok(StartOutcome::AlreadyRunning { active_agent })
        }
    }
}

// ── Create / ResumeProvisioning path ─────────────────────────────────────────

/// Run the full provisioning workflow (or resume from checkpoint).
async fn run_provisioning<P, A, H, R>(
    provisioner: &P,
    state_mgr: &impl WorkspaceStateStore,
    assets: &A,
    hasher: &H,
    opts: &StartOptions<'_, R>,
) -> Result<()>
where
    P: VmProvisioner,
    A: AssetExtractor,
    H: FileHasher,
    R: ProgressReporter,
{
    // Pre-compute config hash before any I/O (needed by WriteConfigHash step).
    let tar_path = opts.assets_dir.join("polis-setup.config.tar");
    let config_hash = hasher
        .sha256_file(&tar_path)
        .context("computing config tarball SHA256")?;

    let runner = ProvisioningRunner::new("create-v1", state_mgr);
    let ctx = ProvisioningContext {
        provisioner,
        assets,
        hasher,
    };

    let launch_vm = LaunchVm;
    let transfer_config_step = TransferConfigStep {
        assets_dir: opts.assets_dir,
        version: opts.version,
    };
    let persist_ip = PersistVmIp;
    let generate_certs = GenerateCerts;
    let pull_images_step = PullImages;
    let verify_digests = VerifyDigests;
    let set_base_overlay = SetBaseOverlay;
    let set_ready = SetReadyMarker;
    let start_services = StartServices;
    let wait_health = WaitHealth;
    let write_hash = WriteConfigHash { hash: config_hash };
    let persist = FinalizeProvisioning;

    let steps: &[&dyn ProvisioningStep<P, A, H, R>] = &[
        &launch_vm,
        &transfer_config_step,
        &persist_ip,
        &generate_certs,
        &pull_images_step,
        &verify_digests,
        &set_base_overlay,
        &set_ready,
        &start_services,
        &wait_health,
        &write_hash,
        &persist,
    ];

    runner.run(steps, &ctx, opts.reporter).await
}

// ── Provisioning step structs ─────────────────────────────────────────────────

/// Generate a `ProvisioningStep` impl with the standard where-clause and
/// `Pin<Box<dyn Future>>` return type.
macro_rules! impl_provisioning_step {
    ($ty:ty, $id:expr, $ctx:ident, $reporter:ident, $body:expr) => {
        impl<P, A, H, R> ProvisioningStep<P, A, H, R> for $ty
        where
            P: VmProvisioner,
            A: AssetExtractor,
            H: FileHasher,
            R: ProgressReporter,
        {
            fn id(&self) -> &'static str {
                $id
            }
            fn execute<'a>(
                &'a self,
                $ctx: &'a ProvisioningContext<'a, P, A, H>,
                $reporter: &'a R,
            ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
                Box::pin(async move { $body })
            }
        }
    };
}

struct LaunchVm;
impl_provisioning_step!(
    LaunchVm,
    "launch-vm",
    ctx,
    reporter,
    vm::create(ctx.provisioner, ctx.assets, reporter).await
);

struct TransferConfigStep<'b> {
    assets_dir: &'b std::path::Path,
    version: &'b str,
}

impl<P, A, H, R> ProvisioningStep<P, A, H, R> for TransferConfigStep<'_>
where
    P: VmProvisioner,
    A: AssetExtractor,
    H: FileHasher,
    R: ProgressReporter,
{
    fn id(&self) -> &'static str {
        "transfer-config"
    }
    fn execute<'a>(
        &'a self,
        ctx: &'a ProvisioningContext<'a, P, A, H>,
        reporter: &'a R,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        let assets_dir = self.assets_dir;
        let version = self.version;
        Box::pin(async move {
            reporter.begin_stage("securing workspace...");
            transfer_config(ctx.provisioner, assets_dir, version)
                .await
                .context("transferring config to VM")
        })
    }
}

struct PersistVmIp;
impl_provisioning_step!(PersistVmIp, "persist-vm-ip", ctx, _reporter, {
    persist_vm_ip(ctx.provisioner).await.ok();
    Ok(())
});

struct GenerateCerts;
impl_provisioning_step!(
    GenerateCerts,
    "generate-certs",
    ctx,
    _reporter,
    generate_certs_and_secrets(ctx.provisioner)
        .await
        .context("generating certificates and secrets")
);

struct PullImages;
impl_provisioning_step!(PullImages, "pull-images", ctx, reporter, {
    reporter.begin_stage("verifying components...");
    pull_images(ctx.provisioner, reporter)
        .await
        .context("pulling Docker images")
});

struct VerifyDigests;
impl_provisioning_step!(
    VerifyDigests,
    "verify-digests",
    ctx,
    reporter,
    verify_image_digests(ctx.provisioner, ctx.assets, reporter)
        .await
        .context("verifying image digests")
);

struct SetBaseOverlay;
impl_provisioning_step!(
    SetBaseOverlay,
    "set-base-overlay",
    ctx,
    _reporter,
    set_active_overlay(ctx.provisioner, None).await
);

struct SetReadyMarker;
impl_provisioning_step!(
    SetReadyMarker,
    "set-ready-marker",
    ctx,
    _reporter,
    set_ready_marker(ctx.provisioner, true).await
);

struct StartServices;
impl_provisioning_step!(StartServices, "start-services", ctx, _reporter, {
    ctx.provisioner
        .exec(&["sudo", "systemctl", "start", "polis"])
        .await
        .context("starting polis service")?;
    Ok(())
});

struct WaitHealth;
impl_provisioning_step!(
    WaitHealth,
    "wait-health",
    ctx,
    reporter,
    wait_ready(ctx.provisioner, reporter, false, "workspace ready").await
);

struct WriteConfigHash {
    hash: String,
}

impl<P, A, H, R> ProvisioningStep<P, A, H, R> for WriteConfigHash
where
    P: VmProvisioner,
    A: AssetExtractor,
    H: FileHasher,
    R: ProgressReporter,
{
    fn id(&self) -> &'static str {
        "write-config-hash"
    }
    fn execute<'a>(
        &'a self,
        ctx: &'a ProvisioningContext<'a, P, A, H>,
        _reporter: &'a R,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        let hash = self.hash.clone();
        Box::pin(async move {
            write_config_hash(ctx.provisioner, &hash)
                .await
                .context("writing config hash")
        })
    }
}

/// Sentinel step that causes the `ProvisioningRunner` to clear the checkpoint
/// only after `wait-health` has succeeded.
///
/// The runner persists the checkpoint after each step and clears it
/// (`provisioning = None`) when all steps complete. By placing this step last,
/// we guarantee the checkpoint is cleared — and state is considered fully
/// provisioned — only after health has been confirmed (BUG-3 fix, Req 5.8, 9.3).
struct FinalizeProvisioning;
impl_provisioning_step!(
    FinalizeProvisioning,
    "finalize-provisioning",
    _ctx,
    _reporter,
    // No-op: the runner clears the checkpoint after this step completes,
    // which is the desired side-effect. All real work is done by prior steps.
    Ok(())
);

// ── Restart path ──────────────────────────────────────────────────────────────

/// Restart a stopped workspace.
///
/// Does NOT use `ProvisioningRunner` — the restart sequence is short (5 steps),
/// every step is idempotent, and re-running from the top is cheap.
/// State is persisted AFTER `wait_ready` succeeds (Req 5.8).
async fn restart_workspace<P, A, R>(
    provisioner: &P,
    state_mgr: &impl WorkspaceStateStore,
    assets: &A,
    opts: &StartOptions<'_, R>,
    persisted: Option<WorkspaceState>,
) -> Result<StartOutcome>
where
    P: VmProvisioner,
    A: AssetExtractor,
    R: ProgressReporter,
{
    opts.reporter.begin_stage("starting workspace...");
    vm::start(provisioner).await?;
    opts.reporter.complete_stage();

    // Persist VM IP for container access (best-effort).
    persist_vm_ip(provisioner).await.ok();

    opts.reporter.begin_stage("verifying components...");
    pull_images(provisioner, opts.reporter)
        .await
        .context("pulling Docker images")?;

    // BUG-6 fix: verify image digests on restart path (Req 5.10, 9.4, 9.7)
    verify_image_digests(provisioner, assets, opts.reporter)
        .await
        .context("verifying image digests")?;

    set_ready_marker(provisioner, true).await?;

    opts.reporter.begin_stage("starting services...");
    provisioner
        .exec(&["sudo", "systemctl", "start", "polis"])
        .await
        .context("starting polis service")?;
    opts.reporter.complete_stage();

    wait_ready(provisioner, opts.reporter, false, "workspace ready").await?;

    // Persist state AFTER wait_ready succeeds (Req 5.8)
    let active_agent = persisted.as_ref().and_then(|s| s.active_agent.clone());
    let state = persisted.unwrap_or_else(|| WorkspaceState {
        created_at: Utc::now(),
        image_sha256: None,
        image_source: None,
        active_agent: None,
        provisioning: None,
    });
    state_mgr.save_async(&state).await?;

    Ok(StartOutcome::Restarted { active_agent })
}

// ── WaitThenResolve path ──────────────────────────────────────────────────────

/// Poll VM state until it leaves `Starting`, then re-evaluate once.
async fn wait_then_resolve<P, A, H, R>(
    provisioner: &P,
    state_mgr: &impl WorkspaceStateStore,
    assets: &A,
    hasher: &H,
    opts: StartOptions<'_, R>,
    has_incomplete: bool,
) -> Result<StartOutcome>
where
    P: VmProvisioner,
    A: AssetExtractor,
    H: FileHasher,
    R: ProgressReporter,
{
    let timeout = opts.start_timeout;
    let final_state = wait_for_vm_running(provisioner, timeout).await;

    if final_state == VmState::Starting {
        return Err(WorkspaceError::StartTimeout(timeout.as_secs()).into());
    }

    // Re-evaluate once — final_state is not Starting, so resolve_action
    // cannot return WaitThenResolve again (bounded re-evaluation, Req 5.5, 5.6).
    let new_action = resolve_action(final_state, has_incomplete);

    match new_action {
        StartAction::Create | StartAction::ResumeProvisioning => {
            run_provisioning(provisioner, state_mgr, assets, hasher, &opts).await?;
            Ok(StartOutcome::Created { active_agent: None })
        }
        StartAction::Restart => {
            let persisted = state_mgr.load_async().await?;
            restart_workspace(provisioner, state_mgr, assets, &opts, persisted).await
        }
        StartAction::AlreadyRunning => {
            let persisted = state_mgr.load_async().await?;
            let active_agent = persisted.and_then(|s| s.active_agent);
            Ok(StartOutcome::AlreadyRunning { active_agent })
        }
        // Cannot happen: final_state is not Starting, so resolve_action
        // cannot return WaitThenResolve again.
        StartAction::WaitThenResolve => Err(WorkspaceError::StartTimeout(timeout.as_secs()).into()),
    }
}

/// Poll `vm::state` every 5 s until the VM leaves `Starting`, or timeout.
///
/// Returns the final `VmState`. If the timeout expires while still `Starting`,
/// returns `VmState::Starting` — the caller converts this to `StartTimeout`.
async fn wait_for_vm_running(
    provisioner: &impl crate::application::ports::InstanceInspector,
    timeout: std::time::Duration,
) -> VmState {
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() >= timeout {
            return VmState::Starting; // timeout — caller converts to StartTimeout
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        match vm::state(provisioner).await {
            Ok(s) if s != VmState::Starting => return s,
            _ => {}
        }
    }
}
