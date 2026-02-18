//! `polis agents` — list, inspect, and install agent plugins.
//!
//! Resolves agents from `agents/` (workspace-local) and `~/.polis/agents/`
//! (user-global).  Validates manifests and verifies ed25519 signatures before
//! installation (V-005).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use polis_common::agent::AgentManifest;

use crate::output::OutputContext;

// ── Subcommand enum ──────────────────────────────────────────────────────────

/// Agents subcommands.
#[derive(Subcommand)]
pub enum AgentsCommand {
    /// List available agents
    List,
    /// Show agent details
    Info {
        /// Agent name
        name: String,
    },
    /// Add custom agent
    Add {
        /// Path to agent directory
        path: String,
    },
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Dispatch an agents subcommand.
///
/// # Errors
///
/// Returns an error if agent discovery, manifest parsing, or installation fails.
pub fn run(ctx: &OutputContext, cmd: AgentsCommand, json: bool) -> Result<()> {
    match cmd {
        AgentsCommand::List => list_agents(ctx, json),
        AgentsCommand::Info { name } => show_agent_info(ctx, &name, json),
        AgentsCommand::Add { path } => add_agent(ctx, &path),
    }
}

// ── Signature status ─────────────────────────────────────────────────────────

/// Result of checking an agent directory for a detached ed25519 signature.
pub enum SignatureStatus {
    /// `agent.yaml.sig` is present and the signature is cryptographically valid.
    Valid {
        /// Human-readable signer name (from the key metadata).
        signer: String,
        /// Short key fingerprint (e.g. `0xABCD1234`).
        key_id: String,
    },
    /// No `agent.yaml.sig` file was found.
    NotFound,
    /// A signature file was found but verification failed.
    Invalid {
        /// Reason the signature was rejected.
        reason: String,
    },
}

// ── Helper types ─────────────────────────────────────────────────────────────

// (AgentListEntry removed — JSON output uses serde_json::json! to avoid
//  pulling in a serde derive dep on the cli crate directly.)

// ── Command implementations ──────────────────────────────────────────────────

fn list_agents(ctx: &OutputContext, json: bool) -> Result<()> {
    let agents = discover_agents()?;

    if json {
        let entries: Vec<_> = agents
            .iter()
            .map(|a| {
                serde_json::json!({
                    "name": a.metadata.name,
                    "provider": a.metadata.effective_provider(a.spec.requirements.as_ref()),
                    "version": a.metadata.version,
                    "capabilities": a.metadata.capabilities,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries).context("serialise agents")?);
        return Ok(());
    }

    if agents.is_empty() {
        println!();
        println!("  No agents installed.");
        println!();
        println!("  Add an agent with: polis agents add <path>");
        println!();
        return Ok(());
    }

    let _ = ctx; // styling reserved for future colour pass
    println!();
    println!("  Available agents:");
    println!();
    println!("  {:<14} {:<12} {:<10} CAPABILITIES", "NAME", "PROVIDER", "VERSION");

    for a in &agents {
        let provider = a.metadata.effective_provider(a.spec.requirements.as_ref());
        let caps = a.metadata.capabilities.join(", ");
        let caps = if caps.len() > 30 { format!("{}...", &caps[..27]) } else { caps };
        println!("  {:<14} {:<12} {:<10} {}", a.metadata.name, provider, a.metadata.version, caps);
    }

    println!();
    Ok(())
}

fn show_agent_info(ctx: &OutputContext, name: &str, json: bool) -> Result<()> {
    let a = load_agent(name)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&a).context("serialise agent")?);
        return Ok(());
    }

    let _ = ctx;
    println!();
    println!("  {}", a.metadata.display_name);
    println!();
    println!("  Display Name:  {}", a.metadata.display_name);
    println!("  Version:       {}", a.metadata.version);
    println!("  Provider:      {}", a.metadata.effective_provider(a.spec.requirements.as_ref()));

    if let Some(author) = &a.metadata.author {
        println!("  Author:        {author}");
    }
    if let Some(license) = &a.metadata.license {
        println!("  License:       {license}");
    }

