//! Agent artifact generation — pure functions, no I/O, no async.
//!
//! Each function accepts manifest data and returns a `String` containing
//! the artifact content. The caller is responsible for writing to disk.
//!
//! This module has zero imports from `crate::infra`, `crate::commands`,
//! `crate::application`, `tokio`, `std::fs`, `std::process`, or `std::net`.

use polis_common::agent::AgentManifest;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Generate `compose.agent.yaml` content — Docker Compose overlay with port
/// mappings, volumes, healthcheck, and socat proxy sidecars.
///
/// Returns the YAML string — does NOT write to disk.
///
/// Uses typed structs serialized via `serde_yaml_ng` to ensure YAML-special
/// characters are properly escaped and structural errors are caught at compile time.
///
/// # Panics
///
/// Panics if YAML serialization fails, which should never happen as all fields
/// are simple strings and the structure is well-defined.
#[must_use]
pub fn compose_overlay(manifest: &AgentManifest) -> String {
    let name = &manifest.metadata.name;
    let spec = &manifest.spec;

    // Build healthcheck test command
    let health_interval = spec.health.as_ref().map_or("30s", |h| h.interval.as_str());
    let health_timeout = spec.health.as_ref().map_or("10s", |h| h.timeout.as_str());
    let health_retries = spec.health.as_ref().map_or(3, |h| h.retries);
    let health_start_period = spec
        .health
        .as_ref()
        .map_or("60s", |h| h.start_period.as_str());
    let health_cmd = spec.health.as_ref().map_or("", |h| h.command.as_str());

    let healthcheck_test = format!(
        "systemctl is-active polis-init.service && systemctl is-active {name}.service && {health_cmd} && ip route | grep -q default"
    );

    // Build actual volume mounts
    let mut volume_mounts = vec![
        format!("./agents/{name}/:/opt/agents/{name}/:ro"),
        format!("./agents/{name}/.generated/{name}.service:/etc/systemd/system/{name}.service:ro"),
        format!(
            "./agents/{name}/.generated/{name}.service.sha256:/etc/systemd/system/{name}.service.sha256:ro"
        ),
        format!("./agents/{name}/.generated/{name}.env:/run/{name}-env:ro"),
    ];

    // Add persistence volume mounts
    for p in &spec.persistence {
        volume_mounts.push(format!(
            "polis-agent-{name}-{}:{}",
            p.name, p.container_path
        ));
    }

    // Build deploy resources if specified
    let deploy = build_deploy_resources(spec);

    // Build workspace service
    let workspace_service = WorkspaceService {
        env_file: vec![".env".to_string()],
        volumes: volume_mounts,
        healthcheck: ComposeHealthcheck {
            test: vec!["CMD-SHELL".to_string(), healthcheck_test],
            interval: health_interval.to_string(),
            timeout: health_timeout.to_string(),
            retries: health_retries,
            start_period: health_start_period.to_string(),
        },
        deploy,
    };

    // Build services map
    let mut services: BTreeMap<String, ComposeServiceEntry> = BTreeMap::new();
    services.insert(
        "workspace".to_string(),
        ComposeServiceEntry::Workspace(workspace_service),
    );

    // Add socat proxy sidecars (one per port)
    for port_spec in &spec.ports {
        let container_port = port_spec.container;
        let host_env = port_spec.host_env.as_str();
        let default_port = port_spec.default;

        let port_mapping = if host_env.is_empty() {
            format!("{default_port}:{container_port}")
        } else {
            format!("${{{host_env}:-{default_port}}}:{container_port}")
        };

        let socat_service = SocatService {
            image: "alpine/socat:latest".to_string(),
            restart: "unless-stopped".to_string(),
            ports: vec![port_mapping],
            command: format!(
                "TCP-LISTEN:{container_port},fork,reuseaddr TCP:polis-workspace:{container_port}"
            ),
            networks: vec!["internal-bridge".to_string(), "default".to_string()],
            depends_on: vec!["workspace".to_string()],
        };

        services.insert(
            format!("{name}-proxy-{container_port}"),
            ComposeServiceEntry::Socat(socat_service),
        );
    }

    // Build top-level volumes section
    let mut top_level_volumes: BTreeMap<String, ComposeVolume> = BTreeMap::new();
    for p in &spec.persistence {
        let volume_name = format!("polis-agent-{name}-{}", p.name);
        top_level_volumes.insert(volume_name.clone(), ComposeVolume { name: volume_name });
    }

    // Build the complete overlay
    let overlay = ComposeOverlay {
        services,
        volumes: top_level_volumes,
    };

    // Serialize to YAML with header comment
    // Note: serialization of ComposeOverlay should never fail as all fields are
    // simple strings and the structure is well-defined. If it does fail, it
    // indicates a bug in the code that should be caught during development.
    let yaml = serde_yaml_ng::to_string(&overlay)
        .unwrap_or_else(|e| panic!("ComposeOverlay serialization failed unexpectedly: {e}"));

    format!("# Generated from agents/{name}/agent.yaml - DO NOT EDIT\n{yaml}")
}

