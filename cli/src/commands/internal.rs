//! Internal commands (`_ssh-proxy`, `_extract-host-key`).
//!
//! These are invoked by tooling (e.g. SSH client via `ProxyCommand`), not by users.

use anyhow::{Context, Result};
use std::process::ExitCode;

use crate::domain::process::exit_code_from_status;
use crate::domain::workspace::CONTAINER_NAME;
use crate::infra::ssh::SshTransport;

// ---------------------------------------------------------------------------
// Proxy implementation — Stdio::inherit
//
// Spawns `ssh ubuntu@vm docker exec -i <container> sshd -i` with inherited
// stdin/stdout/stderr. The SSH client's pipe handles pass directly to the
// child ssh process — no Rust-side bridging, no pipe forwarding issues.
// ---------------------------------------------------------------------------

/// SSH `ProxyCommand` helper — bridges SSH client STDIO to workspace sshd.
///
/// Invoked by the SSH client via `ProxyCommand polis _ssh-proxy`.
///
/// Spawns `ssh ubuntu@<vm-ip> docker exec -i <container> /usr/sbin/sshd -i`
/// with inherited stdin/stdout/stderr. The SSH client's pipe handles pass
/// directly to the child `ssh` process with zero Rust-side bridging.
///
/// # Errors
///
/// Returns an error if the VM IP cannot be resolved or SSH cannot be spawned.
pub async fn ssh_proxy(mp: &impl crate::application::ports::InstanceInspector) -> Result<ExitCode> {
    let vm_ip = crate::application::services::vm::lifecycle::resolve_vm_ip(mp).await?;

    let docker_cmd = format!("docker exec -i {CONTAINER_NAME} /usr/sbin/sshd -i");

    let transport = SshTransport::new()?;
    let status = transport.spawn_inherited(&vm_ip, &docker_cmd).await?;

    Ok(exit_code_from_status(status))
}

// ---------------------------------------------------------------------------
// Host key extraction
// ---------------------------------------------------------------------------

/// Extracts the workspace SSH host key and prints it in `known_hosts` format.
///
/// Output: `workspace ssh-ed25519 <key-material>`
///
/// Invoked during provisioning via `polis _extract-host-key`.
///
/// # Errors
///
/// Returns an error if the host key cannot be extracted.
// Allow large future: This function is called infrequently (once during provisioning),
// so the trade-off favors avoiding heap allocation overhead over minimizing stack size.
#[allow(clippy::large_futures)]
pub async fn extract_host_key(
    mp: &impl crate::application::ports::ShellExecutor,
) -> Result<ExitCode> {
    let output = mp
        .exec(&[
            "docker",
            "exec",
            CONTAINER_NAME,
            "cat",
            "/etc/ssh/ssh_host_ed25519_key.pub",
        ])
        .await
        .context("failed to run multipass")?;
    anyhow::ensure!(output.status.success(), "multipass exec failed");
    let key = String::from_utf8(output.stdout)
        .context("host key output is not valid UTF-8")?
        .trim()
        .to_string();
    crate::domain::ssh::validate_host_key(&key)?;
    // Output format is a protocol consumed by SSH tooling (ProxyCommand, known_hosts),
    // not user-facing output — intentionally bypasses OutputContext/Renderer.
    println!("workspace {key}");
    Ok(ExitCode::SUCCESS)
}
