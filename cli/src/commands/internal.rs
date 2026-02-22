//! Internal commands (`_ssh-proxy`, `_extract-host-key`).
//!
//! These are invoked by tooling (e.g. SSH client via `ProxyCommand`), not by users.

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::workspace::CONTAINER_NAME;

// ---------------------------------------------------------------------------
// STDIO bridge
// ---------------------------------------------------------------------------

/// Copies bytes from `reader` to `writer` until EOF.
///
/// # Errors
///
/// Returns an error if reading or writing fails.
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
// Proxy implementation
// ---------------------------------------------------------------------------

async fn bridge_stdio(child: &mut tokio::process::Child) -> Result<()> {
    let mut child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("child stdin unavailable"))?;
    let mut child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("child stdout unavailable"))?;

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    tokio::select! {
        r = bridge_io(&mut stdin, &mut child_stdin) => r?,
        r = bridge_io(&mut child_stdout, &mut stdout) => r?,
    }

    let _ = child.start_kill();
    child.wait().await?;
    Ok(())
}

/// SSH `ProxyCommand` helper — bridges SSH client STDIO to workspace sshd.
///
/// Invoked by the SSH client via `ProxyCommand polis _ssh-proxy`.
///
/// # Errors
///
/// Returns an error if multipass cannot be spawned or STDIO bridging fails.
#[allow(clippy::large_futures)]
pub async fn ssh_proxy(mp: &impl crate::multipass::Multipass) -> Result<()> {
    let mut child = mp.exec_spawn(&[
        "docker",
        "exec",
        "-i",
        CONTAINER_NAME,
        "/usr/sbin/sshd",
        "-i",
    ]).context("failed to spawn multipass")?;
    bridge_stdio(&mut child).await
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
pub async fn extract_host_key(mp: &impl crate::multipass::Multipass) -> Result<()> {
    let output = mp.exec(&[
        "docker",
        "exec",
        CONTAINER_NAME,
        "cat",
        "/etc/ssh/ssh_host_ed25519_key.pub",
    ]).await.context("failed to run multipass")?;
    anyhow::ensure!(output.status.success(), "multipass exec failed");
    let key = String::from_utf8(output.stdout)
        .context("host key output is not valid UTF-8")?
        .trim()
        .to_string();
    crate::ssh::validate_host_key(&key)?;
    println!("workspace {key}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::bridge_io;

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
}
