//! Shared request/response types for the Polis control plane API.

#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

mod config;
mod observability;
mod workspace;

pub use config::{
    BypassAddRequest, BypassListResponse, ConfigAgentResponse, ConfigEvent, ConfigResponse,
    SecurityConfigResponse, SecurityOverview,
};
pub use observability::{
    ContainerMetrics, LogLine, LogsResponse, MetricsHistoryResponse, MetricsPoint, MetricsResponse,
    SystemMetrics,
};
pub use workspace::{
    AgentResponse, ContainerInfo, ContainerSummary, ContainersResponse, PortMapping, ResourceUsage,
    WorkspaceResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusResponse {
    pub security_level: String,
    pub pending_count: usize,
    pub recent_approvals: usize,
    pub events_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockedItem {
    pub request_id: String,
    pub reason: String,
    pub destination: String,
    pub blocked_at: DateTime<Utc>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockedListResponse {
    pub items: Vec<BlockedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventItem {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventsResponse {
    pub events: Vec<EventItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LevelRequest {
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LevelResponse {
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleItem {
    pub pattern: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RulesResponse {
    pub rules: Vec<RuleItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleCreateRequest {
    pub pattern: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorResponse {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;
    use serde::de::DeserializeOwned;

    use super::*;

    fn sample_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 5, 19, 0, 0)
            .single()
            .expect("valid timestamp")
    }

    fn assert_json_roundtrip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let restored: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&restored, value);
    }

    #[test]
    fn status_response_roundtrip() {
        assert_json_roundtrip(&StatusResponse {
            security_level: "balanced".to_string(),
            pending_count: 3,
            recent_approvals: 1,
            events_count: 42,
        });
    }

    #[test]
    fn blocked_item_roundtrip() {
        assert_json_roundtrip(&BlockedItem {
            request_id: "req-abc12345".to_string(),
            reason: "credential_detected".to_string(),
            destination: "https://example.com/api".to_string(),
            blocked_at: sample_time(),
            status: "pending".to_string(),
        });
    }

    #[test]
    fn blocked_list_response_roundtrip() {
        assert_json_roundtrip(&BlockedListResponse {
            items: vec![BlockedItem {
                request_id: "req-abc12345".to_string(),
                reason: "credential_detected".to_string(),
                destination: "https://example.com/api".to_string(),
                blocked_at: sample_time(),
                status: "pending".to_string(),
            }],
        });
    }

    #[test]
    fn event_item_roundtrip() {
        assert_json_roundtrip(&EventItem {
            timestamp: sample_time(),
            event_type: "block_reported".to_string(),
            request_id: Some("req-abc12345".to_string()),
            details: "Blocked request to example.com".to_string(),
        });
    }

    #[test]
    fn events_response_roundtrip() {
        assert_json_roundtrip(&EventsResponse {
            events: vec![EventItem {
                timestamp: sample_time(),
                event_type: "block_reported".to_string(),
                request_id: Some("req-abc12345".to_string()),
                details: "Blocked request to example.com".to_string(),
            }],
        });
    }

    #[test]
    fn level_request_roundtrip() {
        assert_json_roundtrip(&LevelRequest {
            level: "strict".to_string(),
        });
    }

    #[test]
    fn level_response_roundtrip() {
        assert_json_roundtrip(&LevelResponse {
            level: "balanced".to_string(),
        });
    }

    #[test]
    fn rule_item_roundtrip() {
        assert_json_roundtrip(&RuleItem {
            pattern: "*.example.com".to_string(),
            action: "allow".to_string(),
        });
    }

    #[test]
    fn rules_response_roundtrip() {
        assert_json_roundtrip(&RulesResponse {
            rules: vec![RuleItem {
                pattern: "*.example.com".to_string(),
                action: "allow".to_string(),
            }],
        });
    }

    #[test]
    fn rule_create_request_roundtrip() {
        assert_json_roundtrip(&RuleCreateRequest {
            pattern: "*.example.com".to_string(),
            action: "allow".to_string(),
        });
    }

    #[test]
    fn action_response_roundtrip() {
        assert_json_roundtrip(&ActionResponse {
            message: "approved req-abc12345".to_string(),
        });
    }

    #[test]
    fn error_response_roundtrip() {
        assert_json_roundtrip(&ErrorResponse {
            error: "no blocked request found".to_string(),
        });
    }

    prop_compose! {
        fn request_id_strategy()(suffix in "[a-f0-9]{8}") -> String {
            format!("req-{suffix}")
        }
    }

    prop_compose! {
        fn blocked_item_strategy()(
            request_id in request_id_strategy(),
            reason in "[a-z_]{3,24}",
            destination in "[a-zA-Z0-9:/._-]{3,40}",
            status in "(pending|approved|denied)"
        ) -> BlockedItem {
            BlockedItem {
                request_id,
                reason,
                destination,
                blocked_at: sample_time(),
                status,
            }
        }
    }

    proptest! {
        #[test]
        fn proptest_status_response_roundtrip(
            security_level in "(relaxed|balanced|strict)",
            pending_count in 0usize..10_000,
            recent_approvals in 0usize..10_000,
            events_count in 0usize..10_000,
        ) {
            let value = StatusResponse {
                security_level,
                pending_count,
                recent_approvals,
                events_count,
            };

            let json = serde_json::to_string(&value)?;
            let restored: StatusResponse = serde_json::from_str(&json)?;
            prop_assert_eq!(restored, value);
        }

        #[test]
        fn proptest_blocked_item_roundtrip(value in blocked_item_strategy()) {
            let json = serde_json::to_string(&value)?;
            let restored: BlockedItem = serde_json::from_str(&json)?;
            prop_assert_eq!(restored, value);
        }
    }
}