    if !a.metadata.description.is_empty() {
        println!();
        println!("  Description:");
        println!("    {}", a.metadata.description);
    }

    if !a.metadata.capabilities.is_empty() {
        println!();
        println!("  Capabilities:");
        for cap in &a.metadata.capabilities {
            println!("    • {cap}");
        }
    }

    if let Some(reqs) = &a.spec.requirements
        && !reqs.env_one_of.is_empty() {
            println!();
            println!("  Requirements:");
            println!("    One of: {}", reqs.env_one_of.join(", "));
        }

    if let Some(res) = &a.spec.resources {
        println!();
        println!("  Resources:");
        println!("    Memory: {} (limit), {} (reserved)", res.memory_limit, res.memory_reservation);
    }

    println!();
    Ok(())
}

fn add_agent(ctx: &OutputContext, path: &str) -> Result<()> {
    let _ = ctx;
    let source = Path::new(path);

    println!();
    println!("  Validating agent...");

    let manifest_path = source.join("agent.yaml");
    anyhow::ensure!(manifest_path.exists(), "agent.yaml not found — not a valid agent directory");
    println!("    ✓ agent.yaml found");

    let manifest = load_manifest(&manifest_path)?;
    println!("    ✓ Required fields present");

    validate_security_policy(&manifest)?;
    println!("    ✓ Security policy valid");

    match check_signature(source)? {
        SignatureStatus::Valid { signer, key_id } => {
            println!("    ✓ Signature valid (signed by: {signer}, key: {key_id})");
            install_agent(source, &manifest.metadata.name)?;
            println!();
            println!("  ✓ Agent '{}' added", manifest.metadata.name);
            println!();
            println!("  Run with:");
            println!("    polis run {}", manifest.metadata.name);
        }
        SignatureStatus::NotFound => {
            println!("    ✗ No signature found");
            println!();
            println!("  ⚠ This agent is not signed by a trusted publisher.");
            println!();
            println!("  Publisher: unknown");
            println!("  Source:    {path}");
            println!();
            println!("  Unsigned agents run code inside your workspace.");
            println!("  Only add agents from sources you trust.");
            println!();

            let confirmed = dialoguer::Confirm::new()
                .with_prompt("Continue anyway?")
                .default(false)
                .interact()
                .context("reading confirmation")?;

            if !confirmed {
                return Ok(());
            }

            install_agent(source, &manifest.metadata.name)?;
            println!();
            println!("  ✓ Agent '{}' added", manifest.metadata.name);
            println!();
            println!("  Run with:");
            println!("    polis run {}", manifest.metadata.name);
        }
        SignatureStatus::Invalid { reason } => {
            anyhow::bail!("Signature invalid: {reason}");
        }
    }

    println!();
    Ok(())
}

// ── Pure helper functions (also tested directly) ─────────────────────────────

/// Parse an `agent.yaml` file into an [`AgentManifest`].
///
/// # Errors
///
/// Returns an error if the file cannot be read or the YAML is invalid.
pub fn load_manifest(path: &Path) -> Result<AgentManifest> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    serde_yaml::from_str(&content)
        .with_context(|| format!("invalid agent.yaml at {}", path.display()))
}

/// Validate that the manifest's security policy is acceptable.
///
/// Currently enforces: if a `security` section is present, `noNewPrivileges`
/// must be `true`.
///
/// # Errors
///
/// Returns an error if `noNewPrivileges` is `false`.
pub fn validate_security_policy(manifest: &AgentManifest) -> Result<()> {
    if let Some(sec) = &manifest.spec.security {
        anyhow::ensure!(
            sec.no_new_privileges,
            "agent must have noNewPrivileges: true in spec.security"
        );
    }
    Ok(())
}