/// Build deploy resources configuration from the agent spec.
fn build_deploy_resources(spec: &polis_common::agent::AgentSpec) -> Option<ComposeDeploy> {
    let mem_limit = spec.resources.as_ref().map(|r| r.memory_limit.clone());
    let mem_reservation = spec
        .resources
        .as_ref()
        .map(|r| r.memory_reservation.clone());

    if mem_limit.is_none() && mem_reservation.is_none() {
        return None;
    }

    let limits = mem_limit.map(|m| ComposeResourceLimit { memory: Some(m) });

    let reservations = mem_reservation.map(|m| ComposeResourceLimit { memory: Some(m) });

    Some(ComposeDeploy {
        resources: ComposeResources {
            limits,
            reservations,
        },
    })
}

/// Generate `<name>.service` content — systemd unit with security hardening.
///
/// Returns the unit file string — does NOT write to disk.
///
/// # Panics
///
/// Panics if writing to the internal string buffer fails, which should never
/// happen in practice as `String` implements `Write` infallibly.
#[must_use]
pub fn systemd_unit(manifest: &AgentManifest) -> String {
    let name = &manifest.metadata.name;
    let spec = &manifest.spec;
    let runtime = &spec.runtime;

    let display_name = &manifest.metadata.display_name;
    let protect_system = spec
        .security
        .as_ref()
        .map_or("strict", |s| s.protect_system.as_str());
    let protect_home = spec
        .security
        .as_ref()
        .map_or("true", |s| s.protect_home.as_str());
    let private_tmp = spec.security.as_ref().is_none_or(|s| s.private_tmp);
    let mem_max = spec.security.as_ref().and_then(|s| s.memory_max.as_deref());
    let cpu_quota = spec.security.as_ref().and_then(|s| s.cpu_quota.as_deref());
    let rw_paths = spec.security.as_ref().map(|s| s.read_write_paths.join(" "));

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Generated from agents/{name}/agent.yaml - DO NOT EDIT"
    );
    let _ = writeln!(out, "[Unit]");
    let _ = writeln!(out, "Description={display_name}");
    let _ = writeln!(out, "After=network-online.target polis-init.service");
    let _ = writeln!(out, "Wants=network-online.target");
    let _ = writeln!(out, "Requires=polis-init.service");
    let _ = writeln!(out, "StartLimitIntervalSec=300");
    let _ = writeln!(out, "StartLimitBurst=5");
    let _ = writeln!(out);
    let _ = writeln!(out, "[Service]");
    let _ = writeln!(out, "Type=simple");
    let _ = writeln!(out, "User={}", runtime.user);
    let _ = writeln!(out, "WorkingDirectory={}", runtime.workdir);
    let _ = writeln!(out);
    if let Some(env_file) = &runtime.env_file {
        let _ = writeln!(out, "EnvironmentFile=-{env_file}");
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Environment=NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/polis-ca.crt"
    );
    let _ = writeln!(
        out,
        "Environment=SSL_CERT_FILE=/usr/local/share/ca-certificates/polis-ca.crt"
    );
    let _ = writeln!(
        out,
        "Environment=REQUESTS_CA_BUNDLE=/usr/local/share/ca-certificates/polis-ca.crt"
    );

    // Inline env vars from spec.runtime.env (sorted for deterministic output)
    let mut entries: Vec<(&String, &String)> = runtime.env.iter().collect();
    entries.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in entries {
        let _ = writeln!(out, "Environment=\"{k}={v}\"");
    }

    let _ = writeln!(out);
    if let Some(init) = &spec.init {
        let _ = writeln!(out, "ExecStartPre=+/bin/bash /opt/agents/{name}/{init}");
    }
    let _ = writeln!(out, "ExecStart={}", runtime.command);
    let _ = writeln!(out);
    let _ = writeln!(out, "Restart=always");
    let _ = writeln!(out, "RestartSec=5");
    let _ = writeln!(out);
    let _ = writeln!(out, "NoNewPrivileges=true");
    let _ = writeln!(out, "ProtectSystem={protect_system}");
    let _ = writeln!(out, "ProtectHome={protect_home}");
    if let Some(paths) = &rw_paths
        && !paths.is_empty()
    {
        let _ = writeln!(out, "ReadWritePaths={paths}");
    }
    let _ = writeln!(out, "PrivateTmp={private_tmp}");
    if let Some(mem) = mem_max {
        let _ = writeln!(out, "MemoryMax={mem}");
    }
    if let Some(cpu) = cpu_quota {
        let _ = writeln!(out, "CPUQuota={cpu}");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "[Install]");
    let _ = writeln!(out, "WantedBy=multi-user.target");

    out
}

