use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceResponse {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,
    pub containers: ContainerSummary,
    pub networks: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerSummary {
    pub total: usize,
    pub healthy: usize,
    pub unhealthy: usize,
    pub starting: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentResponse {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub status: String,
    pub health: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,
    pub ports: Vec<PortMapping>,
    pub resources: ResourceUsage,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortMapping {
    pub container: u16,
    pub host: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceUsage {
    pub memory_usage_mb: u64,
    pub memory_limit_mb: u64,
    pub cpu_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerInfo {
    pub name: String,
    pub service: String,
    pub status: String,
    pub health: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,
    pub memory_usage_mb: u64,
    pub memory_limit_mb: u64,
    pub cpu_percent: f64,
    pub network: String,
    pub ip: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainersResponse {
    pub containers: Vec<ContainerInfo>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use proptest::prelude::*;
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
    fn workspace_response_roundtrip() {
        assert_json_roundtrip(&WorkspaceResponse {
            status: "running".to_string(),
            uptime_seconds: Some(3_600),
            containers: ContainerSummary {
                total: 11,
                healthy: 10,
                unhealthy: 1,
                starting: 0,
            },
            networks: HashMap::from([
                ("internal_bridge".to_string(), "10.10.1.0/24".to_string()),
                ("gateway_bridge".to_string(), "10.30.1.0/24".to_string()),
            ]),
        });
    }

    #[test]
    fn agent_response_roundtrip() {
        assert_json_roundtrip(&AgentResponse {
            name: "openclaw".to_string(),
            display_name: "OpenClaw".to_string(),
            version: "1.0.0".to_string(),
            status: "running".to_string(),
            health: "healthy".to_string(),
            uptime_seconds: Some(3_540),
            ports: vec![PortMapping {
                container: 18_789,
                host: 18_789,
                protocol: "tcp".to_string(),
            }],
            resources: ResourceUsage {
                memory_usage_mb: 512,
                memory_limit_mb: 6_144,
                cpu_percent: 12.5,
            },
            stale: false,
        });
    }

    #[test]
    fn container_info_roundtrip() {
        assert_json_roundtrip(&ContainerInfo {
            name: "polis-gate".to_string(),
            service: "gate".to_string(),
            status: "running".to_string(),
            health: "healthy".to_string(),
            uptime_seconds: Some(3_600),
            memory_usage_mb: 45,
            memory_limit_mb: 256,
            cpu_percent: 2.1,
            network: "gateway-bridge".to_string(),
            ip: "10.30.1.6".to_string(),
            stale: false,
        });
    }

    #[test]
    fn containers_response_roundtrip() {
        assert_json_roundtrip(&ContainersResponse {
            containers: vec![ContainerInfo {
                name: "polis-workspace".to_string(),
                service: "workspace".to_string(),
                status: "running".to_string(),
                health: "healthy".to_string(),
                uptime_seconds: Some(3_600),
                memory_usage_mb: 512,
                memory_limit_mb: 4_096,
                cpu_percent: 8.1,
                network: "internal-bridge".to_string(),
                ip: "10.10.1.10".to_string(),
                stale: true,
            }],
        });
    }

    #[test]
    fn stale_fields_are_omitted_when_false() {
        let agent = AgentResponse {
            name: "openclaw".to_string(),
            display_name: "OpenClaw".to_string(),
            version: "1.0.0".to_string(),
            status: "running".to_string(),
            health: "healthy".to_string(),
            uptime_seconds: Some(42),
            ports: Vec::new(),
            resources: ResourceUsage {
                memory_usage_mb: 10,
                memory_limit_mb: 20,
                cpu_percent: 1.0,
            },
            stale: false,
        };
        let json = serde_json::to_string(&agent).expect("serialize");
        assert!(!json.contains("stale"));
    }

    fn cpu_percent_roundtrips_close_enough(left: f64, right: f64) -> bool {
        let diff = (left - right).abs();
        diff <= 1e-12 || diff <= left.abs().max(right.abs()) * 1e-12
    }

    proptest! {
        #[test]
        fn proptest_resource_usage_roundtrip(
            memory_usage_mb in 0_u64..10_000,
            memory_limit_mb in 1_u64..20_000,
            cpu_percent in 0.0_f64..500.0_f64,
        ) {
            let value = ResourceUsage {
                memory_usage_mb,
                memory_limit_mb,
                cpu_percent,
            };

            let json = serde_json::to_string(&value)?;
            let restored: ResourceUsage = serde_json::from_str(&json)?;
            prop_assert_eq!(restored.memory_usage_mb, value.memory_usage_mb);
            prop_assert_eq!(restored.memory_limit_mb, value.memory_limit_mb);
            prop_assert!(
                cpu_percent_roundtrips_close_enough(restored.cpu_percent, value.cpu_percent),
                "cpu_percent changed too much after roundtrip: {} vs {}",
                restored.cpu_percent,
                value.cpu_percent
            );
        }
    }
}
