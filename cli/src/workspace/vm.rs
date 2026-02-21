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
pub fn exists(mp: &impl Multipass) -> Result<bool> {
    Ok(mp.vm_info().map(|o| o.status.success()).unwrap_or(false))
}

/// Get current VM state.
pub fn state(mp: &impl Multipass) -> Result<VmState> {
    let output = match mp.vm_info() {
        Ok(o) if o.status.success() => o,
        _ => return Ok(VmState::NotFound),
    };
    let info: serde_json::Value = serde_json::from_slice(&output.stdout).context("parsing multipass info")?;
    let state_str = info.get("info").and_then(|i| i.get("polis")).and_then(|p| p.get("state")).and_then(|s| s.as_str()).unwrap_or("Unknown");
    Ok(match state_str {
        "Running" => VmState::Running,
        "Starting" => VmState::Starting,
        _ => VmState::Stopped,
    })
}

/// Check if VM is running.
#[allow(dead_code)] // API for future use
pub fn is_running(mp: &impl Multipass) -> Result<bool> {
    Ok(state(mp)? == VmState::Running)
}

/// Create VM from image.
///
/// # Errors
///
/// Returns an error if prerequisites are not met or launch fails.
pub fn create(mp: &impl Multipass, image_path: &Path, quiet: bool) -> Result<()> {
    check_prerequisites(mp)?;

    if !quiet {
        println!("✓ {}", inception_line("L0", "sequence started."));
    }

    let image_url = format!("file://{}", image_path.canonicalize()?.display());

    let pb = (!quiet).then(|| crate::output::progress::spinner(&inception_line("L1", "workspace isolation starting...")));
    let output = mp.launch(&image_url, VM_CPUS, VM_MEMORY, VM_DISK).context("launching workspace")?;
    if let Some(pb) = pb {
        if output.status.success() {
            crate::output::progress::finish_ok(&pb, &inception_line("L1", "workspace isolation starting..."));
        } else {
            pb.finish_and_clear();
        }
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        #[cfg(target_os = "linux")]
        if stderr.contains("Failed to copy") {
            anyhow::bail!("Failed to create workspace.\n\nFix: sudo snap connect multipass:removable-media");
        }
        anyhow::bail!("Failed to create workspace.\n\nRun 'polis doctor' to diagnose.");
    }

    configure_credentials(mp)?;

    let pb = (!quiet).then(|| crate::output::progress::spinner(&inception_line("L2", "agent isolation starting...")));
    start_services(mp)?;
    if let Some(pb) = pb {
        crate::output::progress::finish_ok(&pb, &inception_line("L2", "agent isolation starting..."));
    }

    pin_host_key();
    Ok(())
}

fn inception_line(level: &str, msg: &str) -> String {
    use owo_colors::{OwoColorize, Style, Stream::Stdout};
    let tag_style = match level {
        "L0" => Style::new().truecolor(107, 33, 168),  // stop 1
        "L1" => Style::new().truecolor(93, 37, 163),   // stop 2
        "L2" => Style::new().truecolor(64, 47, 153),   // stop 3
        _    => Style::new().truecolor(46, 53, 147),   // stop 4
    };
    format!(
        "{}  {}",
        "[inception]".if_supports_color(Stdout, |t| t.style(tag_style)),
        msg
    )
}

/// Start existing VM.
pub fn start(mp: &impl Multipass) -> Result<()> {
    let output = mp.start().context("starting workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to start workspace: {stderr}");
    }
    Ok(())
}

/// Stop VM.
pub fn stop(mp: &impl Multipass) -> Result<()> {
    let _ = mp.exec(&["docker", "compose", "-f", COMPOSE_PATH, "stop"]);
    let output = std::process::Command::new("multipass").args(["stop", "polis"]).output().context("stopping workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stop workspace: {stderr}");
    }
    Ok(())
}

/// Delete VM.
pub fn delete(_mp: &impl Multipass) -> Result<()> {
    let _ = std::process::Command::new("multipass").args(["delete", "polis"]).output();
    let _ = std::process::Command::new("multipass").args(["purge"]).output();
    Ok(())
}

/// Ensure VM is running (create if needed, start if stopped).
pub fn ensure_running(mp: &impl Multipass, image_path: &Path, quiet: bool) -> Result<()> {
    match state(mp)? {
        VmState::Running => Ok(()),
        VmState::Stopped | VmState::Starting => {
            if !quiet {
                println!("Starting workspace...");
            }
            start(mp)
        }
        VmState::NotFound => create(mp, image_path, quiet),
    }
}

// ── Private helpers ──────────────────────────────────────────────────────────

const MULTIPASS_MIN_VERSION: semver::Version = semver::Version::new(1, 16, 0);

fn check_prerequisites(mp: &impl Multipass) -> Result<()> {
    let output = mp.version().map_err(|_| anyhow::anyhow!("Workspace runtime not available.\n\nRun 'polis doctor' to diagnose and fix."))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(ver_str) = stdout.lines().next().and_then(|l| l.split_whitespace().nth(1)) {
        if let Ok(v) = semver::Version::parse(ver_str) {
            if v < MULTIPASS_MIN_VERSION {
                anyhow::bail!("Workspace runtime needs update.\n\nRun 'polis doctor' to diagnose and fix.");
            }
        }
    }
    Ok(())
}

fn configure_credentials(mp: &impl Multipass) -> Result<()> {
    let ca_cert = std::path::PathBuf::from("certs/ca/ca.pem");
    if ca_cert.exists() {
        let _ = mp.transfer(&ca_cert.to_string_lossy(), "/tmp/ca.pem");
    }
    Ok(())
}

fn start_services(mp: &impl Multipass) -> Result<()> {
    let _ = mp.exec(&["sudo", "systemctl", "start", "polis"]);
    Ok(())
}

fn pin_host_key() {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("polis"));
    if let Ok(output) = std::process::Command::new(exe).args(["_extract-host-key"]).output() {
        if output.status.success() {
            if let Ok(host_key) = String::from_utf8(output.stdout) {
                let _ = crate::ssh::KnownHostsManager::new().and_then(|m| m.update(host_key.trim()));
            }
        }
    }
}