/// Compute SHA256 hash of a service unit content string.
///
/// Returns the hex-encoded hash with a trailing newline, matching the
/// format written to `<name>.service.sha256`.
#[must_use]
pub fn service_hash(unit_content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(unit_content.as_bytes());
    format!("{:x}\n", hasher.finalize())
}

/// Generate filtered env file content from declared requirements.
///
/// Takes the full `.env` file content and the manifest's requirements,
/// returns only the lines whose keys are declared in `spec.requirements`.
/// Returns an empty string if no matching keys are found or no `.env` exists.
#[must_use]
pub fn filtered_env(env_content: &str, manifest: &AgentManifest) -> String {
    // Collect all declared keys from requirements
    let mut declared_keys: Vec<String> = Vec::new();
    if let Some(reqs) = &manifest.spec.requirements {
        declared_keys.extend(reqs.env_one_of.iter().cloned());
        declared_keys.extend(reqs.env_optional.iter().cloned());
    }

    let mut filtered_lines: Vec<String> = Vec::new();
    for line in env_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let key = trimmed.split('=').next().unwrap_or("").trim();
        if declared_keys.iter().any(|k| k == key) {
            filtered_lines.push(line.to_string());
        }
    }

    if filtered_lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", filtered_lines.join("\n"))
    }
}

// ============================================================================
// Typed Compose Overlay Structs
// ============================================================================
//
// These structs model the Docker Compose overlay structure for typed
// serialization via `serde_yaml_ng`. They replace manual string concatenation
// with compile-time validated structures.

/// Top-level Docker Compose overlay structure.
///
/// Contains service definitions and optional named volumes.
#[derive(Debug, Serialize)]
pub(crate) struct ComposeOverlay {
    pub services: BTreeMap<String, ComposeServiceEntry>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub volumes: BTreeMap<String, ComposeVolume>,
}

/// Two distinct service shapes in the compose overlay.
/// Uses `#[serde(untagged)]` so serde picks the variant whose fields match.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum ComposeServiceEntry {
    /// The main workspace container override (`env_file`, volumes, healthcheck, deploy).
    Workspace(WorkspaceService),
    /// A socat TCP-proxy sidecar (image, restart, ports, command, networks, `depends_on`).
    Socat(SocatService),
}

/// Workspace service override configuration.
///
/// Extends the base workspace service with agent-specific env files,
/// volume mounts, healthcheck, and optional resource limits.
#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceService {
    pub env_file: Vec<String>,
    pub volumes: Vec<String>,
    pub healthcheck: ComposeHealthcheck,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy: Option<ComposeDeploy>,
}

/// Socat TCP-proxy sidecar service configuration.
///
/// Creates a proxy container that forwards traffic from the host
/// to the workspace container on a specific port.
#[derive(Debug, Serialize)]
pub(crate) struct SocatService {
    pub image: String,
    pub restart: String,
    pub ports: Vec<String>,
    pub command: String,
    pub networks: Vec<String>,
    pub depends_on: Vec<String>,
}

/// Docker Compose healthcheck configuration.
#[derive(Debug, Serialize)]
pub(crate) struct ComposeHealthcheck {
    pub test: Vec<String>,
    pub interval: String,
    pub timeout: String,
    pub retries: u32,
    pub start_period: String,
}

/// Docker Compose deploy configuration (resource constraints).
#[derive(Debug, Serialize)]
pub(crate) struct ComposeDeploy {
    pub resources: ComposeResources,
}

/// Resource limits and reservations for a service.
#[derive(Debug, Serialize)]
pub(crate) struct ComposeResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<ComposeResourceLimit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reservations: Option<ComposeResourceLimit>,
}

/// Individual resource limit (memory, etc.).
#[derive(Debug, Serialize)]
pub(crate) struct ComposeResourceLimit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

/// Named volume definition in the compose overlay.
#[derive(Debug, Serialize)]
pub(crate) struct ComposeVolume {
    pub name: String,
}
