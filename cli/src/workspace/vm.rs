//! VM lifecycle operations.

use std::path::Path;

use anyhow::{Context, Result};

use crate::multipass::Multipass;

const VM_CPUS: &str = "2";
const VM_MEMORY: &str = "8G";
const VM_DISK: &str = "40G";

/// Path to `docker-compose.yml` inside the VM.
const COMPOSE_PATH: &str = "/opt/polis/docker-compose.yml";

/// VM state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    NotFound,
    Stopped,
    Starting,
    Running,
}

/// Check if VM exists.
pub fn exists(mp: &impl Multipass) -> bool {
    mp.vm_info().map(|o| o.status.success()).unwrap_or(false)
}

/// Get current VM state.
///
/// # Errors
///
/// Returns an error if the multipass output cannot be parsed.
pub fn state(mp: &impl Multipass) -> Result<VmState> {
    let output = match mp.vm_info() {
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
pub fn is_running(mp: &impl Multipass) -> Result<bool> {
    Ok(state(mp)? == VmState::Running)
}

/// Create VM from image.
///
/// # Errors
///
/// Returns an error if prerequisites are not met, the image path cannot be
/// canonicalized, or the multipass launch fails.
pub fn create(mp: &impl Multipass, image_path: &Path, quiet: bool) -> Result<()> {
    check_prerequisites(mp)?;

    if !quiet {
        println!("✓ {}", inception_line("L0", "sequence started."));
    }

    let image_url = format!("file://{}", image_path.canonicalize()?.display());

    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L1", "workspace isolation starting..."))
    });
    let output = mp
        .launch(&image_url, VM_CPUS, VM_MEMORY, VM_DISK)
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

    configure_credentials(mp);
    start_services_with_progress(mp, quiet);
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
pub fn start(mp: &impl Multipass) -> Result<()> {
    let output = mp.start().context("starting workspace")?;
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
pub fn stop(mp: &impl Multipass) -> Result<()> {
    let _ = mp.exec(&["docker", "compose", "-f", COMPOSE_PATH, "stop"]);
    let output = std::process::Command::new("multipass")
        .args(["stop", "polis"])
        .output()
        .context("stopping workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stop workspace: {stderr}");
    }
    Ok(())
}

/// Delete VM.
pub fn delete(_mp: &impl Multipass) {
    let _ = std::process::Command::new("multipass")
        .args(["delete", "polis"])
        .output();
    let _ = std::process::Command::new("multipass")
        .args(["purge"])
        .output();
}

/// Restart a stopped VM with inception progress messages.
///
/// # Errors
///
/// Returns an error if the multipass start command fails.
pub fn restart(mp: &impl Multipass, quiet: bool) -> Result<()> {
    if !quiet {
        println!("✓ {}", inception_line("L0", "sequence started."));
    }

    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L1", "workspace isolation starting..."))
    });
    start(mp)?;
    if let Some(pb) = pb {
        crate::output::progress::finish_ok(
            &pb,
            &inception_line("L1", "workspace isolation starting..."),
        );
    }

    start_services_with_progress(mp, quiet);
    Ok(())
}

// ── Private helpers ──────────────────────────────────────────────────────────

const MULTIPASS_MIN_VERSION: semver::Version = semver::Version::new(1, 16, 0);

fn check_prerequisites(mp: &impl Multipass) -> Result<()> {
    let output = mp.version().map_err(|_| {
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

fn configure_credentials(mp: &impl Multipass) {
    let ca_cert = std::path::PathBuf::from("certs/ca/ca.pem");
    if ca_cert.exists() {
        let _ = mp.transfer(&ca_cert.to_string_lossy(), "/tmp/ca.pem");
    }
}

fn start_services(mp: &impl Multipass) {
    let _ = mp.exec(&["sudo", "systemctl", "start", "polis"]);
}

fn start_services_with_progress(mp: &impl Multipass, quiet: bool) {
    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L2", "agent isolation starting..."))
    });
    start_services(mp);
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

    struct MockVm(Output);
    impl Multipass for MockVm {
        fn vm_info(&self) -> Result<Output> {
            Ok(Output {
                status: self.0.status,
                stdout: self.0.stdout.clone(),
                stderr: self.0.stderr.clone(),
            })
        }
        fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        fn start(&self) -> Result<Output> {
            unimplemented!()
        }
        fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        fn exec(&self, _: &[&str]) -> Result<Output> {
            unimplemented!()
        }
        fn version(&self) -> Result<Output> {
            unimplemented!()
        }
    }

    #[test]
    fn state_not_found_when_vm_info_fails() {
        let mp = MockVm(fail());
        assert_eq!(state(&mp).expect("state"), VmState::NotFound);
    }

    #[test]
    fn state_running() {
        let mp = MockVm(ok(br#"{"info":{"polis":{"state":"Running"}}}"#));
        assert_eq!(state(&mp).expect("state"), VmState::Running);
    }

    #[test]
    fn state_stopped() {
        let mp = MockVm(ok(br#"{"info":{"polis":{"state":"Stopped"}}}"#));
        assert_eq!(state(&mp).expect("state"), VmState::Stopped);
    }

    #[test]
    fn exists_true_when_vm_info_succeeds() {
        let mp = MockVm(ok(b"{}"));
        assert!(exists(&mp));
    }

    #[test]
    fn exists_false_when_vm_info_fails() {
        let mp = MockVm(fail());
        assert!(!exists(&mp));
    }

    struct MockRestart {
        start_called: std::cell::Cell<bool>,
        exec_called: std::cell::Cell<bool>,
    }
    impl MockRestart {
        fn new() -> Self {
            Self {
                start_called: std::cell::Cell::new(false),
                exec_called: std::cell::Cell::new(false),
            }
        }
    }
    impl Multipass for MockRestart {
        fn vm_info(&self) -> Result<Output> {
            unimplemented!()
        }
        fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        fn start(&self) -> Result<Output> {
            self.start_called.set(true);
            Ok(ok(b""))
        }
        fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        fn exec(&self, _: &[&str]) -> Result<Output> {
            self.exec_called.set(true);
            Ok(ok(b""))
        }
        fn version(&self) -> Result<Output> {
            unimplemented!()
        }
    }

    #[test]
    fn restart_calls_start_and_services() {
        let mp = MockRestart::new();
        let result = restart(&mp, true);
        assert!(result.is_ok());
        assert!(mp.start_called.get(), "start() should be called");
        assert!(
            mp.exec_called.get(),
            "exec() should be called for systemctl"
        );
    }
}
