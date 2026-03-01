//! Agent artifact generation — pure functions, no I/O, no async.
//!
//! Each function accepts manifest data and returns a `String` containing
//! the artifact content. The caller is responsible for writing to disk.
//!
//! This module has zero imports from `crate::infra`, `crate::commands`,
//! `crate::application`, `tokio`, `std::fs`, `std::process`, or `std::net`.

#![allow(clippy::format_push_string)]
#![allow(clippy::too_many_lines)]

use polis_common::agent::AgentManifest;
use sha2::{Digest, Sha256};

/// Generate `compose.agent.yaml` content — Docker Compose overlay with port
/// mappings, volumes, healthcheck, and socat proxy sidecars.
///
/// Returns the YAML string — does NOT write to disk.
#[must_use]
pub fn compose_overlay(manifest: &AgentManifest) -> String {
    let name = &manifest.metadata.name;
    let spec = &manifest.spec;

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
    // Mount agent env file so init scripts can read it (e.g. /run/{name}-env)
    out.push_str(&format!(
        "      - ./agents/{name}/.generated/{name}.env:/run/{name}-env:ro\n"
    ));

    // Persistence volume mounts
    for p in &spec.persistence {
        out.push_str(&format!(
            "      - polis-agent-{name}-{}:{}\n",
            p.name, p.container_path
        ));
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
    append_resource_limits(&mut out, spec);

    // Socat proxy sidecars (one per port)
    append_socat_sidecars(&mut out, name, spec);

    // Top-level volumes section
    if !spec.persistence.is_empty() {
        out.push('\n');
        out.push_str("volumes:\n");
        for p in &spec.persistence {
            out.push_str(&format!("  polis-agent-{name}-{}:\n", p.name));
            out.push_str(&format!("    name: polis-agent-{name}-{}\n", p.name));
        }
    }

    out
}

fn append_resource_limits(out: &mut String, spec: &polis_common::agent::AgentSpec) {
    let mem_limit = spec.resources.as_ref().map(|r| r.memory_limit.as_str());
    let mem_reservation = spec
        .resources
        .as_ref()
        .map(|r| r.memory_reservation.as_str());
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
}

fn append_socat_sidecars(out: &mut String, name: &str, spec: &polis_common::agent::AgentSpec) {
    if spec.ports.is_empty() {
        return;
    }
    out.push('\n');
    for port_spec in &spec.ports {
        let container_port = port_spec.container;
        let host_env = port_spec.host_env.as_str();
        let default_port = port_spec.default;
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

/// Generate `<name>.service` content — systemd unit with security hardening.
///
/// Returns the unit file string — does NOT write to disk.
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
    out.push_str(&format!("WorkingDirectory={}\n", runtime.workdir));
    out.push('\n');
    if let Some(env_file) = &runtime.env_file {
        out.push_str(&format!("EnvironmentFile=-{env_file}\n"));
    }
    out.push('\n');
    out.push_str("Environment=NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/polis-ca.crt\n");
    out.push_str("Environment=SSL_CERT_FILE=/usr/local/share/ca-certificates/polis-ca.crt\n");
    out.push_str("Environment=REQUESTS_CA_BUNDLE=/usr/local/share/ca-certificates/polis-ca.crt\n");

    // Inline env vars from spec.runtime.env (sorted for deterministic output)
    let mut entries: Vec<(&String, &String)> = runtime.env.iter().collect();
    entries.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in entries {
        out.push_str(&format!("Environment=\"{k}={v}\"\n"));
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
    if let Some(paths) = &rw_paths
        && !paths.is_empty()
    {
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
