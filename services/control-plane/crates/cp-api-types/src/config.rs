use serde::{Deserialize, Serialize};

use crate::RuleItem;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityOverview {
    pub level: String,
    pub protected_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigAgentResponse {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigResponse {
    pub security: SecurityOverview,
    pub auto_approve_rules: Vec<RuleItem>,
    pub bypass_domains_count: usize,
    pub agent: ConfigAgentResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityConfigResponse {
    pub level: String,
    pub auto_approve_rules: Vec<RuleItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BypassListResponse {
    pub domains: Vec<String>,
    pub total: usize,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BypassAddRequest {
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialAllowItem {
    pub pattern: String,
    pub host: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialAllowsResponse {
    pub items: Vec<CredentialAllowItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde::{Serialize, de::DeserializeOwned};

    use super::*;

    fn assert_json_roundtrip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let restored: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&restored, value);
    }

    #[test]
    fn config_response_roundtrip() {
        assert_json_roundtrip(&ConfigResponse {
            security: SecurityOverview {
                level: "balanced".to_string(),
                protected_paths: vec!["~/.ssh".to_string(), "~/.aws".to_string()],
            },
            auto_approve_rules: vec![RuleItem {
                pattern: "*.example.com".to_string(),
                action: "allow".to_string(),
            }],
            bypass_domains_count: 142,
            agent: ConfigAgentResponse {
                name: "openclaw".to_string(),
                version: "1.0.0".to_string(),
            },
        });
    }

    #[test]
    fn security_config_response_roundtrip() {
        assert_json_roundtrip(&SecurityConfigResponse {
            level: "strict".to_string(),
            auto_approve_rules: vec![RuleItem {
                pattern: "github.com".to_string(),
                action: "allow".to_string(),
            }],
        });
    }

    #[test]
    fn bypass_types_roundtrip() {
        assert_json_roundtrip(&BypassListResponse {
            domains: vec!["registry.npmjs.org".to_string(), "pypi.org".to_string()],
            total: 2,
            source: "runtime".to_string(),
        });
        assert_json_roundtrip(&BypassAddRequest {
            domain: "internal.corp.com".to_string(),
        });
    }

    #[test]
    fn credential_allow_types_roundtrip() {
        assert_json_roundtrip(&CredentialAllowsResponse {
            items: vec![CredentialAllowItem {
                pattern: "aws_access".to_string(),
                host: "generativelanguage.googleapis.com".to_string(),
                fingerprint: "0123456789abcdef".to_string(),
            }],
        });
    }

    #[test]
    fn config_event_roundtrip() {
        assert_json_roundtrip(&ConfigEvent {
            event_type: "bypass_added".to_string(),
            level: None,
            domain: Some("internal.corp.com".to_string()),
        });
    }
}
