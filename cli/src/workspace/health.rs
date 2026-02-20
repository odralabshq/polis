//! Health checks and readiness waiting.

use std::time::Duration;

use anyhow::Result;

use crate::multipass::Multipass;

/// Path to `docker-compose.yml` inside the VM.
const COMPOSE_PATH: &str = "/opt/polis/docker-compose.yml";

/// Health status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Unhealthy { reason: String },
    Unknown,
}

/// Wait for workspace to become healthy.
///
/// Polls every 2 seconds for up to 60 seconds.
///
/// # Errors
///
/// Returns an error if the workspace does not become healthy within timeout.
pub fn wait_ready(mp: &impl Multipass, quiet: bool) -> Result<()> {
    if !quiet {
        println!("Activating security controls...");
    }

    let max_attempts = 30;
    let delay = Duration::from_secs(2);

    for attempt in 1..=max_attempts {
        match check(mp)? {
            HealthStatus::Healthy => return Ok(()),
            HealthStatus::Unhealthy { reason } if attempt == max_attempts => {
                anyhow::bail!("Workspace did not start properly.\n\nReason: {reason}\nDiagnose: polis doctor\nView logs: polis logs");
            }
            _ => std::thread::sleep(delay),
        }
    }

    anyhow::bail!("Workspace did not start properly.\n\nDiagnose: polis doctor\nView logs: polis logs")
}

/// Check current health status.
pub fn check(mp: &impl Multipass) -> Result<HealthStatus> {
    let output = match mp.exec(&[
        "docker", "compose", "-f", COMPOSE_PATH, "ps", "--format", "json", "workspace",
    ]) {
        Ok(o) => o,
        Err(_) => return Ok(HealthStatus::Unknown),
    };

    if !output.status.success() {
        return Ok(HealthStatus::Unknown);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(line) = stdout.lines().next() else {
        return Ok(HealthStatus::Unknown);
    };

    let container: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Ok(HealthStatus::Unknown),
    };

    let state = container.get("State").and_then(|s| s.as_str()).unwrap_or("");
    let health = container.get("Health").and_then(|s| s.as_str()).unwrap_or("");

    if state == "running" && health == "healthy" {
        Ok(HealthStatus::Healthy)
    } else if state == "running" {
        Ok(HealthStatus::Unhealthy { reason: format!("health: {health}") })
    } else {
        Ok(HealthStatus::Unhealthy { reason: format!("state: {state}") })
    }
}