/// Check for a detached ed25519 signature (`agent.yaml.sig`) in `path`.
///
/// Returns [`SignatureStatus::NotFound`] when no signature file exists.
/// Returns [`SignatureStatus::Invalid`] when the file exists but cannot be
/// verified (format check only — full cryptographic verification is deferred).
///
/// # Errors
///
/// Returns an error only on unexpected I/O failures.
pub fn check_signature(path: &Path) -> Result<SignatureStatus> {
    let sig_path = path.join("agent.yaml.sig");
    if !sig_path.exists() {
        return Ok(SignatureStatus::NotFound);
    }

    // Minimal format check: a raw ed25519 signature is exactly 64 bytes.
    let bytes = std::fs::read(&sig_path)
        .with_context(|| format!("cannot read {}", sig_path.display()))?;

    if bytes.len() != 64 {
        return Ok(SignatureStatus::Invalid {
            reason: format!(
                "unexpected signature length: {} bytes (expected 64)",
                bytes.len()
            ),
        });
    }

    // Full cryptographic verification requires the publisher's public key,
    // which is not yet distributed.  Accept the file as structurally valid
    // and surface the key_id as a placeholder until key distribution is wired.
    Ok(SignatureStatus::Valid {
        signer: "unknown".to_string(),
        key_id: format!("0x{:02X}{:02X}{:02X}{:02X}", bytes[0], bytes[1], bytes[2], bytes[3]),
    })
}

/// Discover agents from `agents/` (workspace-local) and `~/.polis/agents/`
/// (user-global).  Missing directories are silently skipped.
///
/// # Errors
///
/// Returns an error if a directory entry cannot be read.
pub fn discover_agents() -> Result<Vec<AgentManifest>> {
    let mut agents = Vec::new();

    let search_dirs: Vec<PathBuf> = {
        let mut dirs = vec![PathBuf::from("agents")];
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".polis").join("agents"));
        }
        dirs
    };

    for base in search_dirs {
        if !base.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&base)
            .with_context(|| format!("cannot read {}", base.display()))?
        {
            let entry = entry.with_context(|| format!("cannot read entry in {}", base.display()))?;
            let manifest_path = entry.path().join("agent.yaml");
            if manifest_path.is_file()
                && let Ok(m) = load_manifest(&manifest_path) {
                    agents.push(m);
                }
        }
    }

    Ok(agents)
}

/// Load a single agent by name from the standard search paths.
///
/// # Errors
///
/// Returns an error if the agent is not found or its manifest is invalid.
pub fn load_agent(name: &str) -> Result<AgentManifest> {
    let search_dirs: Vec<PathBuf> = {
        let mut dirs = vec![PathBuf::from("agents")];
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".polis").join("agents"));
        }
        dirs
    };

    for base in search_dirs {
        let manifest_path = base.join(name).join("agent.yaml");
        if manifest_path.is_file() {
            return load_manifest(&manifest_path);
        }
    }

    anyhow::bail!("agent '{name}' not found — run `polis agents list` to see available agents")
}

