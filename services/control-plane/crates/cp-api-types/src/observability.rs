use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogLine {
    pub timestamp: DateTime<Utc>,
    pub service: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogsResponse {
    pub lines: Vec<LogLine>,
    pub total: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerMetrics {
    pub service: String,
    pub status: String,
    pub health: String,
    pub memory_usage_mb: u64,
    pub memory_limit_mb: u64,
    pub cpu_percent: f64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub pids: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemMetrics {
    pub total_memory_usage_mb: u64,
    pub total_memory_limit_mb: u64,
    pub total_cpu_percent: f64,
    pub container_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsResponse {
    pub timestamp: DateTime<Utc>,
    pub system: SystemMetrics,
    pub containers: Vec<ContainerMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsPoint {
    pub timestamp: DateTime<Utc>,
    pub total_memory_usage_mb: u64,
    pub total_cpu_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsHistoryResponse {
    pub interval_seconds: u32,
    pub points: Vec<MetricsPoint>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde::{Serialize, de::DeserializeOwned};

    use super::*;

    fn sample_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 5, 19, 5, 32)
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
    fn logs_response_roundtrip() {
        assert_json_roundtrip(&LogsResponse {
            lines: vec![LogLine {
                timestamp: sample_time(),
                service: "sentinel".to_string(),
                level: "info".to_string(),
                message: "Blocked credential in request".to_string(),
            }],
            total: 1,
            truncated: false,
        });
    }

    #[test]
    fn metrics_response_roundtrip() {
        assert_json_roundtrip(&MetricsResponse {
            timestamp: sample_time(),
            system: SystemMetrics {
                total_memory_usage_mb: 1_280,
                total_memory_limit_mb: 8_192,
                total_cpu_percent: 15.2,
                container_count: 11,
            },
            containers: vec![ContainerMetrics {
                service: "workspace".to_string(),
                status: "running".to_string(),
                health: "healthy".to_string(),
                memory_usage_mb: 512,
                memory_limit_mb: 4_096,
                cpu_percent: 8.1,
                network_rx_bytes: 1_048_576,
                network_tx_bytes: 524_288,
                pids: 42,
                stale: false,
            }],
        });
    }

    #[test]
    fn metrics_history_roundtrip() {
        assert_json_roundtrip(&MetricsHistoryResponse {
            interval_seconds: 10,
            points: vec![MetricsPoint {
                timestamp: sample_time(),
                total_memory_usage_mb: 1_280,
                total_cpu_percent: 15.2,
            }],
        });
    }
}
