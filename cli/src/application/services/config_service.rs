//! Application service — configuration use-cases.

use crate::application::ports::ConfigStore;
use crate::domain::config::PolisConfig;
use anyhow::Result;

/// Load configuration.
pub fn load_config(store: &impl ConfigStore) -> Result<PolisConfig> {
    store.load()
}

/// Save configuration.
pub fn save_config(store: &impl ConfigStore, config: &PolisConfig) -> Result<()> {
    store.save(config)
}

const VM_MCP_ADMIN_PASS: &str = "/opt/polis/secrets/mcp-admin-pass.txt";

/// Propagate the security level to the workspace VM.
/// Returns Ok(true) if successful, Ok(false) if VM is not running or unreachable.
pub async fn propagate_security_level(
    mp: &(impl crate::application::ports::ShellExecutor + crate::application::ports::InstanceInspector),
    level: &str,
) -> Result<bool> {
    use crate::application::services::vm;
    if vm::lifecycle::state(mp).await.ok() != Some(vm::lifecycle::VmState::Running) {
        return Ok(false);
    }
    let pass = match mp.exec(&["cat", VM_MCP_ADMIN_PASS]).await {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => return Ok(false),
    };

    let env_arg = format!("REDISCLI_AUTH={pass}");
    match mp
        .exec(&[
            "docker",
            "exec",
            "-e",
            &env_arg,
            "polis-state",
            "valkey-cli",
            "--tls",
            "--cert",
            "/etc/valkey/tls/client.crt",
            "--key",
            "/etc/valkey/tls/client.key",
            "--cacert",
            "/etc/valkey/tls/ca.crt",
            "--user",
            "mcp-admin",
            "SET",
            "polis:config:security_level",
            level,
        ])
        .await
    {
        Ok(output) if output.status.success() => Ok(true),
        _ => Ok(false),
    }
}