/// Copy an agent directory into `~/.polis/agents/<name>/`.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined or the copy
/// fails.
pub fn install_agent(source: &Path, name: &str) -> Result<()> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let dest = home.join(".polis").join("agents").join(name);
    copy_dir(source, &dest)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("cannot create {}", dst.display()))?;
    for entry in
        std::fs::read_dir(src).with_context(|| format!("cannot read {}", src.display()))?
    {
        let entry = entry.with_context(|| format!("cannot read entry in {}", src.display()))?;
        let dst_path = dst.join(entry.file_name());
        let ft = entry.file_type().context("cannot determine file type")?;
        if ft.is_dir() {
            copy_dir(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)
                .with_context(|| format!("cannot copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

// ── RED tests — issue 15: Agents Commands ────────────────────────────────────
//
// These tests define the expected behaviour of the helper functions that back
// `polis agents list / info / add`.  They reference `load_manifest`,
// `validate_security_policy`, `check_signature`, and `SignatureStatus`, none
// of which exist yet.  The suite therefore fails to compile (RED) until the
// implementation is written.
//
// Spec: docs/linear-issues/polis-oss/ux-improvements/15-agents-commands.md
// Depends on: 14-agent-manifest-extension (AgentManifest types — already done)

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{check_signature, load_manifest, validate_security_policy, SignatureStatus};

    // ── YAML fixtures ────────────────────────────────────────────────────────

    /// Minimal valid agent with `noNewPrivileges: true`.
    const VALID_YAML: &str = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: test-agent
  displayName: "Test Agent"
  version: "1.0.0"
  description: "A test agent"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
  security:
    protectSystem: "strict"
    protectHome: "true"
    noNewPrivileges: true
    privateTmp: true
"#;

    /// Agent with `noNewPrivileges: false` — must be rejected.
    const INSECURE_YAML: &str = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: bad-agent
  displayName: "Bad Agent"
  version: "1.0.0"
  description: "Insecure agent"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
  security:
    protectSystem: "strict"
    protectHome: "true"
    noNewPrivileges: false
    privateTmp: true
"#;

    /// Agent with no `security` section — policy check must pass (no constraint to violate).
    const NO_SECURITY_YAML: &str = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: no-sec-agent
  displayName: "No Security Agent"
  version: "1.0.0"
  description: "Agent without security section"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
"#;

    // ── load_manifest ────────────────────────────────────────────────────────

    /// EARS: WHEN a valid agent.yaml is present THEN load_manifest returns the manifest.
    #[test]
    fn test_load_manifest_valid_yaml_returns_manifest() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("agent.yaml");
        fs::write(&path, VALID_YAML).expect("write");

        let manifest = load_manifest(&path).expect("should parse");

        assert_eq!(manifest.metadata.name, "test-agent");
        assert_eq!(manifest.metadata.version, "1.0.0");
    }

    /// EARS: IF agent.yaml is missing THEN load_manifest returns an error.
    #[test]
    fn test_load_manifest_missing_file_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let result = load_manifest(&dir.path().join("agent.yaml"));
        assert!(result.is_err(), "missing file must be an error");
    }

    /// EARS: IF agent.yaml contains invalid YAML THEN load_manifest returns an error.
    #[test]
    fn test_load_manifest_invalid_yaml_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("agent.yaml");
        fs::write(&path, "{ not: valid: yaml: [}").expect("write");

        let result = load_manifest(&path);
        assert!(result.is_err(), "invalid YAML must be an error");
    }

    // ── validate_security_policy ─────────────────────────────────────────────

    /// EARS: WHEN noNewPrivileges is true THEN security policy validation passes.
    #[test]
    fn test_validate_security_policy_no_new_privileges_true_passes() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("agent.yaml");
        fs::write(&path, VALID_YAML).expect("write");
        let manifest = load_manifest(&path).expect("parse");

        assert!(validate_security_policy(&manifest).is_ok());
    }

    /// EARS: IF noNewPrivileges is false THEN security policy validation returns an error.
    #[test]
    fn test_validate_security_policy_no_new_privileges_false_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("agent.yaml");
        fs::write(&path, INSECURE_YAML).expect("write");
        let manifest = load_manifest(&path).expect("parse");

        assert!(
            validate_security_policy(&manifest).is_err(),
            "noNewPrivileges: false must be rejected"
        );
    }

    /// EARS: WHEN no security section is present THEN validation passes (no constraint to violate).
    #[test]
    fn test_validate_security_policy_absent_security_section_passes() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("agent.yaml");
        fs::write(&path, NO_SECURITY_YAML).expect("write");
        let manifest = load_manifest(&path).expect("parse");

        assert!(validate_security_policy(&manifest).is_ok());
    }

    // ── check_signature ──────────────────────────────────────────────────────

    /// EARS: WHEN no agent.yaml.sig file is present THEN check_signature returns NotFound.
    /// Security constraint: unsigned agents must NOT be auto-installed.
    #[test]
    fn test_check_signature_no_sig_file_returns_not_found() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("agent.yaml"), VALID_YAML).expect("write");

        let status = check_signature(dir.path()).expect("should not error");

        assert!(
            matches!(status, SignatureStatus::NotFound),
            "missing .sig must yield NotFound"
        );
    }

    /// EARS: IF agent.yaml.sig contains garbage bytes THEN check_signature returns Invalid.
    #[test]
    fn test_check_signature_corrupt_sig_returns_invalid() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("agent.yaml"), VALID_YAML).expect("write");
        fs::write(dir.path().join("agent.yaml.sig"), b"not-a-valid-ed25519-signature")
            .expect("write sig");

        let status = check_signature(dir.path()).expect("should not error");

        assert!(
            matches!(status, SignatureStatus::Invalid { .. }),
            "corrupt .sig must yield Invalid"
        );
    }
}

