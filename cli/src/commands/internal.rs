//! Internal commands (`_ssh-proxy`, `_extract-host-key`).
//!
//! These are invoked by tooling (e.g. SSH client via `ProxyCommand`), not by users.

use anyhow::{Context, Result};
use std::process::ExitCode;

use crate::domain::workspace::CONTAINER_NAME;

// ---------------------------------------------------------------------------
// STDIO bridge (async — used by tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Copies bytes from `reader` to `writer` until EOF.
///
/// # Errors
///
/// Returns an error if reading or writing fails.
#[cfg(test)]
pub async fn bridge_io<R, W>(reader: &mut R, writer: &mut W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await?;
        writer.flush().await?;
    }
    Ok(())
}

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

    let identity_key = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".polis")
        .join("id_ed25519");

    #[cfg(windows)]
    let devnull = "NUL";
    #[cfg(not(windows))]
    let devnull = "/dev/null";

    let docker_cmd = format!("docker exec -i {CONTAINER_NAME} /usr/sbin/sshd -i");

    // Inherit stdin/stdout/stderr directly — no Rust-side piping.
    // The SSH client's pipe handles pass straight through to the child
    // ssh process, avoiding any Windows pipe forwarding issues in Rust.
    let status = std::process::Command::new("ssh")
        .args([
            "-i",
            &identity_key.to_string_lossy(),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            &format!("UserKnownHostsFile={devnull}"),
            "-o",
            "LogLevel=ERROR",
            "-o",
            "BatchMode=yes",
            &format!("ubuntu@{vm_ip}"),
            &docker_cmd,
        ])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("failed to spawn ssh")?;

    let code = status.code().unwrap_or(255);
    #[allow(clippy::cast_possible_truncation)]
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(255)))
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
    println!("workspace {key}");
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::bridge_io;
    use proptest::prelude::*;

    #[tokio::test]
    async fn test_bridge_io_forwards_bytes_from_reader_to_writer() {
        let input = b"SSH-2.0-OpenSSH_8.9\r\n";
        let mut writer = Vec::new();
        bridge_io(&mut input.as_ref(), &mut writer)
            .await
            .expect("bridge should succeed");
        assert_eq!(writer, input);
    }

    #[tokio::test]
    async fn test_bridge_io_terminates_when_reader_closes() {
        let mut writer = tokio::io::sink();
        let result = bridge_io(&mut tokio::io::empty(), &mut writer).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_bridge_io_flushes_partial_writes() {
        let input = b"hello";
        let mut buf = tokio::io::BufWriter::new(Vec::new());
        bridge_io(&mut input.as_ref(), &mut buf)
            .await
            .expect("bridge should succeed");
        assert_eq!(buf.get_ref(), b"hello");
    }

    proptest! {
        #[test]
        fn prop_bridge_io_forwards_all_bytes(bs in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            rt.block_on(async {
                let mut writer: Vec<u8> = Vec::new();
                bridge_io(&mut bs.as_slice(), &mut writer)
                    .await
                    .expect("bridge_io should complete without stalling");
                prop_assert_eq!(writer, bs);
                Ok(())
            })?;
        }
    }
}

#[cfg(test)]
mod preservation_tests {
    use super::bridge_io;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_bridge_byte_fidelity_preservation(bs in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            rt.block_on(async {
                let mut writer: Vec<u8> = Vec::new();
                bridge_io(&mut bs.as_slice(), &mut writer)
                    .await
                    .expect("bridge_io should complete without error");
                prop_assert_eq!(&writer, &bs, "bridge must preserve every byte exactly");
                Ok(())
            })?;
        }
    }
}
