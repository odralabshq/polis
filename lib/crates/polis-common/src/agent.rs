// lib/crates/polis-common/src/agent.rs

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Agent manifest (`agent.yaml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: AgentMetadata,
    pub spec: AgentSpec,
}

/// Metadata section of an agent manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// Explicit provider name (e.g. `"Anthropic"`). Derived from
    /// `requirements.envOneOf` when absent.
    #[serde(default)]
    pub provider: Option<String>,
    /// User-facing capability tags (e.g. `["code-generation"]`).
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl AgentMetadata {
    /// Returns the provider name, falling back to derivation from
    /// `requirements.envOneOf` when `provider` is not set.
    ///
    /// Returns `"Unknown"` when neither source yields a known provider.
    #[must_use]
    pub fn effective_provider(&self, requirements: Option<&AgentRequirements>) -> String {
        if let Some(p) = &self.provider
            && !p.is_empty()
        {
            return p.clone();
        }
        if let Some(reqs) = requirements {
            for env in &reqs.env_one_of {
                match env.as_str() {
                    "ANTHROPIC_API_KEY" => return "Anthropic".to_string(),
                    "OPENAI_API_KEY" => return "OpenAI".to_string(),
                    "OPENROUTER_API_KEY" => return "OpenRouter".to_string(),
                    _ => {}
                }
            }
        }
        "Unknown".to_string()
    }
}

/// Spec section of an agent manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub packaging: String,
    pub install: String,
    pub runtime: AgentRuntime,
    #[serde(default)]
    pub init: Option<String>,
    #[serde(default)]
    pub health: Option<AgentHealth>,
    #[serde(default)]
    pub security: Option<AgentSecurity>,
    #[serde(default)]
    pub ports: Vec<AgentPort>,
    #[serde(default)]
    pub resources: Option<AgentResources>,
    #[serde(default)]
    pub requirements: Option<AgentRequirements>,
    #[serde(default)]
    pub persistence: Vec<AgentPersistence>,
    #[serde(default)]
    pub capabilities: Option<AgentCapabilities>,
    #[serde(default)]
    pub commands: Option<String>,
}

/// Runtime configuration for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRuntime {
    pub command: String,
    pub workdir: String,
    pub user: String,
    #[serde(rename = "envFile", default)]
    pub env_file: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Health-check configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHealth {
    pub command: String,
    pub interval: String,
    pub timeout: String,
    pub retries: u32,
    #[serde(rename = "startPeriod")]
    pub start_period: String,
}

/// Systemd-style security constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSecurity {
    #[serde(rename = "protectSystem")]
    pub protect_system: String,
    #[serde(rename = "protectHome")]
    pub protect_home: String,
    #[serde(rename = "readWritePaths", default)]
    pub read_write_paths: Vec<String>,
    #[serde(rename = "noNewPrivileges")]
    pub no_new_privileges: bool,
    #[serde(rename = "privateTmp")]
    pub private_tmp: bool,
    #[serde(rename = "memoryMax", default)]
    pub memory_max: Option<String>,
    #[serde(rename = "cpuQuota", default)]
    pub cpu_quota: Option<String>,
}

/// Port mapping for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPort {
    pub container: u16,
    #[serde(rename = "hostEnv")]
    pub host_env: String,
    pub default: u16,
}

/// Resource limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResources {
    #[serde(rename = "memoryLimit")]
    pub memory_limit: String,
    #[serde(rename = "memoryReservation")]
    pub memory_reservation: String,
}

/// Environment variable requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequirements {
    #[serde(rename = "envOneOf", default)]
    pub env_one_of: Vec<String>,
    #[serde(rename = "envOptional", default)]
    pub env_optional: Vec<String>,
}

/// Named persistent volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPersistence {
    pub name: String,
    #[serde(rename = "containerPath")]
    pub container_path: String,
}

/// Runtime capability flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub network: bool,
    #[serde(default)]
    pub filesystem: Vec<String>,
    pub mcp: bool,
    #[serde(rename = "dockerInDocker")]
    pub docker_in_docker: bool,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // ── YAML fixtures ────────────────────────────────────────────────────────

    /// Full manifest with every new field present.
    const FULL_MANIFEST_YAML: &str = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: claude-dev
  displayName: "Claude Dev"
  version: "1.0.0"
  description: "Claude AI coding assistant"
  author: "anthropic"
  license: "MIT"
  provider: "Anthropic"
  capabilities:
    - code-generation
    - code-review
    - documentation
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/usr/bin/node dist/index.js"
    workdir: /app
    user: polis
"#;

    /// Existing openclaw manifest — no provider/capabilities in metadata.
    const OPENCLAW_YAML: &str = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: openclaw
  displayName: "OpenClaw"
  version: "1.0.0"
  description: "AI coding agent with Control UI"
  author: "openclaw"
  license: "MIT"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/usr/bin/node dist/index.js gateway --allow-unconfigured --bind lan --port 18789"
    workdir: /app
    user: polis
  requirements:
    envOneOf:
      - ANTHROPIC_API_KEY
      - OPENAI_API_KEY
      - OPENROUTER_API_KEY
"#;

    /// Minimal template manifest — only required fields.
    const TEMPLATE_YAML: &str = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: my-agent
  displayName: "My Agent"
  version: "0.1.0"
  description: "A minimal agent"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo hello"
    workdir: /opt/agents/my-agent
    user: polis
"#;

    // ── Parsing: happy path ──────────────────────────────────────────────────

    #[test]
    fn test_agent_manifest_full_yaml_parses_all_fields() {
        let manifest: AgentManifest =
            serde_yaml::from_str(FULL_MANIFEST_YAML).expect("full manifest should parse");

        assert_eq!(manifest.api_version, "polis.dev/v1");
        assert_eq!(manifest.kind, "AgentPlugin");
        assert_eq!(manifest.metadata.name, "claude-dev");
        assert_eq!(manifest.metadata.display_name, "Claude Dev");
        assert_eq!(manifest.metadata.version, "1.0.0");
        assert_eq!(manifest.metadata.provider.as_deref(), Some("Anthropic"));
        assert_eq!(
            manifest.metadata.capabilities,
            vec!["code-generation", "code-review", "documentation"]
        );
    }

    #[test]
    fn test_agent_manifest_openclaw_yaml_parses_successfully() {
        let manifest: AgentManifest =
            serde_yaml::from_str(OPENCLAW_YAML).expect("openclaw manifest should parse");

        assert_eq!(manifest.metadata.name, "openclaw");
        assert_eq!(manifest.metadata.provider, None);
        assert!(manifest.metadata.capabilities.is_empty());
    }

    #[test]
    fn test_agent_manifest_template_yaml_parses_successfully() {
        let manifest: AgentManifest =
            serde_yaml::from_str(TEMPLATE_YAML).expect("template manifest should parse");

        assert_eq!(manifest.metadata.name, "my-agent");
        assert_eq!(manifest.metadata.author, None);
        assert_eq!(manifest.metadata.license, None);
        assert_eq!(manifest.metadata.provider, None);
        assert!(manifest.metadata.capabilities.is_empty());
    }

    // ── Parsing: optional fields default correctly ───────────────────────────

    #[test]
    fn test_agent_metadata_provider_absent_defaults_to_none() {
        let manifest: AgentManifest = serde_yaml::from_str(TEMPLATE_YAML).expect("should parse");
        assert_eq!(manifest.metadata.provider, None);
    }

    #[test]
    fn test_agent_metadata_capabilities_absent_defaults_to_empty_vec() {
        let manifest: AgentManifest = serde_yaml::from_str(TEMPLATE_YAML).expect("should parse");
        assert!(manifest.metadata.capabilities.is_empty());
    }

    // ── Parsing: error paths ─────────────────────────────────────────────────

    #[test]
    fn test_agent_manifest_missing_required_name_returns_error() {
        let yaml = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  displayName: "No Name"
  version: "1.0.0"
  description: "Missing name field"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
"#;
        let result: Result<AgentManifest, _> = serde_yaml::from_str(yaml);
        assert!(
            result.is_err(),
            "manifest without 'name' should fail to parse"
        );
    }

    #[test]
    fn test_agent_manifest_invalid_yaml_returns_error() {
        let result: Result<AgentManifest, _> = serde_yaml::from_str("{ not: valid: yaml: [}");
        assert!(result.is_err(), "invalid YAML should return an error");
    }

    // ── effective_provider ───────────────────────────────────────────────────

    #[test]
    fn test_effective_provider_explicit_provider_returns_it() {
        let manifest: AgentManifest =
            serde_yaml::from_str(FULL_MANIFEST_YAML).expect("should parse");
        let provider = manifest
            .metadata
            .effective_provider(manifest.spec.requirements.as_ref());
        assert_eq!(provider, "Anthropic");
    }

    #[test]
    fn test_effective_provider_anthropic_key_derives_anthropic() {
        let manifest: AgentManifest = serde_yaml::from_str(OPENCLAW_YAML).expect("should parse");
        // openclaw has ANTHROPIC_API_KEY first in envOneOf
        let provider = manifest
            .metadata
            .effective_provider(manifest.spec.requirements.as_ref());
        assert_eq!(provider, "Anthropic");
    }

    #[test]
    fn test_effective_provider_openai_key_derives_openai() {
        let yaml = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: gpt-agent
  displayName: "GPT Agent"
  version: "1.0.0"
  description: "OpenAI agent"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
  requirements:
    envOneOf:
      - OPENAI_API_KEY
"#;
        let manifest: AgentManifest = serde_yaml::from_str(yaml).expect("should parse");
        let provider = manifest
            .metadata
            .effective_provider(manifest.spec.requirements.as_ref());
        assert_eq!(provider, "OpenAI");
    }

    #[test]
    fn test_effective_provider_openrouter_key_derives_openrouter() {
        let yaml = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: router-agent
  displayName: "Router Agent"
  version: "1.0.0"
  description: "OpenRouter agent"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
  requirements:
    envOneOf:
      - OPENROUTER_API_KEY
"#;
        let manifest: AgentManifest = serde_yaml::from_str(yaml).expect("should parse");
        let provider = manifest
            .metadata
            .effective_provider(manifest.spec.requirements.as_ref());
        assert_eq!(provider, "OpenRouter");
    }

    #[test]
    fn test_effective_provider_no_provider_no_matching_env_returns_unknown() {
        let yaml = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: custom-agent
  displayName: "Custom Agent"
  version: "1.0.0"
  description: "Custom agent with unknown key"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
  requirements:
    envOneOf:
      - CUSTOM_API_KEY
"#;
        let manifest: AgentManifest = serde_yaml::from_str(yaml).expect("should parse");
        let provider = manifest
            .metadata
            .effective_provider(manifest.spec.requirements.as_ref());
        assert_eq!(provider, "Unknown");
    }

    #[test]
    fn test_effective_provider_no_requirements_returns_unknown() {
        let manifest: AgentManifest = serde_yaml::from_str(TEMPLATE_YAML).expect("should parse");
        let provider = manifest.metadata.effective_provider(None);
        assert_eq!(provider, "Unknown");
    }

    #[test]
    fn test_effective_provider_explicit_takes_precedence_over_env() {
        let yaml = r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: explicit-agent
  displayName: "Explicit Agent"
  version: "1.0.0"
  description: "Explicit provider wins"
  provider: "CustomCorp"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/bin/echo"
    workdir: /tmp
    user: polis
  requirements:
    envOneOf:
      - ANTHROPIC_API_KEY
"#;
        let manifest: AgentManifest = serde_yaml::from_str(yaml).expect("should parse");
        let provider = manifest
            .metadata
            .effective_provider(manifest.spec.requirements.as_ref());
        assert_eq!(provider, "CustomCorp");
    }

    // ── Property tests ───────────────────────────────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// effective_provider never panics for any string inputs.
        #[test]
        fn prop_effective_provider_never_panics(
            provider in proptest::option::of("[\\PC]{0,50}"),
            env_key in "[A-Z_]{1,30}",
        ) {
            let reqs = AgentRequirements {
                env_one_of: vec![env_key],
                env_optional: vec![],
            };
            let meta = AgentMetadata {
                name: "test".to_string(),
                display_name: "Test".to_string(),
                version: "0.1.0".to_string(),
                description: "test".to_string(),
                author: None,
                license: None,
                provider,
                capabilities: vec![],
            };
            // Must not panic — result is either a string or "Unknown"
            let result = meta.effective_provider(Some(&reqs));
            prop_assert!(!result.is_empty());
        }

        /// An explicit non-empty provider always takes precedence over envOneOf derivation.
        #[test]
        fn prop_effective_provider_explicit_nonempty_always_wins(
            provider in "[\\PC]{1,50}",
            env_keys in proptest::collection::vec("[A-Z_]{1,30}", 0usize..5),
        ) {
            let reqs = AgentRequirements { env_one_of: env_keys, env_optional: vec![] };
            let meta = AgentMetadata {
                name: "t".to_string(),
                display_name: "T".to_string(),
                version: "0.1.0".to_string(),
                description: "t".to_string(),
                author: None,
                license: None,
                provider: Some(provider.clone()),
                capabilities: vec![],
            };
            prop_assert_eq!(meta.effective_provider(Some(&reqs)), provider);
        }

        /// provider and capabilities survive a JSON serde roundtrip.
        #[test]
        fn prop_agent_metadata_new_fields_serde_roundtrip(
            provider in proptest::option::of("[\\PC]{1,50}"),
            capabilities in proptest::collection::vec("[a-z-]{1,20}", 0usize..5),
        ) {
            let meta = AgentMetadata {
                name: "t".to_string(),
                display_name: "T".to_string(),
                version: "0.1.0".to_string(),
                description: "t".to_string(),
                author: None,
                license: None,
                provider: provider.clone(),
                capabilities: capabilities.clone(),
            };
            let json = serde_json::to_string(&meta).expect("serialize");
            let back: AgentMetadata = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(back.provider, provider);
            prop_assert_eq!(back.capabilities, capabilities);
        }
    }
}
