//! `polis agent` — agent management subcommands.
#![allow(clippy::format_push_string)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::LazyLock;

use crate::output::OutputContext;
use crate::provisioner::{FileTransfer, InstanceInspector, ShellExecutor};
use crate::state::StateManager;
use crate::workspace::{CONTAINER_NAME, vm};

const VM_ROOT: &str = "/opt/polis";

/// Same rule enforced by `generate-agent.sh`; checked here before any
/// path interpolation to prevent path-traversal (CWE-22).
static AGENT_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Safety: this is a compile-time constant pattern — cannot fail.
    #[allow(clippy::expect_used)]
    Regex::new(r"^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$").expect("valid regex")
});

/// Shell metacharacters that must not appear in runtime.command.
static SHELL_METACHAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"[;|&`$()\\<>!#~*\[\]{}]").expect("valid regex")
});

/// Platform-reserved ports that agents must not use.
const PLATFORM_PORTS: &[u16] = &[
    80, 443, 8080, 8443, 9090, 9091, 9092, 9093, 3000, 5432, 6379, 27017,
];

/// Allowed prefixes for readWritePaths (same as generate-agent.sh).
const ALLOWED_RW_PREFIXES: &[&str] = &["/opt/polis/agents/", "/tmp/", "/var/tmp/"];

// ---------------------------------------------------------------------------
// Full AgentManifest struct (typed representation of agent.yaml)
// ---------------------------------------------------------------------------