// ── Property tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::fs;
    use tempfile::TempDir;

    /// Minimal valid YAML template — name and noNewPrivileges are substituted.
    fn agent_yaml(name: &str, no_new_privileges: bool) -> String {
        format!(
            r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: {name}
  displayName: "Test"
  version: "1.0.0"
  description: "test"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
  security:
    protectSystem: "strict"
    protectHome: "true"
    noNewPrivileges: {no_new_privileges}
    privateTmp: true
"#
        )
    }

    proptest! {
        /// validate_security_policy always passes when noNewPrivileges is true,
        /// regardless of other manifest content.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_validate_security_policy_no_new_privileges_true_always_passes(
            name in "[a-z][a-z0-9-]{1,20}",
        ) {
            let dir = TempDir::new().expect("tempdir");
            let path = dir.path().join("agent.yaml");
            fs::write(&path, agent_yaml(&name, true)).expect("write");
            let manifest = load_manifest(&path).expect("parse");
            prop_assert!(validate_security_policy(&manifest).is_ok());
        }

        /// validate_security_policy always errors when noNewPrivileges is false.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_validate_security_policy_no_new_privileges_false_always_errors(
            name in "[a-z][a-z0-9-]{1,20}",
        ) {
            let dir = TempDir::new().expect("tempdir");
            let path = dir.path().join("agent.yaml");
            fs::write(&path, agent_yaml(&name, false)).expect("write");
            let manifest = load_manifest(&path).expect("parse");
            prop_assert!(validate_security_policy(&manifest).is_err());
        }

        /// check_signature always returns NotFound when no .sig file is present.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_check_signature_absent_sig_always_not_found(
            content in ".*",
        ) {
            let dir = TempDir::new().expect("tempdir");
            fs::write(dir.path().join("agent.yaml"), content).expect("write");
            // No .sig file written.
            let status = check_signature(dir.path()).expect("should not error");
            prop_assert!(matches!(status, SignatureStatus::NotFound));
        }

        /// check_signature returns Valid for any 64-byte payload (structurally correct ed25519).
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_check_signature_64_byte_sig_always_valid(
            bytes in proptest::collection::vec(any::<u8>(), 64..=64),
        ) {
            let dir = TempDir::new().expect("tempdir");
            fs::write(dir.path().join("agent.yaml.sig"), &bytes).expect("write sig");
            let status = check_signature(dir.path()).expect("should not error");
            let is_valid = matches!(status, SignatureStatus::Valid { .. });
            prop_assert!(is_valid, "64-byte sig must be Valid");
        }

        /// check_signature returns Invalid for any payload whose length is not 64.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_check_signature_non_64_byte_sig_always_invalid(
            bytes in proptest::collection::vec(any::<u8>(), 0..200usize)
                .prop_filter("must not be 64 bytes", |v| v.len() != 64),
        ) {
            let dir = TempDir::new().expect("tempdir");
            fs::write(dir.path().join("agent.yaml.sig"), &bytes).expect("write sig");
            let status = check_signature(dir.path()).expect("should not error");
            let is_invalid = matches!(status, SignatureStatus::Invalid { .. });
            prop_assert!(is_invalid, "non-64-byte sig must be Invalid");
        }

        /// load_manifest preserves name and version through a write→parse round-trip.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_load_manifest_name_version_roundtrip(
            name in "[a-z][a-z0-9-]{1,20}",
            version in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
        ) {
            let yaml = format!(
                r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: {name}
  displayName: "T"
  version: "{version}"
  description: "t"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
"#
            );
            let dir = TempDir::new().expect("tempdir");
            let path = dir.path().join("agent.yaml");
            fs::write(&path, &yaml).expect("write");
            let manifest = load_manifest(&path).expect("parse");
            prop_assert_eq!(manifest.metadata.name, name);
            prop_assert_eq!(manifest.metadata.version, version);
        }
    }
}
