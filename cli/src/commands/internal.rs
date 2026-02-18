//! Internal commands (`_ssh-proxy`, `_provision`, `_extract-host-key`).
//!
//! These are invoked by tooling (e.g. SSH client via `ProxyCommand`), not by users.

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Backend detection
// ---------------------------------------------------------------------------

/// The backend used to reach the workspace.
pub enum Backend {
    Multipass,
    Docker,
}

/// Abstraction over backend availability checks, enabling unit-test injection.
#[allow(async_fn_in_trait)]
pub trait BackendProber {
    /// Returns `true` if a Multipass workspace named `polis` is running.
    async fn multipass_exists(&self) -> bool;
}

/// Detects which backend is available.
///
/// # Errors
///
/// Currently infallible; returns `Result` for forward compatibility.
pub async fn detect_backend<P: BackendProber>(prober: &P) -> Result<Backend> {
    if prober.multipass_exists().await {
        Ok(Backend::Multipass)
    } else {
        Ok(Backend::Docker)
    }
}

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
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Proxy implementations
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

async fn proxy_via_multipass() -> Result<()> {
    let mut child = tokio::process::Command::new("multipass")
        .args(["exec", "polis", "--", "nc", "localhost", "22"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("failed to spawn multipass")?;
    Box::pin(bridge_stdio(&mut child)).await
}

async fn proxy_via_docker() -> Result<()> {
    let mut child = tokio::process::Command::new("docker")
        .args(["exec", "-i", "polis-workspace-1", "nc", "localhost", "22"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("failed to spawn docker")?;
    Box::pin(bridge_stdio(&mut child)).await
}

// ---------------------------------------------------------------------------
// Real prober
// ---------------------------------------------------------------------------

struct SystemProber;

impl BackendProber for SystemProber {
    async fn multipass_exists(&self) -> bool {
        tokio::process::Command::new("multipass")
            .args(["info", "polis", "--format", "json"])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// SSH `ProxyCommand` helper — bridges SSH client STDIO to workspace sshd.
///
/// Invoked by the SSH client via `ProxyCommand polis _ssh-proxy`.
///
/// # Errors
///
/// Returns an error if the backend cannot be spawned or STDIO bridging fails.
pub async fn ssh_proxy() -> Result<()> {
    let backend = detect_backend(&SystemProber).await?;
    match backend {
        Backend::Multipass => Box::pin(proxy_via_multipass()).await,
        Backend::Docker => Box::pin(proxy_via_docker()).await,
    }
}

// ---------------------------------------------------------------------------
// Host key extraction
// ---------------------------------------------------------------------------

async fn extract_key_from(cmd: &str, args: &[&str]) -> Result<String> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to run {cmd}"))?;
    anyhow::ensure!(output.status.success(), "{cmd} exec failed");
    let key = String::from_utf8(output.stdout)
        .context("host key output is not valid UTF-8")?
        .trim()
        .to_string();
    crate::ssh::validate_host_key(&key)?;
    Ok(key)
}

async fn extract_from_multipass() -> Result<String> {
    extract_key_from(
        "multipass",
        &["exec", "polis", "--", "cat", "/etc/ssh/ssh_host_ed25519_key.pub"],
    )
    .await
}

async fn extract_from_docker() -> Result<String> {
    extract_key_from(
        "docker",
        &["exec", "polis-workspace-1", "cat", "/etc/ssh/ssh_host_ed25519_key.pub"],
    )
    .await
}

/// Extracts the workspace SSH host key and prints it in `known_hosts` format.
///
/// Output: `workspace ssh-ed25519 <key-material>`
///
/// Invoked during provisioning via `polis _extract-host-key`.
///
/// # Errors
///
/// Returns an error if no backend is reachable or the host key cannot be extracted.
pub async fn extract_host_key() -> Result<()> {
    let backend = detect_backend(&SystemProber).await?;
    let key = match backend {
        Backend::Multipass => extract_from_multipass().await,
        Backend::Docker => extract_from_docker().await,
    }
    .context("failed to extract host key")?;
    println!("workspace {key}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests (pre-existing RED phase — do not modify)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptests {
    use super::{bridge_io, detect_backend, Backend, BackendProber};
    use proptest::prelude::*;

    struct FixedProber(bool);
    impl BackendProber for FixedProber {
        async fn multipass_exists(&self) -> bool {
            self.0
        }
    }

    proptest! {
        /// detect_backend returns Multipass iff multipass_exists() is true.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_detect_backend_follows_prober(available in proptest::bool::ANY) {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let backend = rt.block_on(detect_backend(&FixedProber(available))).expect("infallible");
            if available {
                prop_assert!(matches!(backend, Backend::Multipass));
            } else {
                prop_assert!(matches!(backend, Backend::Docker));
            }
        }

        /// bridge_io forwards every byte from reader to writer exactly.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_bridge_io_preserves_all_bytes(input in prop::collection::vec(any::<u8>(), 0..16384)) {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let mut writer = Vec::new();
            rt.block_on(bridge_io(&mut input.as_slice(), &mut writer)).expect("bridge ok");
            prop_assert_eq!(writer, input);
        }

        /// bridge_io output length always equals input length.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_bridge_io_output_length_equals_input_length(
            input in prop::collection::vec(any::<u8>(), 0..16384)
        ) {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let expected_len = input.len();
            let mut writer = Vec::new();
            rt.block_on(bridge_io(&mut input.as_slice(), &mut writer)).expect("bridge ok");
            prop_assert_eq!(writer.len(), expected_len);
        }

        /// bridge_io always returns Ok for any in-memory input.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_bridge_io_never_errors_on_memory_io(
            input in prop::collection::vec(any::<u8>(), 0..16384)
        ) {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let mut writer = Vec::new();
            let result = rt.block_on(bridge_io(&mut input.as_slice(), &mut writer));
            prop_assert!(result.is_ok());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_backend, Backend, BackendProber};

    struct AlwaysMultipass;
    impl BackendProber for AlwaysMultipass {
        async fn multipass_exists(&self) -> bool {
            true
        }
    }

    struct NeverMultipass;
    impl BackendProber for NeverMultipass {
        async fn multipass_exists(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn test_detect_backend_returns_multipass_when_available() {
        let backend = detect_backend(&AlwaysMultipass).await.expect("should detect backend");
        assert!(matches!(backend, Backend::Multipass));
    }

    #[tokio::test]
    async fn test_detect_backend_returns_docker_when_multipass_unavailable() {
        let backend = detect_backend(&NeverMultipass).await.expect("should detect backend");
        assert!(matches!(backend, Backend::Docker));
    }

    use super::bridge_io;

    #[tokio::test]
    async fn test_bridge_io_forwards_bytes_from_reader_to_writer() {
        let input = b"SSH-2.0-OpenSSH_8.9\r\n";
        let mut writer = Vec::new();
        bridge_io(&mut input.as_ref(), &mut writer).await.expect("bridge should succeed");
        assert_eq!(writer, input);
    }

    #[tokio::test]
    async fn test_bridge_io_terminates_when_reader_closes() {
        // Empty reader → EOF immediately → bridge returns Ok(())
        let mut writer = tokio::io::sink();
        let result = bridge_io(&mut tokio::io::empty(), &mut writer).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_bridge_io_flushes_partial_writes() {
        // Verifies that a non-empty payload is fully flushed to the writer.
        let input = b"hello";
        let mut buf = tokio::io::BufWriter::new(Vec::new());
        bridge_io(&mut input.as_ref(), &mut buf).await.expect("bridge should succeed");
        assert_eq!(buf.buffer(), b"hello");
    }
}
