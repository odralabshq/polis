//! VM lifecycle operations.

use std::path::Path;

use anyhow::{Context, Result};

use crate::multipass::Multipass;
use crate::workspace::COMPOSE_PATH;

const VM_CPUS: &str = "2";
const VM_MEMORY: &str = "8G";
const VM_DISK: &str = "40G";

/// VM state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    NotFound,
    Stopped,
    Starting,
    Running,
}

/// Check if VM exists.
pub async fn exists(mp: &impl Multipass) -> bool {
    mp.vm_info()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get current VM state.
///
/// # Errors
///
/// Returns an error if the multipass output cannot be parsed.
pub async fn state(mp: &impl Multipass) -> Result<VmState> {
    let output = match mp.vm_info().await {
        Ok(o) if o.status.success() => o,
        _ => return Ok(VmState::NotFound),
    };
    let info: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parsing multipass info")?;
    let state_str = info
        .get("info")
        .and_then(|i| i.get("polis"))
        .and_then(|p| p.get("state"))
        .and_then(|s| s.as_str())
        .unwrap_or("Unknown");
    Ok(match state_str {
        "Running" => VmState::Running,
        "Starting" => VmState::Starting,
        _ => VmState::Stopped,
    })
}

/// Check if VM is running.
///
/// # Errors
///
/// Returns an error if the VM state cannot be determined.
#[allow(dead_code)] // API for future use
pub async fn is_running(mp: &impl Multipass) -> Result<bool> {
    Ok(state(mp).await? == VmState::Running)
}

/// Create VM from image.
///
/// # Errors
///
/// Returns an error if prerequisites are not met, the image path cannot be
/// canonicalized, or the multipass launch fails.
pub async fn create(mp: &impl Multipass, image_path: &Path, quiet: bool) -> Result<()> {
    check_prerequisites(mp).await?;

    if !quiet {
        println!("✓ {}", inception_line("L0", "sequence started."));
    }

    let image_url = format!("file://{}", image_path.canonicalize()?.display());

    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L1", "workspace isolation starting..."))
    });
    let output = mp
        .launch(&image_url, VM_CPUS, VM_MEMORY, VM_DISK)
        .await
        .context("launching workspace")?;
    if let Some(pb) = pb {
        if output.status.success() {
            crate::output::progress::finish_ok(
                &pb,
                &inception_line("L1", "workspace isolation starting..."),
            );
        } else {
            pb.finish_and_clear();
        }
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        #[cfg(target_os = "linux")]
        if stderr.contains("Failed to copy") {
            anyhow::bail!(
                "Failed to create workspace.\n\nFix: sudo snap connect multipass:removable-media"
            );
        }
        anyhow::bail!("Failed to create workspace.\n\nRun 'polis doctor' to diagnose.");
    }

    configure_credentials(mp).await;
    start_services_with_progress(mp, quiet).await;
    pin_host_key();
    Ok(())
}

fn inception_line(level: &str, msg: &str) -> String {
    use owo_colors::{OwoColorize, Stream::Stdout, Style};
    let tag_style = match level {
        "L0" => Style::new().truecolor(107, 33, 168), // stop 1
        "L1" => Style::new().truecolor(93, 37, 163),  // stop 2
        "L2" => Style::new().truecolor(64, 47, 153),  // stop 3
        _ => Style::new().truecolor(46, 53, 147),     // stop 4
    };
    format!(
        "{}  {}",
        "[inception]".if_supports_color(Stdout, |t| t.style(tag_style)),
        msg
    )
}

/// Start existing VM.
///
/// # Errors
///
/// Returns an error if the multipass start command fails.
pub async fn start(mp: &impl Multipass) -> Result<()> {
    let output = mp.start().await.context("starting workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to start workspace: {stderr}");
    }
    Ok(())
}

/// Stop VM.
///
/// # Errors
///
/// Returns an error if the multipass stop command fails.
pub async fn stop(mp: &impl Multipass) -> Result<()> {
    let _ = mp
        .exec(&["docker", "compose", "-f", COMPOSE_PATH, "stop"])
        .await;
    let output = mp.stop().await.context("stopping workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stop workspace: {stderr}");
    }
    Ok(())
}

/// Delete VM.
pub async fn delete(mp: &impl Multipass) {
    let _ = mp.delete().await;
    let _ = mp.purge().await;
}

/// Restart a stopped VM with inception progress messages.
///
/// # Errors
///
/// Returns an error if the multipass start command fails.
pub async fn restart(mp: &impl Multipass, quiet: bool) -> Result<()> {
    if !quiet {
        println!("✓ {}", inception_line("L0", "sequence started."));
    }

    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L1", "workspace isolation starting..."))
    });
    start(mp).await?;
    if let Some(pb) = pb {
        crate::output::progress::finish_ok(
            &pb,
            &inception_line("L1", "workspace isolation starting..."),
        );
    }

    start_services_with_progress(mp, quiet).await;
    Ok(())
}