/// Full typed representation of an `agent.yaml` manifest.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FullAgentManifest {
    api_version: String,
    kind: String,
    metadata: FullAgentManifestMetadata,
    spec: AgentSpec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FullAgentManifestMetadata {
    name: String,
    display_name: Option<String>,
    #[allow(dead_code)]
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSpec {
    packaging: String,
    runtime: RuntimeSpec,
    health: Option<HealthSpec>,
    security: Option<SecuritySpec>,
    ports: Option<Vec<PortSpec>>,
    resources: Option<ResourceSpec>,
    persistence: Option<Vec<PersistenceSpec>>,
    requirements: Option<RequirementsSpec>,
    install: Option<String>,
    init: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSpec {
    command: String,
    workdir: Option<String>,
    user: String,
    env_file: Option<String>,
    env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthSpec {
    command: Option<String>,
    interval: Option<String>,
    timeout: Option<String>,
    retries: Option<u32>,
    start_period: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecuritySpec {
    protect_system: Option<String>,
    protect_home: Option<String>,
    read_write_paths: Option<Vec<String>>,
    #[allow(dead_code)]
    no_new_privileges: Option<bool>,
    private_tmp: Option<bool>,
    memory_max: Option<String>,
    cpu_quota: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortSpec {
    container: u16,
    host_env: Option<String>,
    default: Option<u16>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceSpec {
    memory_limit: Option<String>,
    memory_reservation: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistenceSpec {
    name: String,
    container_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementsSpec {
    env_one_of: Option<Vec<String>>,
    env_optional: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Manifest validation
// ---------------------------------------------------------------------------

/// Validate a parsed `FullAgentManifest` against the same rules as
/// `generate-agent.sh`. Returns `Ok(())` or an error listing all violations.
fn validate_full_manifest(manifest: &FullAgentManifest) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    if manifest.api_version != "polis.dev/v1" {
        errors.push("Unsupported apiVersion. Expected polis.dev/v1".to_string());
    }

    if manifest.kind != "AgentPlugin" {
        errors.push("Unsupported kind. Expected AgentPlugin".to_string());
    }

    if !AGENT_NAME_RE.is_match(&manifest.metadata.name) {
        errors.push(format!(
            "metadata.name '{}' must be lowercase alphanumeric with hyphens",
            manifest.metadata.name
        ));
    }

    if manifest.spec.packaging != "script" {
        errors.push("Only 'script' packaging is supported".to_string());
    }

    let cmd = &manifest.spec.runtime.command;
    if !cmd.starts_with('/') {
        errors.push("runtime.command must start with /".to_string());
    }
    if SHELL_METACHAR_RE.is_match(cmd) {
        errors.push("runtime.command contains shell metacharacters".to_string());
    }

    if manifest.spec.runtime.user == "root" {
        errors.push("Agents must run as unprivileged user (not root)".to_string());
    }

    // install/init path escape check
    if let Some(install) = &manifest.spec.install
        && install.contains("..")
    {
        errors.push("spec.install path escapes agent directory".to_string());
    }
    if let Some(init) = &manifest.spec.init
        && init.contains("..")
    {
        errors.push("spec.init path escapes agent directory".to_string());
    }

    // Port conflict check
    if let Some(ports) = &manifest.spec.ports {
        for port_spec in ports {
            let port = port_spec.default.unwrap_or(port_spec.container);
            if PLATFORM_PORTS.contains(&port) {
                errors.push(format!("Port {port} conflicts with platform service"));
            }
        }
    }

    // readWritePaths prefix check
    if let Some(security) = &manifest.spec.security
        && let Some(rw_paths) = &security.read_write_paths
    {
        for path in rw_paths {
            let allowed = ALLOWED_RW_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix));
            if !allowed {
                errors.push(format!(
                    "readWritePaths entry '{path}' is outside allowed prefixes: {}",
                    ALLOWED_RW_PREFIXES.join(", ")
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("Agent manifest validation failed:\n{}", errors.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// Artifact generators
// ---------------------------------------------------------------------------

/// Generate `compose.agent.yaml` — Docker Compose overlay with port mappings,
/// volumes, healthcheck, and socat proxy sidecars.
fn generate_compose_overlay(manifest: &FullAgentManifest, out_dir: &Path) -> Result<()> {
    let name = &manifest.metadata.name;
    let spec = &manifest.spec;

    let health_interval = spec
        .health
        .as_ref()
        .and_then(|h| h.interval.as_deref())
        .unwrap_or("30s");
    let health_timeout = spec
        .health
        .as_ref()
        .and_then(|h| h.timeout.as_deref())
        .unwrap_or("10s");
    let health_retries = spec.health.as_ref().and_then(|h| h.retries).unwrap_or(3);
    let health_start_period = spec
        .health
        .as_ref()
        .and_then(|h| h.start_period.as_deref())
        .unwrap_or("60s");
    let health_cmd = spec
        .health
        .as_ref()
        .and_then(|h| h.command.as_deref())
        .unwrap_or("");

    let healthcheck_test = format!(
        "systemctl is-active polis-init.service && systemctl is-active {name}.service && {health_cmd} && ip route | grep -q default"
    );

    let mut out = String::new();
    out.push_str(&format!(
        "# Generated from agents/{name}/agent.yaml - DO NOT EDIT\n"
    ));
    out.push_str("services:\n");
    out.push_str("  workspace:\n");
    out.push_str("    env_file:\n");
    out.push_str("      - .env\n");
    out.push_str("    volumes:\n");
    out.push_str(&format!(
        "      - ./agents/{name}/:/opt/agents/{name}/:ro\n"
    ));
    out.push_str(&format!(
        "      - ./agents/{name}/.generated/{name}.service:/etc/systemd/system/{name}.service:ro\n"
    ));
    out.push_str(&format!("      - ./agents/{name}/.generated/{name}.service.sha256:/etc/systemd/system/{name}.service.sha256:ro\n"));

    // Persistence volume mounts
    if let Some(persistence) = &spec.persistence {
        for p in persistence {
            out.push_str(&format!(
                "      - polis-agent-{name}-{}:{}\n",
                p.name, p.container_path
            ));
        }
    }

    out.push_str("    healthcheck:\n");
    out.push_str(&format!(
        "      test: [\"CMD-SHELL\", \"{healthcheck_test}\"]\n"
    ));
    out.push_str(&format!("      interval: {health_interval}\n"));
    out.push_str(&format!("      timeout: {health_timeout}\n"));
    out.push_str(&format!("      retries: {health_retries}\n"));
    out.push_str(&format!("      start_period: {health_start_period}\n"));

    // Resources
    let mem_limit = spec
        .resources
        .as_ref()
        .and_then(|r| r.memory_limit.as_deref());
    let mem_reservation = spec
        .resources
        .as_ref()
        .and_then(|r| r.memory_reservation.as_deref());
    if mem_limit.is_some() || mem_reservation.is_some() {
        out.push_str("    deploy:\n");
        out.push_str("      resources:\n");
        if let Some(limit) = mem_limit {
            out.push_str("        limits:\n");
            out.push_str(&format!("          memory: {limit}\n"));
        }
        if let Some(reservation) = mem_reservation {
            out.push_str("        reservations:\n");
            out.push_str(&format!("          memory: {reservation}\n"));
        }
    }

    // Socat proxy sidecars (one per port)
    if let Some(ports) = &spec.ports
        && !ports.is_empty()
    {
        out.push('\n');
        for port_spec in ports {
            let container_port = port_spec.container;
            let host_env = port_spec.host_env.as_deref().unwrap_or("");
            let default_port = port_spec.default.unwrap_or(container_port);
            out.push_str(&format!("  {name}-proxy-{container_port}:\n"));
            out.push_str("    image: alpine/socat:latest\n");
            out.push_str("    restart: unless-stopped\n");
            out.push_str("    ports:\n");
            if host_env.is_empty() {
                out.push_str(&format!("      - \"{default_port}:{container_port}\"\n"));
            } else {
                out.push_str(&format!(
                    "      - \"${{{host_env}:-{default_port}}}:{container_port}\"\n"
                ));
            }
            out.push_str(&format!(
                    "    command: TCP-LISTEN:{container_port},fork,reuseaddr TCP:polis-workspace:{container_port}\n"
                ));
            out.push_str("    networks:\n");
            out.push_str("      - internal-bridge\n");
            out.push_str("      - default\n");
            out.push_str("    depends_on:\n");
            out.push_str("      - workspace\n");
        }
    }

    // Top-level volumes section
    if let Some(persistence) = &spec.persistence
        && !persistence.is_empty()
    {
        out.push('\n');
        out.push_str("volumes:\n");
        for p in persistence {
            out.push_str(&format!("  polis-agent-{name}-{}:\n", p.name));
            out.push_str(&format!("    name: polis-agent-{name}-{}\n", p.name));
        }
    }

    let out_path = out_dir.join("compose.agent.yaml");
    std::fs::write(&out_path, out).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Generate `<name>.service` — systemd unit with security hardening.
fn generate_systemd_unit(manifest: &FullAgentManifest, out_dir: &Path) -> Result<()> {
    let name = &manifest.metadata.name;
    let spec = &manifest.spec;
    let runtime = &spec.runtime;

    let display_name = manifest.metadata.display_name.as_deref().unwrap_or(name);
    let protect_system = spec
        .security
        .as_ref()
        .and_then(|s| s.protect_system.as_deref())
        .unwrap_or("strict");
    let protect_home = spec
        .security
        .as_ref()
        .and_then(|s| s.protect_home.as_deref())
        .unwrap_or("true");
    let private_tmp = spec
        .security
        .as_ref()
        .and_then(|s| s.private_tmp)
        .unwrap_or(true);
    let mem_max = spec.security.as_ref().and_then(|s| s.memory_max.as_deref());
    let cpu_quota = spec.security.as_ref().and_then(|s| s.cpu_quota.as_deref());
    let rw_paths = spec
        .security
        .as_ref()
        .and_then(|s| s.read_write_paths.as_ref())
        .map(|paths| paths.join(" "));

    let mut out = String::new();
    out.push_str(&format!(
        "# Generated from agents/{name}/agent.yaml - DO NOT EDIT\n"
    ));
    out.push_str("[Unit]\n");
    out.push_str(&format!("Description={display_name}\n"));
    out.push_str("After=network-online.target polis-init.service\n");
    out.push_str("Wants=network-online.target\n");
    out.push_str("Requires=polis-init.service\n");
    out.push_str("StartLimitIntervalSec=300\n");
    out.push_str("StartLimitBurst=3\n");
    out.push('\n');
    out.push_str("[Service]\n");
    out.push_str("Type=simple\n");
    out.push_str(&format!("User={}\n", runtime.user));
    if let Some(workdir) = &runtime.workdir {
        out.push_str(&format!("WorkingDirectory={workdir}\n"));
    }
    out.push('\n');
    if let Some(env_file) = &runtime.env_file {
        out.push_str(&format!("EnvironmentFile=-{env_file}\n"));
    }
    out.push('\n');
    out.push_str("Environment=NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/polis-ca.crt\n");
    out.push_str("Environment=SSL_CERT_FILE=/usr/local/share/ca-certificates/polis-ca.crt\n");
    out.push_str("Environment=REQUESTS_CA_BUNDLE=/usr/local/share/ca-certificates/polis-ca.crt\n");

    // Inline env vars from spec.runtime.env
    if let Some(env_map) = &runtime.env {
        // Sort for deterministic output
        let mut entries: Vec<(&String, &String)> = env_map.iter().collect();
        entries.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in entries {
            out.push_str(&format!("Environment=\"{k}={v}\"\n"));
        }
    }

    out.push('\n');
    if let Some(init) = &spec.init {
        out.push_str(&format!(
            "ExecStartPre=+/bin/bash /opt/agents/{name}/{init}\n"
        ));
    }
    out.push_str(&format!("ExecStart={}\n", runtime.command));
    out.push('\n');
    out.push_str("Restart=always\n");
    out.push_str("RestartSec=5\n");
    out.push_str("StartLimitBurst=3\n");
    out.push('\n');
    out.push_str("NoNewPrivileges=true\n");
    out.push_str(&format!("ProtectSystem={protect_system}\n"));
    out.push_str(&format!("ProtectHome={protect_home}\n"));
    if let Some(paths) = &rw_paths {
        out.push_str(&format!("ReadWritePaths={paths}\n"));
    }
    out.push_str(&format!("PrivateTmp={private_tmp}\n"));
    if let Some(mem) = mem_max {
        out.push_str(&format!("MemoryMax={mem}\n"));
    }
    if let Some(cpu) = cpu_quota {
        out.push_str(&format!("CPUQuota={cpu}\n"));
    }
    out.push('\n');
    out.push_str("[Install]\n");
    out.push_str("WantedBy=multi-user.target\n");

    let out_path = out_dir.join(format!("{name}.service"));
    std::fs::write(&out_path, out).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Generate `<name>.service.sha256` — SHA256 hash of the service file.
fn generate_service_hash(manifest: &FullAgentManifest, out_dir: &Path) -> Result<()> {
    let name = &manifest.metadata.name;
    let service_path = out_dir.join(format!("{name}.service"));
    let content = std::fs::read(&service_path)
        .with_context(|| format!("reading {}", service_path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let hash = format!("{:x}\n", hasher.finalize());
    let out_path = out_dir.join(format!("{name}.service.sha256"));
    std::fs::write(&out_path, hash).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Generate `<name>.env` — filtered env vars from the polis `.env` file,
/// keeping only keys declared in `spec.requirements`.
fn generate_filtered_env(
    manifest: &FullAgentManifest,
    out_dir: &Path,
    polis_dir: &Path,
) -> Result<()> {
    let name = &manifest.metadata.name;

    // Collect all declared keys from requirements
    let mut declared_keys: Vec<String> = Vec::new();
    if let Some(reqs) = &manifest.spec.requirements {
        if let Some(one_of) = &reqs.env_one_of {
            declared_keys.extend(one_of.iter().cloned());
        }
        if let Some(optional) = &reqs.env_optional {
            declared_keys.extend(optional.iter().cloned());
        }
    }

    let mut filtered_lines: Vec<String> = Vec::new();
    let env_path = polis_dir.join(".env");
    if env_path.exists() {
        let content = std::fs::read_to_string(&env_path)
            .with_context(|| format!("reading {}", env_path.display()))?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            let key = trimmed.split('=').next().unwrap_or("").trim();
            if declared_keys.iter().any(|k| k == key) {
                filtered_lines.push(line.to_string());
            }
        }
    }

    let out_path = out_dir.join(format!("{name}.env"));
    let content = if filtered_lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", filtered_lines.join("\n"))
    };
    std::fs::write(&out_path, content)
        .with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Parse an `agent.yaml` manifest into a strongly-typed `FullAgentManifest`.
fn parse_agent_manifest(path: &Path) -> Result<FullAgentManifest> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading agent manifest: {}", path.display()))?;
    serde_yaml::from_str(&content).context("parsing agent.yaml")
}

/// Generate all 4 runtime artifacts for an agent from its manifest.
///
/// This replaces the VM-side invocation of `generate-agent.sh` for
/// `polis agent add` (Requirement 16.1). Produces:
/// - `compose.agent.yaml`
/// - `<name>.service`
/// - `<name>.service.sha256`
/// - `<name>.env`
///
/// # Errors
///
/// Returns an error if the manifest is invalid or any file cannot be written.
pub(crate) fn generate_agent_artifacts(polis_dir: &Path, agent_name: &str) -> Result<()> {
    let agent_dir = polis_dir.join("agents").join(agent_name);
    let manifest = parse_agent_manifest(&agent_dir.join("agent.yaml"))?;
    validate_full_manifest(&manifest)?;
    let out_dir = agent_dir.join(".generated");
    std::fs::create_dir_all(&out_dir).context("creating .generated directory")?;

    generate_compose_overlay(&manifest, &out_dir)?;
    generate_systemd_unit(&manifest, &out_dir)?;
    generate_service_hash(&manifest, &out_dir)?;
    generate_filtered_env(&manifest, &out_dir, polis_dir)?;

    Ok(())
}

fn validate_agent_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        AGENT_NAME_RE.is_match(name),
        "Invalid agent name '{name}': must match ^[a-z0-9]([a-z0-9-]{{0,61}}[a-z0-9])?$"
    );
    Ok(())
}

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Install a new agent from a local folder
    Add(AddArgs),
    /// Remove an installed agent
    Remove(RemoveArgs),
    /// List available agents
    List,
    /// Restart the active agent's workspace
    Restart,
    /// Re-generate artifacts and recreate workspace
    Update,
    /// Open an interactive shell in the workspace container
    Shell,
    /// Run a command in the workspace container
    Exec(ExecArgs),
    /// Run an agent-specific command (defined in agents/<name>/commands.sh)
    Cmd(CmdArgs),
}

#[derive(Args)]
pub struct AddArgs {
    /// Path to agent folder on host (must contain agent.yaml)
    #[arg(long)]
    pub path: String,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Agent name to remove
    pub name: String,
}

#[derive(Args)]
pub struct ExecArgs {
    /// Command and arguments to run in the workspace container
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

#[derive(Args)]
pub struct CmdArgs {
    /// Agent-specific subcommand (e.g. token, devices, onboard)
    #[arg(trailing_var_arg = true, required = true)]
    pub args: Vec<String>,
}

/// Minimal agent manifest shape — only the fields we need.
#[derive(Deserialize)]
struct AgentManifest {
    metadata: AgentManifestMetadata,
}

#[derive(Deserialize)]
struct AgentManifestMetadata {
    name: String,
}

/// Run the given agent subcommand.
///
/// # Errors
///
/// Returns an error if the subcommand fails (e.g. VM not running, agent not
/// found, artifact generation failure).
pub async fn run(
    cmd: AgentCommand,
    mp: &(impl ShellExecutor + FileTransfer + InstanceInspector),
    ctx: &OutputContext,
    json: bool,
) -> Result<()> {
    match cmd {
        AgentCommand::Add(args) => add(&args, mp, ctx).await,
        AgentCommand::Remove(args) => remove(&args, mp, ctx).await,
        AgentCommand::List => list(mp, ctx, json).await,
        AgentCommand::Restart => restart(mp, ctx).await,
        AgentCommand::Update => update(mp, ctx).await,
        AgentCommand::Shell => shell(mp).await,
        AgentCommand::Exec(args) => exec_cmd(mp, &args).await,
        AgentCommand::Cmd(args) => agent_cmd(mp, &args).await,
    }
}

async fn add(
    args: &AddArgs,
    mp: &(impl ShellExecutor + FileTransfer + InstanceInspector),
    ctx: &OutputContext,
) -> Result<()> {
    let name = validate_agent_folder(&args.path)?;
    require_vm_running(mp).await?;
    ensure_agent_not_exists(mp, &name).await?;

    let agent_folder = std::path::Path::new(&args.path);
    let parent_dir = agent_folder
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cannot determine parent directory of agent folder"))?;
    let polis_dir = parent_dir.parent().unwrap_or(parent_dir);

    if !ctx.quiet {
        println!("Generating artifacts...");
    }
    generate_agent_artifacts(polis_dir, &name)?;

    transfer_agent_to_vm(mp, ctx, &args.path, &name).await?;

    if !ctx.quiet {
        println!("Agent '{name}' installed. Start with: polis start --agent {name}");
    }
    Ok(())
}

/// Validate agent folder and return the agent name from manifest.
fn validate_agent_folder(path: &str) -> Result<String> {
    let folder = std::path::Path::new(path);
    anyhow::ensure!(folder.exists(), "Path not found: {path}");

    let manifest_path = folder.join("agent.yaml");
    anyhow::ensure!(manifest_path.exists(), "No agent.yaml found in: {path}");

    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: AgentManifest = serde_yaml::from_str(&content)
        .context("failed to parse agent.yaml: missing or invalid metadata.name")?;

    anyhow::ensure!(
        !manifest.metadata.name.is_empty(),
        "metadata.name is empty in agent.yaml"
    );
    validate_agent_name(&manifest.metadata.name)?;
    Ok(manifest.metadata.name)
}

/// Ensure VM is running.
async fn require_vm_running(mp: &impl InstanceInspector) -> Result<()> {
    anyhow::ensure!(
        vm::state(mp).await? == vm::VmState::Running,
        "VM is not running. Start it first: polis start"
    );
    Ok(())
}

/// Ensure agent doesn't already exist.
async fn ensure_agent_not_exists(mp: &impl ShellExecutor, name: &str) -> Result<()> {
    let target_dir = format!("{VM_ROOT}/agents/{name}");
    let exists = mp.exec(&["test", "-d", &target_dir]).await?;
    anyhow::ensure!(
        !exists.status.success(),
        "Agent '{name}' already installed. Remove it first: polis agent remove {name}"
    );
    Ok(())
}

/// Transfer agent folder to VM.
async fn transfer_agent_to_vm(
    mp: &impl FileTransfer,
    ctx: &OutputContext,
    path: &str,
    name: &str,
) -> Result<()> {
    if !ctx.quiet {
        println!("Copying agent '{name}' to VM...");
    }
    let dest = format!("{VM_ROOT}/agents/{name}");
    let out = mp
        .transfer_recursive(path, &dest)
        .await
        .context("multipass transfer")?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to transfer agent folder: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

async fn remove(
    args: &RemoveArgs,
    mp: &(impl ShellExecutor + InstanceInspector),
    ctx: &OutputContext,
) -> Result<()> {
    let name = &args.name;
    validate_agent_name(name)?;
    let agent_dir = format!("{VM_ROOT}/agents/{name}");

    // Must exist
    let exists = mp.exec(&["test", "-d", &agent_dir]).await?;
    anyhow::ensure!(exists.status.success(), "Agent '{name}' is not installed.");

    let state_mgr = StateManager::new()?;
    let active = state_mgr.load()?.and_then(|s| s.active_agent);
    let is_active = active.as_deref() == Some(name.as_str());

    if is_active {
        if !ctx.quiet {
            println!("Stopping active agent '{name}'...");
        }
        let base = format!("{VM_ROOT}/docker-compose.yml");
        let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
        let down = mp
            .exec(&["docker", "compose", "-f", &base, "-f", &overlay, "down"])
            .await?;
        anyhow::ensure!(
            down.status.success(),
            "Failed to stop stack: {}",
            String::from_utf8_lossy(&down.stderr)
        );
    }

    let rm = mp.exec(&["rm", "-rf", &agent_dir]).await?;
    anyhow::ensure!(
        rm.status.success(),
        "Failed to remove agent directory: {}",
        String::from_utf8_lossy(&rm.stderr)
    );

    if is_active {
        if !ctx.quiet {
            println!("Restarting control plane...");
        }
        let base = format!("{VM_ROOT}/docker-compose.yml");
        let up = mp
            .exec(&["docker", "compose", "-f", &base, "up", "-d"])
            .await?;
        anyhow::ensure!(
            up.status.success(),
            "Failed to restart control plane: {}",
            String::from_utf8_lossy(&up.stderr)
        );

        if let Ok(Some(mut state)) = state_mgr.load() {
            state.active_agent = None;
            let _ = state_mgr.save(&state);
        }
    }

    if !ctx.quiet {
        println!("Agent '{name}' removed.");
    }
    Ok(())
}

async fn list(mp: &impl ShellExecutor, ctx: &OutputContext, json: bool) -> Result<()> {
    // Scan agents/*/agent.yaml inside VM (exclude _template).
    // Use `cat` to read each file and parse with serde_yaml on the host — no yq needed.
    let scan = mp
        .exec(&[
            "bash",
            "-c",
            &format!(
                "for f in {VM_ROOT}/agents/*/agent.yaml; do \
                   dir=$(dirname \"$f\"); \
                   name=$(basename \"$dir\"); \
                   [ \"$name\" = \"_template\" ] && continue; \
                   [ -f \"$f\" ] || continue; \
                   printf '===AGENT:%s===\\n' \"$name\"; \
                   cat \"$f\"; \
                   printf '\\n===END===\\n'; \
                 done"
            ),
        ])
        .await?;

    let output = String::from_utf8_lossy(&scan.stdout);
    let state_mgr = StateManager::new()?;
    let active = state_mgr.load()?.and_then(|s| s.active_agent);

    // Parse each agent block: split on ===AGENT:<name>=== ... ===END===
    let mut agents: Vec<serde_json::Value> = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_yaml = String::new();
    for line in output.lines() {
        if let Some(name) = line
            .strip_prefix("===AGENT:")
            .and_then(|s| s.strip_suffix("==="))
        {
            current_name = Some(name.to_string());
            current_yaml.clear();
        } else if line == "===END===" {
            if let Some(dir_name) = current_name.take() {
                let is_active = active.as_deref() == Some(&dir_name);
                // Parse the yaml block with serde_yaml
                #[derive(serde::Deserialize)]
                struct ListManifest {
                    metadata: ListMetadata,
                }
                #[derive(serde::Deserialize)]
                struct ListMetadata {
                    name: Option<String>,
                    version: Option<String>,
                    description: Option<String>,
                }
                let entry = if let Ok(m) = serde_yaml::from_str::<ListManifest>(&current_yaml) {
                    serde_json::json!({
                        "name": m.metadata.name,
                        "version": m.metadata.version,
                        "description": m.metadata.description,
                        "active": is_active,
                    })
                } else {
                    eprintln!("warning: skipping malformed agent entry: {dir_name}");
                    continue;
                };
                agents.push(entry);
                current_yaml.clear();
            }
        } else if current_name.is_some() {
            current_yaml.push_str(line);
            current_yaml.push('\n');
        }
    }

    if json {
        println!("{}", serde_json::json!({ "agents": agents }));
        return Ok(());
    }

    if agents.is_empty() {
        if !ctx.quiet {
            println!("No agents installed. Install one: polis agent add --path <folder>");
        }
        return Ok(());
    }

    println!("Available agents:\n");
    for agent in &agents {
        let name = agent["name"].as_str().unwrap_or("(unknown)");
        let version = agent["version"].as_str().unwrap_or("");
        let desc = agent["description"].as_str().unwrap_or("");
        let marker = if agent["active"].as_bool().unwrap_or(false) {
            "  [active]"
        } else {
            ""
        };
        println!("  {name:<16} {version:<10} {desc}{marker}");
    }
    println!("\nStart an agent: polis start --agent <name>");
    Ok(())
}

async fn restart(mp: &(impl ShellExecutor + InstanceInspector), ctx: &OutputContext) -> Result<()> {
    let state_mgr = StateManager::new()?;
    let name = state_mgr
        .load()?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("no active agent. Start one: polis start --agent <name>"))?;

    anyhow::ensure!(
        vm::state(mp).await? == vm::VmState::Running,
        "Workspace is not running."
    );

    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
    let out = mp
        .exec(&[
            "docker",
            "compose",
            "-f",
            &base,
            "-f",
            &overlay,
            "restart",
            "workspace",
        ])
        .await?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to restart workspace: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    if !ctx.quiet {
        println!("Agent '{name}' workspace restarted.");
    }
    Ok(())
}

async fn update(
    mp: &(impl ShellExecutor + FileTransfer + InstanceInspector),
    ctx: &OutputContext,
) -> Result<()> {
    let state_mgr = StateManager::new()?;
    let name = state_mgr
        .load()?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("no active agent. Start one: polis start --agent <name>"))?;

    if !ctx.quiet {
        println!("Regenerating artifacts for '{name}'...");
    }

    // Read agent.yaml from the VM, regenerate artifacts locally (Rust, no yq),
    // then transfer the updated .generated/ folder back into the VM.
    let cat_out = mp
        .exec(&["cat", &format!("{VM_ROOT}/agents/{name}/agent.yaml")])
        .await
        .context("reading agent.yaml from VM")?;
    anyhow::ensure!(
        cat_out.status.success(),
        "Failed to read agent manifest from VM: {}",
        String::from_utf8_lossy(&cat_out.stderr)
    );

    // Write manifest to a temp dir and run the Rust generator.
    let tmp = tempfile::tempdir().context("creating temp dir for artifact generation")?;
    let agent_dir = tmp.path().join("agents").join(&name);
    std::fs::create_dir_all(&agent_dir).context("creating temp agent dir")?;
    std::fs::write(agent_dir.join("agent.yaml"), &cat_out.stdout)
        .context("writing agent.yaml to temp dir")?;

    generate_agent_artifacts(tmp.path(), &name)?;

    // Transfer the regenerated .generated/ folder back into the VM.
    let generated_src = agent_dir.join(".generated");
    let generated_src_str = generated_src.to_string_lossy();
    let generated_dest = format!("{VM_ROOT}/agents/{name}/.generated");
    let transfer_out = mp
        .transfer_recursive(&generated_src_str, &generated_dest)
        .await
        .context("transferring regenerated artifacts to VM")?;
    anyhow::ensure!(
        transfer_out.status.success(),
        "Failed to transfer regenerated artifacts: {}",
        String::from_utf8_lossy(&transfer_out.stderr)
    );

    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = format!("{VM_ROOT}/agents/{name}/.generated/compose.agent.yaml");
    let out = mp
        .exec(&[
            "docker",
            "compose",
            "-f",
            &base,
            "-f",
            &overlay,
            "up",
            "-d",
            "--force-recreate",
            "workspace",
        ])
        .await?;
    anyhow::ensure!(
        out.status.success(),
        "Failed to recreate workspace: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    if !ctx.quiet {
        println!("Agent '{name}' updated and workspace recreated.");
    }
    Ok(())
}

/// Require the VM to be running; return an error otherwise.
async fn require_running(mp: &impl InstanceInspector) -> Result<()> {
    anyhow::ensure!(
        vm::state(mp).await? == vm::VmState::Running,
        "Workspace is not running. Start it first: polis start --agent <name>"
    );
    Ok(())
}

/// Return the active agent name from state, or error.
fn require_active_agent() -> Result<String> {
    StateManager::new()?
        .load()?
        .and_then(|s| s.active_agent)
        .ok_or_else(|| anyhow::anyhow!("no active agent. Start one: polis start --agent <name>"))
}

async fn shell(mp: &(impl ShellExecutor + InstanceInspector)) -> Result<()> {
    require_running(mp).await?;
    let name = require_active_agent()?;

    // Read runtime.user from agent manifest inside the VM using cat + serde_yaml — no yq needed.
    let cat_out = mp
        .exec(&["cat", &format!("{VM_ROOT}/agents/{name}/agent.yaml")])
        .await?;
    let user = if cat_out.status.success() {
        let content = String::from_utf8_lossy(&cat_out.stdout);
        let manifest: serde_yaml::Value =
            serde_yaml::from_str(&content).unwrap_or(serde_yaml::Value::Null);
        manifest
            .get("spec")
            .and_then(|s| s.get("runtime"))
            .and_then(|r| r.get("user"))
            .and_then(|u| u.as_str())
            .unwrap_or("root")
            .to_string()
    } else {
        "root".to_string()
    };

    let status = mp
        .exec_status(&["docker", "exec", "-it", "-u", &user, CONTAINER_NAME, "bash"])
        .await?;
    std::process::exit(status.code().unwrap_or(1));
}

async fn exec_cmd(mp: &(impl ShellExecutor + InstanceInspector), args: &ExecArgs) -> Result<()> {
    require_running(mp).await?;
    let mut cmd_args: Vec<&str> = vec!["docker", "exec", CONTAINER_NAME];
    let refs: Vec<&str> = args.command.iter().map(String::as_str).collect();
    cmd_args.extend(&refs);
    let status = mp.exec_status(&cmd_args).await?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

async fn agent_cmd(mp: &(impl ShellExecutor + InstanceInspector), args: &CmdArgs) -> Result<()> {
    require_running(mp).await?;
    let name = require_active_agent()?;
    let commands_sh = format!("{VM_ROOT}/agents/{name}/commands.sh");

    // Verify commands.sh exists
    let check = mp.exec(&["test", "-f", &commands_sh]).await?;
    anyhow::ensure!(check.status.success(), "Agent '{name}' has no commands.sh");

    let mut cmd_args: Vec<&str> = vec!["bash", &commands_sh, CONTAINER_NAME];
    let refs: Vec<&str> = args.args.iter().map(String::as_str).collect();
    cmd_args.extend(&refs);
    let status = mp.exec_status(&cmd_args).await?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // AgentMetadata is test-only: validates that agent.yaml can be parsed
    // via serde_yaml without yq (Requirement 8.3 validation).
    #[derive(Debug)]
    struct AgentMetadata {
        compose_project: String,
        runtime_user: String,
    }

    impl AgentMetadata {
        fn from_manifest(manifest: &serde_yaml::Value) -> anyhow::Result<Self> {
            let compose_project = manifest
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("agent.yaml: metadata.name is missing or empty"))?
                .to_string();

            let runtime_user = manifest
                .get("spec")
                .and_then(|s| s.get("runtime"))
                .and_then(|r| r.get("user"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("agent.yaml: spec.runtime.user is missing or empty")
                })?
                .to_string();

            Ok(Self {
                compose_project,
                runtime_user,
            })
        }
    }

    fn read_agent_metadata(
        polis_dir: &std::path::Path,
        agent_name: &str,
    ) -> anyhow::Result<AgentMetadata> {
        let manifest_path = polis_dir.join("agents").join(agent_name).join("agent.yaml");
        let content = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading agent manifest: {}", manifest_path.display()))?;
        let manifest: serde_yaml::Value =
            serde_yaml::from_str(&content).context("parsing agent.yaml")?;
        AgentMetadata::from_manifest(&manifest)
    }

    #[test]
    fn test_agent_manifest_parses_name() {
        let yaml = "metadata:\n  name: my-agent\n";
        let m: AgentManifest = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(m.metadata.name, "my-agent");
    }

    #[test]
    fn test_agent_manifest_missing_name_returns_error() {
        let yaml = "metadata:\n  version: v1.0\n";
        assert!(serde_yaml::from_str::<AgentManifest>(yaml).is_err());
    }

    #[test]
    fn test_valid_agent_names() {
        for name in ["a", "a1", "my-agent", "agent-0", "0", "abc"] {
            assert!(validate_agent_name(name).is_ok(), "should accept '{name}'");
        }
    }

    #[test]
    fn test_path_traversal_rejected() {
        for name in ["../../.ssh", "../etc", "a/b", "a\\b", ".hidden"] {
            assert!(validate_agent_name(name).is_err(), "should reject '{name}'");
        }
    }

    #[test]
    fn test_invalid_agent_names() {
        for name in ["", "-start", "end-", "UPPER", "has space", "a--b-"] {
            assert!(validate_agent_name(name).is_err(), "should reject '{name}'");
        }
    }

    #[test]
    fn test_max_length_boundary() {
        let max = "a".repeat(63);
        assert!(validate_agent_name(&max).is_ok());
        let over = "a".repeat(64);
        assert!(validate_agent_name(&over).is_err());
    }

    // --- AgentMetadata::from_manifest() tests ---

    #[test]
    fn test_agent_metadata_from_manifest_parses_fields() {
        let yaml = r"
metadata:
  name: my-agent
spec:
  runtime:
    user: agentuser
";
        let manifest: serde_yaml::Value = serde_yaml::from_str(yaml).expect("parse");
        let meta = AgentMetadata::from_manifest(&manifest).expect("from_manifest");
        assert_eq!(meta.compose_project, "my-agent");
        assert_eq!(meta.runtime_user, "agentuser");
    }

    #[test]
    fn test_agent_metadata_missing_name_returns_error() {
        let yaml = r"
metadata:
  version: v1.0
spec:
  runtime:
    user: agentuser
";
        let manifest: serde_yaml::Value = serde_yaml::from_str(yaml).expect("parse");
        let err = AgentMetadata::from_manifest(&manifest).expect_err("expected Err");
        assert!(err.to_string().contains("metadata.name"), "error: {err}");
    }

    #[test]
    fn test_agent_metadata_missing_runtime_user_returns_error() {
        let yaml = r"
metadata:
  name: my-agent
spec:
  runtime:
    command: /usr/bin/myapp
";
        let manifest: serde_yaml::Value = serde_yaml::from_str(yaml).expect("parse");
        let err = AgentMetadata::from_manifest(&manifest).expect_err("expected Err");
        assert!(
            err.to_string().contains("spec.runtime.user"),
            "error: {err}"
        );
    }

    #[test]
    fn test_agent_metadata_empty_name_returns_error() {
        let yaml = r#"
metadata:
  name: ""
spec:
  runtime:
    user: agentuser
"#;
        let manifest: serde_yaml::Value = serde_yaml::from_str(yaml).expect("parse");
        let err = AgentMetadata::from_manifest(&manifest).expect_err("expected Err");
        assert!(err.to_string().contains("metadata.name"), "error: {err}");
    }

    #[test]
    fn test_read_agent_metadata_reads_from_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(
            agent_dir.join("agent.yaml"),
            "metadata:\n  name: test-agent\nspec:\n  runtime:\n    user: testuser\n",
        )
        .expect("write yaml");

        let meta = read_agent_metadata(dir.path(), "test-agent").expect("read_agent_metadata");
        assert_eq!(meta.compose_project, "test-agent");
        assert_eq!(meta.runtime_user, "testuser");
    }

    #[test]
    fn test_read_agent_metadata_missing_file_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = read_agent_metadata(dir.path(), "nonexistent").expect_err("expected Err");
        assert!(
            err.to_string().contains("reading agent manifest"),
            "error: {err}"
        );
    }

    // ---------------------------------------------------------------------------
    // generate_agent_artifacts() tests
    // ---------------------------------------------------------------------------

    fn minimal_valid_manifest_yaml() -> &'static str {
        r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: test-agent
  displayName: "Test Agent"
  version: "1.0.0"
spec:
  packaging: script
  runtime:
    command: /usr/bin/myapp
    user: polis
  health:
    command: "curl -sf http://127.0.0.1:9000/health"
    interval: 30s
    timeout: 10s
    retries: 3
    startPeriod: 60s
"#
    }

    fn full_manifest_yaml() -> &'static str {
        r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: test-agent
  displayName: "Test Agent"
  version: "1.0.0"
spec:
  packaging: script
  install: install.sh
  init: scripts/init.sh
  runtime:
    command: /usr/bin/myapp --port 9000
    workdir: /app
    user: polis
    envFile: /home/polis/.myapp/.env
    env:
      MY_VAR: myvalue
  health:
    command: "curl -sf http://127.0.0.1:9000/health"
    interval: 30s
    timeout: 10s
    retries: 5
    startPeriod: 120s
  security:
    protectSystem: strict
    protectHome: read-only
    readWritePaths:
      - /opt/polis/agents/test-agent
    noNewPrivileges: true
    privateTmp: true
    memoryMax: 2G
    cpuQuota: "100%"
  ports:
    - container: 9000
      hostEnv: TEST_HOST_PORT
      default: 9000
  resources:
    memoryLimit: 4G
    memoryReservation: 512M
  persistence:
    - name: data
      containerPath: /home/polis/.myapp
  requirements:
    envOneOf:
      - MY_API_KEY
    envOptional:
      - MY_OPTIONAL_KEY
"#
    }

    #[test]
    fn test_validate_full_manifest_valid() {
        let manifest: FullAgentManifest =
            serde_yaml::from_str(full_manifest_yaml()).expect("parse");
        assert!(validate_full_manifest(&manifest).is_ok());
    }

    #[test]
    fn test_validate_full_manifest_wrong_api_version() {
        let yaml = full_manifest_yaml().replace("polis.dev/v1", "polis.dev/v2");
        let manifest: FullAgentManifest = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_full_manifest(&manifest).expect_err("expected Err");
        assert!(err.to_string().contains("apiVersion"), "error: {err}");
    }

    #[test]
    fn test_validate_full_manifest_wrong_kind() {
        let yaml = full_manifest_yaml().replace("AgentPlugin", "SomethingElse");
        let manifest: FullAgentManifest = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_full_manifest(&manifest).expect_err("expected Err");
        assert!(err.to_string().contains("kind"), "error: {err}");
    }

    #[test]
    fn test_validate_full_manifest_root_user_rejected() {
        let yaml = full_manifest_yaml().replace("user: polis", "user: root");
        let manifest: FullAgentManifest = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_full_manifest(&manifest).expect_err("expected Err");
        assert!(err.to_string().contains("unprivileged"), "error: {err}");
    }

    #[test]
    fn test_validate_full_manifest_command_no_leading_slash() {
        let yaml =
            full_manifest_yaml().replace("command: /usr/bin/myapp --port 9000", "command: myapp");
        let manifest: FullAgentManifest = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_full_manifest(&manifest).expect_err("expected Err");
        assert!(err.to_string().contains("runtime.command"), "error: {err}");
    }

    #[test]
    fn test_validate_full_manifest_command_shell_metachar_rejected() {
        for metachar in [";", "|", "&", "`", "$", "(", ")", "\\"] {
            let yaml = full_manifest_yaml().replace(
                "command: /usr/bin/myapp --port 9000",
                &format!("command: /usr/bin/myapp {metachar} evil"),
            );
            let manifest: FullAgentManifest = serde_yaml::from_str(&yaml).expect("parse");
            let err = validate_full_manifest(&manifest).expect_err("expected Err");
            assert!(
                err.to_string().contains("metacharacter"),
                "should reject metachar '{metachar}': {err}"
            );
        }
    }

    #[test]
    fn test_validate_full_manifest_platform_port_conflict() {
        for reserved in [80u16, 443, 8080, 8443, 9090, 5432, 6379, 27017] {
            let yaml = full_manifest_yaml()
                .replace("container: 9000", &format!("container: {reserved}"))
                .replace("default: 9000", &format!("default: {reserved}"));
            let manifest: FullAgentManifest = serde_yaml::from_str(&yaml).expect("parse");
            let err = validate_full_manifest(&manifest).expect_err("expected Err");
            assert!(
                err.to_string().contains("conflicts with platform"),
                "should reject port {reserved}: {err}"
            );
        }
    }

    #[test]
    fn test_validate_full_manifest_rw_path_outside_allowed_prefix() {
        let yaml = full_manifest_yaml().replace("- /opt/polis/agents/test-agent", "- /etc/secret");
        let manifest: FullAgentManifest = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_full_manifest(&manifest).expect_err("expected Err");
        assert!(
            err.to_string().contains("outside allowed prefixes"),
            "error: {err}"
        );
    }

    #[test]
    fn test_validate_full_manifest_packaging_not_script() {
        let yaml = full_manifest_yaml().replace("packaging: script", "packaging: docker");
        let manifest: FullAgentManifest = serde_yaml::from_str(&yaml).expect("parse");
        let err = validate_full_manifest(&manifest).expect_err("expected Err");
        assert!(err.to_string().contains("packaging"), "error: {err}");
    }

    #[test]
    fn test_generate_agent_artifacts_produces_four_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), full_manifest_yaml()).expect("write yaml");

        generate_agent_artifacts(dir.path(), "test-agent").expect("generate");

        let gen_dir = agent_dir.join(".generated");
        assert!(
            gen_dir.join("compose.agent.yaml").exists(),
            "compose.agent.yaml missing"
        );
        assert!(
            gen_dir.join("test-agent.service").exists(),
            "test-agent.service missing"
        );
        assert!(
            gen_dir.join("test-agent.service.sha256").exists(),
            "test-agent.service.sha256 missing"
        );
        assert!(
            gen_dir.join("test-agent.env").exists(),
            "test-agent.env missing"
        );
    }

    #[test]
    fn test_generate_compose_overlay_contains_port_mapping() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), full_manifest_yaml()).expect("write yaml");

        generate_agent_artifacts(dir.path(), "test-agent").expect("generate");

        let compose =
            std::fs::read_to_string(agent_dir.join(".generated").join("compose.agent.yaml"))
                .expect("read compose");
        assert!(
            compose.contains("test-agent-proxy-9000"),
            "socat proxy missing"
        );
        assert!(compose.contains("TEST_HOST_PORT"), "host env var missing");
        assert!(
            compose.contains("polis-agent-test-agent-data"),
            "volume missing"
        );
    }

    #[test]
    fn test_generate_systemd_unit_contains_security_hardening() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), full_manifest_yaml()).expect("write yaml");

        generate_agent_artifacts(dir.path(), "test-agent").expect("generate");

        let service =
            std::fs::read_to_string(agent_dir.join(".generated").join("test-agent.service"))
                .expect("read service");
        assert!(
            service.contains("NoNewPrivileges=true"),
            "NoNewPrivileges missing"
        );
        assert!(
            service.contains("ProtectSystem=strict"),
            "ProtectSystem missing"
        );
        assert!(
            service.contains("ProtectHome=read-only"),
            "ProtectHome missing"
        );
        assert!(service.contains("PrivateTmp=true"), "PrivateTmp missing");
        assert!(service.contains("MemoryMax=2G"), "MemoryMax missing");
        assert!(service.contains("CPUQuota=100%"), "CPUQuota missing");
        assert!(
            service.contains("ExecStart=/usr/bin/myapp"),
            "ExecStart missing"
        );
        assert!(service.contains("User=polis"), "User missing");
        assert!(
            service.contains("ExecStartPre=+/bin/bash /opt/agents/test-agent/scripts/init.sh"),
            "ExecStartPre missing"
        );
    }

    #[test]
    fn test_generate_service_hash_is_sha256_hex() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), full_manifest_yaml()).expect("write yaml");

        generate_agent_artifacts(dir.path(), "test-agent").expect("generate");

        let hash_content = std::fs::read_to_string(
            agent_dir
                .join(".generated")
                .join("test-agent.service.sha256"),
        )
        .expect("read hash");
        let hash = hash_content.trim();
        assert_eq!(hash.len(), 64, "SHA256 hex should be 64 chars, got: {hash}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "not hex: {hash}"
        );
    }

    #[test]
    fn test_generate_service_hash_matches_service_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), full_manifest_yaml()).expect("write yaml");

        generate_agent_artifacts(dir.path(), "test-agent").expect("generate");

        let gen_dir = agent_dir.join(".generated");
        let service_bytes =
            std::fs::read(gen_dir.join("test-agent.service")).expect("read service");
        let mut hasher = Sha256::new();
        hasher.update(&service_bytes);
        let expected = format!("{:x}", hasher.finalize());

        let stored =
            std::fs::read_to_string(gen_dir.join("test-agent.service.sha256")).expect("read hash");
        assert_eq!(stored.trim(), expected, "hash mismatch");
    }

    #[test]
    fn test_generate_filtered_env_only_declared_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), full_manifest_yaml()).expect("write yaml");
        // Write a .env with some keys, only MY_API_KEY and MY_OPTIONAL_KEY are declared
        std::fs::write(
            dir.path().join(".env"),
            "MY_API_KEY=secret\nOTHER_KEY=ignored\nMY_OPTIONAL_KEY=opt\n",
        )
        .expect("write .env");

        generate_agent_artifacts(dir.path(), "test-agent").expect("generate");

        let env_content =
            std::fs::read_to_string(agent_dir.join(".generated").join("test-agent.env"))
                .expect("read env");
        assert!(
            env_content.contains("MY_API_KEY=secret"),
            "declared key missing"
        );
        assert!(
            env_content.contains("MY_OPTIONAL_KEY=opt"),
            "optional key missing"
        );
        assert!(
            !env_content.contains("OTHER_KEY"),
            "undeclared key should be filtered"
        );
    }

    #[test]
    fn test_generate_filtered_env_empty_when_no_env_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), full_manifest_yaml()).expect("write yaml");
        // No .env file

        generate_agent_artifacts(dir.path(), "test-agent").expect("generate");

        let env_content =
            std::fs::read_to_string(agent_dir.join(".generated").join("test-agent.env"))
                .expect("read env");
        assert!(
            env_content.is_empty(),
            "env should be empty when no .env file"
        );
    }

    #[test]
    fn test_generate_artifacts_invalid_manifest_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("bad-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        // root user — should fail validation
        let yaml = r"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: bad-agent
spec:
  packaging: script
  runtime:
    command: /usr/bin/myapp
    user: root
";
        std::fs::write(agent_dir.join("agent.yaml"), yaml).expect("write yaml");

        let err = generate_agent_artifacts(dir.path(), "bad-agent").expect_err("expected Err");
        assert!(err.to_string().contains("unprivileged"), "error: {err}");
    }

    #[test]
    fn test_generate_compose_overlay_minimal_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), minimal_valid_manifest_yaml())
            .expect("write yaml");

        generate_agent_artifacts(dir.path(), "test-agent").expect("generate");

        let compose =
            std::fs::read_to_string(agent_dir.join(".generated").join("compose.agent.yaml"))
                .expect("read compose");
        assert!(compose.contains("workspace:"), "workspace service missing");
        assert!(
            compose.contains("./agents/test-agent/"),
            "agent volume mount missing"
        );
        // No socat proxy when no ports defined
        assert!(
            !compose.contains("proxy"),
            "no proxy expected for minimal manifest"
        );
    }

    #[test]
    fn test_generate_artifacts_deterministic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let agent_dir = dir.path().join("agents").join("test-agent");
        std::fs::create_dir_all(&agent_dir).expect("create dirs");
        std::fs::write(agent_dir.join("agent.yaml"), full_manifest_yaml()).expect("write yaml");

        generate_agent_artifacts(dir.path(), "test-agent").expect("first generate");
        let gen_dir = agent_dir.join(".generated");
        let compose1 = std::fs::read_to_string(gen_dir.join("compose.agent.yaml")).expect("read");
        let service1 = std::fs::read_to_string(gen_dir.join("test-agent.service")).expect("read");
        let hash1 =
            std::fs::read_to_string(gen_dir.join("test-agent.service.sha256")).expect("read");

        generate_agent_artifacts(dir.path(), "test-agent").expect("second generate");
        let compose2 = std::fs::read_to_string(gen_dir.join("compose.agent.yaml")).expect("read");
        let service2 = std::fs::read_to_string(gen_dir.join("test-agent.service")).expect("read");
        let hash2 =
            std::fs::read_to_string(gen_dir.join("test-agent.service.sha256")).expect("read");

        assert_eq!(compose1, compose2, "compose.agent.yaml not deterministic");
        assert_eq!(service1, service2, ".service not deterministic");
        assert_eq!(hash1, hash2, ".service.sha256 not deterministic");
    }
}