// ── Private helpers ──────────────────────────────────────────────────────────

const MULTIPASS_MIN_VERSION: semver::Version = semver::Version::new(1, 16, 0);

async fn check_prerequisites(mp: &impl Multipass) -> Result<()> {
    let output = mp.version().await.map_err(|_| {
        anyhow::anyhow!(
            "Workspace runtime not available.\n\nRun 'polis doctor' to diagnose and fix."
        )
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(ver_str) = stdout
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        && let Ok(v) = semver::Version::parse(ver_str)
        && v < MULTIPASS_MIN_VERSION
    {
        anyhow::bail!("Workspace runtime needs update.\n\nRun 'polis doctor' to diagnose and fix.");
    }
    Ok(())
}

async fn configure_credentials(mp: &impl Multipass) {
    let ca_cert = std::path::PathBuf::from("certs/ca/ca.pem");
    if ca_cert.exists() {
        let _ = mp.transfer(&ca_cert.to_string_lossy(), "/tmp/ca.pem").await;
    }
}

async fn start_services(mp: &impl Multipass) {
    let _ = mp.exec(&["sudo", "systemctl", "start", "polis"]).await;
}

async fn start_services_with_progress(mp: &impl Multipass, quiet: bool) {
    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L2", "agent isolation starting..."))
    });
    start_services(mp).await;
    if let Some(pb) = pb {
        crate::output::progress::finish_ok(
            &pb,
            &inception_line("L2", "agent isolation starting..."),
        );
    }
}

fn pin_host_key() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("polis"));
    if let Ok(output) = std::process::Command::new(exe)
        .args(["_extract-host-key"])
        .output()
        && output.status.success()
        && let Ok(host_key) = String::from_utf8(output.stdout)
    {
        let _ = crate::ssh::KnownHostsManager::new().and_then(|m| m.update(host_key.trim()));
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    use anyhow::Result;

    use super::*;
    use crate::multipass::Multipass;

    fn ok(stdout: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }
    fn fail() -> Output {
        Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    /// Mock multipass with configurable `vm_info()` output for state detection.
    struct MultipassVmInfoStub(Output);
    impl Multipass for MultipassVmInfoStub {
        async fn vm_info(&self) -> Result<Output> {
            Ok(Output {
                status: self.0.status,
                stdout: self.0.stdout.clone(),
                stderr: self.0.stderr.clone(),
            })
        }
        async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn start(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn stop(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn delete(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn purge(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            unimplemented!()
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            unimplemented!()
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            unimplemented!()
        }
        async fn version(&self) -> Result<Output> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn state_not_found_when_vm_info_fails() {
        let mp = MultipassVmInfoStub(fail());
        assert_eq!(state(&mp).await.expect("state"), VmState::NotFound);
    }

    #[tokio::test]
    async fn state_running() {
        let mp = MultipassVmInfoStub(ok(br#"{"info":{"polis":{"state":"Running"}}}"#));
        assert_eq!(state(&mp).await.expect("state"), VmState::Running);
    }

    #[tokio::test]
    async fn state_stopped() {
        let mp = MultipassVmInfoStub(ok(br#"{"info":{"polis":{"state":"Stopped"}}}"#));
        assert_eq!(state(&mp).await.expect("state"), VmState::Stopped);
    }

    #[tokio::test]
    async fn exists_true_when_vm_info_succeeds() {
        let mp = MultipassVmInfoStub(ok(b"{}"));
        assert!(exists(&mp).await);
    }

    #[tokio::test]
    async fn exists_false_when_vm_info_fails() {
        let mp = MultipassVmInfoStub(fail());
        assert!(!exists(&mp).await);
    }

    /// Mock multipass that tracks `start()` and `exec()` calls for restart tests.
    struct MultipassRestartSpy {
        start_called: std::cell::Cell<bool>,
        exec_called: std::cell::Cell<bool>,
    }
    impl MultipassRestartSpy {
        fn new() -> Self {
            Self {
                start_called: std::cell::Cell::new(false),
                exec_called: std::cell::Cell::new(false),
            }
        }
    }
    impl Multipass for MultipassRestartSpy {
        async fn vm_info(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn start(&self) -> Result<Output> {
            self.start_called.set(true);
            Ok(ok(b""))
        }
        async fn stop(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn delete(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn purge(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            self.exec_called.set(true);
            Ok(ok(b""))
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            unimplemented!()
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            unimplemented!()
        }
        async fn version(&self) -> Result<Output> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn restart_calls_start_and_services() {
        let mp = MultipassRestartSpy::new();
        let result = restart(&mp, true).await;
        assert!(result.is_ok());
        assert!(mp.start_called.get(), "start() should be called");
        assert!(
            mp.exec_called.get(),
            "exec() should be called for systemctl"
        );
    }
}
