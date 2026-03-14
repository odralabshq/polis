use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use bollard::{
    Docker,
    container::{ListContainersOptions, LogOutput, LogsOptions, Stats, StatsOptions},
    errors::Error as BollardError,
    models::{ContainerInspectResponse, ContainerSummary, Network, NetworkSettings, Port},
    network::ListNetworksOptions,
};
use chrono::{DateTime, Utc};
use cp_api_types::{
    AgentResponse, ContainerInfo, ContainerMetrics as ApiContainerMetrics,
    ContainerSummary as ApiContainerSummary, LogLine, LogsResponse, MetricsHistoryResponse,
    MetricsPoint, MetricsResponse, PortMapping, ResourceUsage, SystemMetrics as ApiSystemMetrics,
    WorkspaceResponse,
};
use futures::{StreamExt, future::join_all};
use tokio::{
    sync::RwLock,
    time::{Instant, timeout},
};

use crate::error::{AppError, AppResult};

const METRICS_HISTORY_LIMIT: usize = 360;
pub const METRICS_INTERVAL_SECONDS: u64 = 10;
const CONTAINER_STATS_TIMEOUT: Duration = Duration::from_secs(3);
const PROJECT_LABEL: &str = "com.docker.compose.project=polis";
const UNKNOWN_VALUE: &str = "unknown";
const NO_ACTIVE_AGENT_MESSAGE: &str = "no active agent detected";
const BYTES_PER_MIB: u64 = 1_048_576;

#[derive(Clone)]
pub struct DockerClient {
    client: Arc<Docker>,
    stats_cache: Arc<RwLock<HashMap<String, CachedStats>>>,
}

#[derive(Clone, Debug)]
struct CachedStats {
    snapshot: ContainerStatsSnapshot,
    captured_at: Instant,
}

#[derive(Clone, Debug)]
struct ContainerStatsSnapshot {
    resources: ResourceUsage,
    network_rx_bytes: u64,
    network_tx_bytes: u64,
    pids: u32,
    stale: bool,
}

#[derive(Debug)]
struct AgentMetadata {
    name: String,
    display_name: String,
    version: String,
}

impl DockerClient {
    /// Create a Docker client using the default socket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be initialized or the daemon
    /// cannot be reached.
    pub async fn new() -> anyhow::Result<Self> {
        let client =
            Docker::connect_with_socket_defaults().map_err(|error| anyhow::anyhow!(error))?;
        client
            .ping()
            .await
            .map_err(|error| anyhow::anyhow!(error))
            .map(|_| Self {
                client: Arc::new(client),
                stats_cache: Arc::new(RwLock::new(HashMap::new())),
            })
    }

    /// Query the workspace aggregate status from Docker.
    ///
    /// # Errors
    ///
    /// Returns an error if Docker metadata cannot be read.
    pub async fn workspace_status(&self) -> AppResult<WorkspaceResponse> {
        let containers = self.list_polis_containers().await?;
        let networks = self.list_project_networks().await?;

        Ok(build_workspace_response(&containers, networks))
    }

    /// Query the active workspace agent metadata from Docker labels.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace container or required labels are
    /// unavailable.
    pub async fn agent_info(&self) -> AppResult<AgentResponse> {
        let summary = self.workspace_container_summary().await?;
        let container_name = container_name(&summary);
        let inspect = self.inspect_container(&container_name).await?;
        let labels = summary.labels.as_ref();
        let metadata = agent_metadata_from_labels(labels, &container_name)?;
        let stats = self.container_stats(&container_name).await?;

        Ok(AgentResponse {
            name: metadata.name,
            display_name: metadata.display_name,
            version: metadata.version,
            status: status_from_inspect(&inspect),
            health: health_from_inspect(&inspect),
            uptime_seconds: uptime_seconds(&inspect),
            ports: port_mappings_from_summary(summary.ports.as_deref()),
            resources: stats.resources.clone(),
            stale: stats.stale,
        })
    }

    /// Query detailed container information for Polis containers.
    ///
    /// # Errors
    ///
    /// Returns an error if Docker metadata cannot be read.
    pub async fn container_details(&self) -> AppResult<Vec<ContainerInfo>> {
        let summaries = self.list_polis_container_summaries().await?;
        let mut containers = Vec::with_capacity(summaries.len());

        for summary in summaries {
            containers.push(self.enrich_container(summary).await?);
        }

        containers.sort_by(|left, right| {
            compare_resource_desc(left.memory_usage_mb, right.memory_usage_mb)
                .then_with(|| left.service.cmp(&right.service))
                .then_with(|| left.name.cmp(&right.name))
        });

        Ok(containers)
    }

    /// Query point-in-time resource usage for all Polis containers.
    ///
    /// # Errors
    ///
    /// Returns an error if Docker metadata cannot be read.
    pub async fn metrics_snapshot(&self) -> AppResult<MetricsResponse> {
        let summaries = self.list_polis_container_summaries().await?;
        let mut containers = Vec::with_capacity(summaries.len());
        let mut total_memory_usage_mb = 0_u64;
        let mut max_memory_limit_mb = 0_u64;
        let mut total_cpu_percent = 0.0_f64;

        for summary in summaries {
            let name = container_name(&summary);
            let inspect = self.inspect_container(&name).await?;
            let stats = self.container_stats(&name).await?;

            total_memory_usage_mb += stats.resources.memory_usage_mb;
            // Use the max container limit as a proxy for the host's total RAM.
            // Containers without explicit limits report the host's total memory,
            // so the maximum across all containers equals the VM's actual RAM.
            if stats.resources.memory_limit_mb > max_memory_limit_mb {
                max_memory_limit_mb = stats.resources.memory_limit_mb;
            }
            total_cpu_percent += stats.resources.cpu_percent;

            containers.push(ApiContainerMetrics {
                service: service_name(&summary, &name),
                status: status_from_inspect(&inspect),
                health: health_from_inspect(&inspect),
                memory_usage_mb: stats.resources.memory_usage_mb,
                memory_limit_mb: stats.resources.memory_limit_mb,
                cpu_percent: stats.resources.cpu_percent,
                network_rx_bytes: stats.network_rx_bytes,
                network_tx_bytes: stats.network_tx_bytes,
                pids: stats.pids,
                stale: stats.stale,
            });
        }

        containers.sort_by(|left, right| left.service.cmp(&right.service));

        Ok(MetricsResponse {
            timestamp: Utc::now(),
            system: ApiSystemMetrics {
                total_memory_usage_mb,
                total_memory_limit_mb: max_memory_limit_mb,
                total_cpu_percent: ((total_cpu_percent * 100.0).round()) / 100.0,
                container_count: containers.len(),
            },
            containers,
        })
    }

    /// Query aggregated container logs for Polis services.
    ///
    /// # Errors
    ///
    /// Returns an error if Docker metadata cannot be read.
    pub async fn logs_snapshot(
        &self,
        service: Option<&str>,
        lines: usize,
        since_seconds: Option<i64>,
        level: Option<&str>,
    ) -> AppResult<LogsResponse> {
        let since = since_unix_timestamp(since_seconds)?;
        let summaries = self.list_polis_container_summaries().await?;
        let containers = summaries
            .into_iter()
            .filter_map(|summary| {
                let name = container_name(&summary);
                let service_name = service_name(&summary, &name);
                if service.is_some_and(|expected| expected != service_name) {
                    return None;
                }
                Some((name, service_name))
            })
            .collect::<Vec<_>>();

        let requests = containers
            .into_iter()
            .map(|(container_name, service_name)| {
                let client = Arc::clone(&self.client);
                async move {
                    fetch_logs_for_container(client, &container_name, &service_name, lines, since)
                        .await
                }
            });

        let mut all_lines = Vec::new();
        for result in join_all(requests).await {
            match result {
                Ok(mut lines) => all_lines.append(&mut lines),
                Err(error) => tracing::debug!(%error, "skipping failed container log fetch"),
            }
        }

        Ok(aggregate_log_lines(all_lines, lines, level))
    }

    async fn enrich_container(&self, summary: ContainerSummary) -> AppResult<ContainerInfo> {
        let name = container_name(&summary);
        let inspect = self.inspect_container(&name).await?;
        let stats = self.container_stats(&name).await?;
        let (network, ip_address) = primary_network_and_ip(inspect.network_settings.as_ref());

        Ok(ContainerInfo {
            name: name.clone(),
            service: service_name(&summary, &name),
            status: status_from_inspect(&inspect),
            health: health_from_inspect(&inspect),
            uptime_seconds: uptime_seconds(&inspect),
            memory_usage_mb: stats.resources.memory_usage_mb,
            memory_limit_mb: stats.resources.memory_limit_mb,
            cpu_percent: stats.resources.cpu_percent,
            network,
            ip: ip_address,
            stale: stats.stale,
        })
    }

    async fn workspace_container_summary(&self) -> AppResult<ContainerSummary> {
        let containers = self.list_polis_container_summaries().await?;
        containers
            .into_iter()
            .find(|summary| service_name(summary, &container_name(summary)) == "workspace")
            .ok_or_else(|| AppError::NotFound(NO_ACTIVE_AGENT_MESSAGE.to_string()))
    }

    pub(crate) async fn list_polis_containers(&self) -> AppResult<Vec<ContainerInfo>> {
        self.container_details().await
    }

    async fn list_polis_container_summaries(&self) -> AppResult<Vec<ContainerSummary>> {
        let mut filters = HashMap::new();
        filters.insert("label".to_string(), vec![PROJECT_LABEL.to_string()]);

        self.client
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|error| map_bollard_error("failed to list polis containers", error))
    }

    async fn list_project_networks(&self) -> AppResult<HashMap<String, String>> {
        let mut filters = HashMap::new();
        filters.insert("label".to_string(), vec![PROJECT_LABEL.to_string()]);

        let networks = self
            .client
            .list_networks(Some(ListNetworksOptions::<String> { filters }))
            .await
            .map_err(|error| map_bollard_error("failed to list polis networks", error))?;

        Ok(network_map(networks))
    }

    async fn inspect_container(&self, name: &str) -> AppResult<ContainerInspectResponse> {
        self.client
            .inspect_container(name, None)
            .await
            .map_err(|error| {
                map_bollard_error(&format!("failed to inspect container `{name}`"), error)
            })
    }

    async fn container_stats(&self, name: &str) -> AppResult<ContainerStatsSnapshot> {
        let result = timeout(CONTAINER_STATS_TIMEOUT, async {
            let mut stream = self.client.stats(
                name,
                Some(StatsOptions {
                    stream: false,
                    one_shot: true,
                }),
            );
            stream.next().await
        })
        .await;

        match result {
            Ok(Some(Ok(stats))) => {
                let snapshot = stats_snapshot_from_response(&stats);
                self.store_cached_stats(name, &snapshot).await;
                Ok(snapshot)
            }
            Ok(Some(Err(error))) => {
                self.cached_stats_or_error(
                    name,
                    map_bollard_error(&format!("failed to read stats for `{name}`"), error),
                )
                .await
            }
            Ok(None) => {
                self.cached_stats_or_error(
                    name,
                    AppError::DependencyUnavailable(format!(
                        "docker stats stream ended for `{name}`"
                    )),
                )
                .await
            }
            Err(_) => {
                self.cached_stats_or_error(
                    name,
                    AppError::DependencyUnavailable(format!("docker stats timed out for `{name}`")),
                )
                .await
            }
        }
    }

    async fn store_cached_stats(&self, name: &str, snapshot: &ContainerStatsSnapshot) {
        self.stats_cache.write().await.insert(
            name.to_string(),
            CachedStats {
                snapshot: snapshot.clone(),
                captured_at: Instant::now(),
            },
        );
    }

    async fn cached_stats_or_error(
        &self,
        name: &str,
        error: AppError,
    ) -> AppResult<ContainerStatsSnapshot> {
        let cache = self.stats_cache.read().await;
        if let Some(cached) = cache.get(name) {
            let stale = cached.captured_at.elapsed() > (CONTAINER_STATS_TIMEOUT * 2);
            let mut snapshot = cached.snapshot.clone();
            snapshot.stale = stale;
            return Ok(snapshot);
        }

        Err(error)
    }
}

#[derive(Clone, Debug, Default)]
pub struct MetricsCollector {
    inner: Arc<RwLock<MetricsState>>,
}

#[derive(Clone, Debug, Default)]
struct MetricsState {
    current: Option<MetricsResponse>,
    history: VecDeque<MetricsPoint>,
}

impl MetricsCollector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn current_snapshot(&self) -> Option<MetricsResponse> {
        self.inner.read().await.current.clone()
    }

    pub async fn update_snapshot(&self, metrics: MetricsResponse) {
        let mut inner = self.inner.write().await;
        inner.current = Some(metrics.clone());
        inner.history.push_back(MetricsPoint {
            timestamp: metrics.timestamp,
            total_memory_usage_mb: metrics.system.total_memory_usage_mb,
            total_cpu_percent: metrics.system.total_cpu_percent,
        });
        while inner.history.len() > METRICS_HISTORY_LIMIT {
            inner.history.pop_front();
        }
    }

    pub async fn history(&self, minutes: Option<u32>) -> MetricsHistoryResponse {
        let inner = self.inner.read().await;
        let points = inner.history.iter().cloned().collect::<Vec<_>>();
        let limit = history_point_limit(minutes);
        let start = points.len().saturating_sub(limit);
        MetricsHistoryResponse {
            points: points[start..].to_vec(),
            interval_seconds: u32::try_from(METRICS_INTERVAL_SECONDS).unwrap_or(u32::MAX),
        }
    }
}

fn history_point_limit(minutes: Option<u32>) -> usize {
    let minutes = minutes.unwrap_or(30).clamp(1, 60);
    let points = (u64::from(minutes) * 60) / METRICS_INTERVAL_SECONDS;
    usize::try_from(points.max(1)).unwrap_or(METRICS_HISTORY_LIMIT)
}

fn build_workspace_response(
    containers: &[ContainerInfo],
    networks: HashMap<String, String>,
) -> WorkspaceResponse {
    let mut healthy = 0_usize;
    let mut unhealthy = 0_usize;
    let mut starting = 0_usize;

    for container in containers {
        match container.health.as_str() {
            "healthy" => healthy += 1,
            "unhealthy" => unhealthy += 1,
            "starting" => starting += 1,
            _ => {}
        }
    }

    WorkspaceResponse {
        status: workspace_status_label(containers),
        uptime_seconds: workspace_uptime_seconds(containers),
        containers: ApiContainerSummary {
            total: containers.len(),
            healthy,
            unhealthy,
            starting,
        },
        networks,
    }
}

fn workspace_status_label(containers: &[ContainerInfo]) -> String {
    if containers.is_empty() {
        return "stopped".to_string();
    }

    if containers.iter().any(|container| {
        matches!(
            container.status.as_str(),
            "dead" | "exited" | "restarting" | "removing"
        ) || container.health == "unhealthy"
    }) {
        return "degraded".to_string();
    }

    if containers
        .iter()
        .all(|container| container.status == "running")
    {
        return "running".to_string();
    }

    "unknown".to_string()
}

fn workspace_uptime_seconds(containers: &[ContainerInfo]) -> Option<u64> {
    containers
        .iter()
        .filter(|container| container.status == "running")
        .filter_map(|container| container.uptime_seconds)
        .max()
}

fn strip_ansi_sequences(message: &str) -> String {
    let mut sanitized = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        sanitized.push(ch);
    }

    sanitized
}

fn network_map(networks: Vec<Network>) -> HashMap<String, String> {
    networks
        .into_iter()
        .filter_map(|network| {
            let name = network.name?;
            let subnet = network
                .ipam
                .and_then(|ipam| ipam.config)
                .and_then(|configs| {
                    configs
                        .into_iter()
                        .find_map(|config| config.subnet.filter(|subnet| !subnet.is_empty()))
                })?;

            Some((normalize_network_name(&name), subnet))
        })
        .collect()
}

fn service_name(summary: &ContainerSummary, container_name: &str) -> String {
    summary
        .labels
        .as_ref()
        .and_then(|labels| labels.get("com.docker.compose.service"))
        .map_or_else(
            || normalize_service_name(container_name),
            |service| normalize_service_name(service),
        )
}

fn normalize_service_name(value: &str) -> String {
    value
        .trim_start_matches('/')
        .trim_start_matches("polis-")
        .to_string()
}

fn container_name(summary: &ContainerSummary) -> String {
    summary
        .names
        .as_ref()
        .and_then(|names| names.first())
        .map(|name| name.trim_start_matches('/').to_string())
        .or_else(|| summary.id.clone())
        .unwrap_or_else(|| "unknown-container".to_string())
}

fn status_from_inspect(inspect: &ContainerInspectResponse) -> String {
    inspect
        .state
        .as_ref()
        .and_then(|state| state.status)
        .map_or_else(
            || UNKNOWN_VALUE.to_string(),
            |status| status.to_string().to_ascii_lowercase(),
        )
}

fn health_from_inspect(inspect: &ContainerInspectResponse) -> String {
    inspect
        .state
        .as_ref()
        .and_then(|state| state.health.as_ref())
        .and_then(|health| health.status)
        .map_or_else(
            || "none".to_string(),
            |status| status.to_string().to_ascii_lowercase(),
        )
}

fn primary_network_and_ip(network_settings: Option<&NetworkSettings>) -> (String, String) {
    let mut networks = network_settings
        .and_then(|settings| settings.networks.as_ref())
        .map(|networks| {
            networks
                .iter()
                .map(|(name, endpoint)| (normalize_network_name(name), endpoint))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    networks.sort_by(|left, right| {
        network_rank(&left.0)
            .cmp(&network_rank(&right.0))
            .then_with(|| left.0.cmp(&right.0))
    });

    networks.into_iter().next().map_or_else(
        || (UNKNOWN_VALUE.to_string(), UNKNOWN_VALUE.to_string()),
        |(network, endpoint)| {
            let ip_address = endpoint
                .ip_address
                .clone()
                .filter(|ip| !ip.is_empty())
                .unwrap_or_else(|| UNKNOWN_VALUE.to_string());
            (network, ip_address)
        },
    )
}

fn network_rank(name: &str) -> usize {
    match name {
        "gateway-bridge" => 0,
        "internal-bridge" => 1,
        "external-bridge" => 2,
        "host-bridge" => 3,
        "internet" => 4,
        _ => 5,
    }
}

fn normalize_network_name(name: &str) -> String {
    name.strip_prefix("polis_").unwrap_or(name).to_string()
}

fn uptime_seconds(inspect: &ContainerInspectResponse) -> Option<u64> {
    let started_at = inspect
        .state
        .as_ref()
        .and_then(|state| state.started_at.as_deref())
        .filter(|timestamp| !timestamp.is_empty())?;

    let started_at = DateTime::parse_from_rfc3339(started_at).ok()?;
    let uptime = Utc::now().signed_duration_since(started_at.with_timezone(&Utc));
    u64::try_from(uptime.num_seconds()).ok()
}

fn port_mappings_from_summary(ports: Option<&[Port]>) -> Vec<PortMapping> {
    let mut mappings = ports
        .unwrap_or_default()
        .iter()
        .map(|port| PortMapping {
            container: port.private_port,
            host: port.public_port.unwrap_or_default(),
            protocol: port
                .typ
                .map(|typ| typ.to_string())
                .filter(|typ| !typ.is_empty())
                .unwrap_or_else(|| "tcp".to_string()),
        })
        .collect::<Vec<_>>();

    mappings.sort_by(|left, right| left.container.cmp(&right.container));
    mappings
}

async fn fetch_logs_for_container(
    client: Arc<Docker>,
    container_name: &str,
    service_name: &str,
    lines: usize,
    since: i64,
) -> AppResult<Vec<LogLine>> {
    let mut stream = client.logs(
        container_name,
        Some(LogsOptions::<String> {
            follow: false,
            stdout: true,
            stderr: true,
            since,
            timestamps: true,
            tail: lines.to_string(),
            ..Default::default()
        }),
    );

    let mut parsed = Vec::new();
    while let Some(entry) = stream.next().await {
        let output = entry.map_err(|error| {
            map_bollard_error(
                &format!("failed to read logs for `{container_name}`"),
                error,
            )
        })?;
        parsed.extend(parse_docker_log_output(service_name, &output));
    }

    Ok(parsed)
}

fn parse_docker_log_output(service: &str, output: &LogOutput) -> Vec<LogLine> {
    output
        .to_string()
        .lines()
        .filter_map(|line| parse_docker_log_line(service, line))
        .collect()
}

fn parse_docker_log_line(service: &str, line: &str) -> Option<LogLine> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (timestamp, message) = match trimmed.split_once(' ') {
        Some((candidate, message)) => match DateTime::parse_from_rfc3339(candidate) {
            Ok(timestamp) => (
                timestamp.with_timezone(&Utc),
                strip_ansi_sequences(message.trim()).trim().to_string(),
            ),
            Err(_) => (Utc::now(), strip_ansi_sequences(trimmed).trim().to_string()),
        },
        None => (Utc::now(), strip_ansi_sequences(trimmed).trim().to_string()),
    };

    Some(LogLine {
        timestamp,
        service: service.to_string(),
        level: detect_log_level(&message).to_string(),
        message,
    })
}

fn detect_log_level(message: &str) -> &'static str {
    // Check for explicit level markers first (e.g. "[INFO]", "level=warn").
    // This avoids false positives from words like "NOERROR" or "warning-page".
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("[error]")
        || lowered.contains("level=error")
        || lowered.contains("level=\"error\"")
    {
        return "error";
    }
    if lowered.contains("[warn]")
        || lowered.contains("[warning]")
        || lowered.contains("level=warn")
        || lowered.contains("level=\"warn\"")
    {
        return "warn";
    }
    if lowered.contains("[info]")
        || lowered.contains("[debug]")
        || lowered.contains("level=info")
        || lowered.contains("level=\"info\"")
    {
        return "info";
    }

    // Fallback: keyword scan, but filter out common false positives.
    let cleaned = lowered.replace("noerror", "");
    if cleaned.contains("error") || cleaned.contains("fatal") || cleaned.contains("panic") {
        "error"
    } else if cleaned.contains("warn") {
        "warn"
    } else {
        "info"
    }
}

fn aggregate_log_lines(mut lines: Vec<LogLine>, limit: usize, level: Option<&str>) -> LogsResponse {
    if let Some(level) = level {
        let requested = level.to_ascii_lowercase();
        lines.retain(|line| line.level == requested);
    }

    lines.sort_by(|left, right| {
        right
            .timestamp
            .cmp(&left.timestamp)
            .then_with(|| left.service.cmp(&right.service))
    });

    let total = lines.len();
    let truncated = total > limit;
    if truncated {
        lines.truncate(limit);
    }

    LogsResponse {
        lines,
        total,
        truncated,
    }
}

fn since_unix_timestamp(since_seconds: Option<i64>) -> AppResult<i64> {
    match since_seconds {
        Some(seconds) if seconds < 0 => Err(AppError::Validation(
            "since must be a non-negative number of seconds".to_string(),
        )),
        Some(seconds) => Ok(Utc::now().timestamp().saturating_sub(seconds)),
        None => Ok(0),
    }
}

fn stats_snapshot_from_response(stats: &Stats) -> ContainerStatsSnapshot {
    ContainerStatsSnapshot {
        resources: ResourceUsage {
            cpu_percent: calculate_cpu_percent(stats),
            memory_usage_mb: memory_usage_mb(stats),
            memory_limit_mb: memory_limit_mb(stats),
        },
        network_rx_bytes: network_bytes(stats, true),
        network_tx_bytes: network_bytes(stats, false),
        pids: process_count(stats),
        stale: false,
    }
}

#[allow(clippy::cast_precision_loss)]
fn calculate_cpu_percent(stats: &Stats) -> f64 {
    let cpu_total = stats.cpu_stats.cpu_usage.total_usage;
    let previous_cpu_total = stats.precpu_stats.cpu_usage.total_usage;
    let system_total = stats.cpu_stats.system_cpu_usage.unwrap_or_default();
    let previous_system_total = stats.precpu_stats.system_cpu_usage.unwrap_or_default();

    let cpu_delta = cpu_total.saturating_sub(previous_cpu_total);
    let system_delta = system_total.saturating_sub(previous_system_total);
    if cpu_delta == 0 || system_delta == 0 {
        return 0.0;
    }

    let online_cpus = stats
        .cpu_stats
        .online_cpus
        .map(|cpus| cpus as f64)
        .or_else(|| {
            stats
                .cpu_stats
                .cpu_usage
                .percpu_usage
                .as_ref()
                .map(|cpus: &Vec<u64>| cpus.len() as f64)
        })
        .unwrap_or(1.0);

    ((cpu_delta as f64 / system_delta as f64) * online_cpus * 100.0 * 100.0).round() / 100.0
}

fn memory_usage_mb(stats: &Stats) -> u64 {
    let usage = stats.memory_stats.usage.unwrap_or_default();
    let cache = memory_cache_bytes(stats.memory_stats.stats.as_ref());
    let adjusted_usage = usage.saturating_sub(cache);
    bytes_to_rounded_mib(adjusted_usage)
}

fn memory_limit_mb(stats: &Stats) -> u64 {
    stats
        .memory_stats
        .limit
        .map(bytes_to_rounded_mib)
        .unwrap_or_default()
}

fn bytes_to_rounded_mib(bytes: u64) -> u64 {
    bytes.saturating_add(BYTES_PER_MIB / 2) / BYTES_PER_MIB
}

fn memory_cache_bytes(stats: Option<&bollard::container::MemoryStatsStats>) -> u64 {
    match stats {
        Some(bollard::container::MemoryStatsStats::V1(values)) => {
            if values.total_inactive_file > 0 {
                values.total_inactive_file
            } else {
                values.cache
            }
        }
        Some(bollard::container::MemoryStatsStats::V2(values)) => values.inactive_file,
        None => 0,
    }
}

fn network_bytes(stats: &Stats, receive: bool) -> u64 {
    stats
        .networks
        .as_ref()
        .map(
            |networks: &HashMap<String, bollard::container::NetworkStats>| {
                networks
                    .values()
                    .map(|network| {
                        if receive {
                            network.rx_bytes
                        } else {
                            network.tx_bytes
                        }
                    })
                    .sum()
            },
        )
        .unwrap_or_default()
}

fn process_count(stats: &Stats) -> u32 {
    stats
        .pids_stats
        .current
        .or_else(|| Some(u64::from(stats.num_procs)))
        .and_then(|count| u32::try_from(count).ok())
        .unwrap_or_default()
}

fn agent_metadata_from_labels(
    labels: Option<&HashMap<String, String>>,
    container_name: &str,
) -> AppResult<AgentMetadata> {
    let labels = labels.ok_or_else(|| {
        AppError::NotFound(format!("{NO_ACTIVE_AGENT_MESSAGE} for `{container_name}`"))
    })?;
    let name = labels
        .get("polis.agent.name")
        .cloned()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            AppError::NotFound(format!("{NO_ACTIVE_AGENT_MESSAGE} for `{container_name}`"))
        })?;
    let display_name = labels
        .get("polis.agent.display_name")
        .cloned()
        .filter(|display_name| !display_name.is_empty())
        .unwrap_or_else(|| name.clone());
    let version = labels
        .get("polis.agent.version")
        .cloned()
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| UNKNOWN_VALUE.to_string());

    Ok(AgentMetadata {
        name,
        display_name,
        version,
    })
}

fn compare_resource_desc(left: u64, right: u64) -> Ordering {
    right.cmp(&left)
}

fn map_bollard_error(operation: &str, error: BollardError) -> AppError {
    match error {
        BollardError::DockerResponseServerError {
            status_code: 404,
            message,
        } => AppError::NotFound(format!("{operation}: {message}")),
        other => AppError::DependencyUnavailable(format!("{operation}: {other}")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use bollard::{
        container::Stats,
        models::{EndpointSettings, Ipam, IpamConfig, NetworkSettings, Port},
    };
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn metrics_collector_tracks_current_snapshot() {
        let collector = MetricsCollector::new();
        let snapshot = MetricsResponse {
            timestamp: Utc::now(),
            system: ApiSystemMetrics {
                total_memory_usage_mb: 256,
                total_memory_limit_mb: 1_024,
                total_cpu_percent: 10.0,
                container_count: 4,
            },
            containers: Vec::new(),
        };

        collector.update_snapshot(snapshot.clone()).await;

        assert_eq!(collector.current_snapshot().await, Some(snapshot));
    }

    #[tokio::test]
    async fn metrics_collector_caps_history() {
        let collector = MetricsCollector::new();

        for index in 0..(METRICS_HISTORY_LIMIT + 5) {
            let index_u64 = u64::try_from(index).expect("history index fits in u64");
            let index_u32 = u32::try_from(index).expect("history index fits in u32");
            collector
                .update_snapshot(MetricsResponse {
                    timestamp: Utc::now(),
                    system: ApiSystemMetrics {
                        total_memory_usage_mb: index_u64,
                        total_memory_limit_mb: 1_024,
                        total_cpu_percent: f64::from(index_u32),
                        container_count: 1,
                    },
                    containers: Vec::new(),
                })
                .await;
        }

        let history = collector.history(Some(60)).await;

        assert_eq!(history.points.len(), METRICS_HISTORY_LIMIT);
        assert_eq!(
            history.interval_seconds,
            u32::try_from(METRICS_INTERVAL_SECONDS).expect("interval fits in u32")
        );
        assert!(
            (history
                .points
                .first()
                .expect("first point")
                .total_cpu_percent
                - 5.0)
                .abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn workspace_status_aggregates_container_health() {
        let workspace = build_workspace_response(
            &[
                sample_container("workspace", "running", "healthy", 512, Some(300)),
                sample_container("control-plane", "running", "healthy", 128, Some(120)),
                sample_container("gate", "restarting", "starting", 64, None),
            ],
            HashMap::from([(String::from("gateway-bridge"), String::from("10.20.0.0/24"))]),
        );

        assert_eq!(workspace.status, "degraded");
        assert_eq!(workspace.containers.total, 3);
        assert_eq!(workspace.containers.healthy, 2);
        assert_eq!(workspace.containers.starting, 1);
        assert_eq!(workspace.uptime_seconds, Some(300));
    }

    #[test]
    fn workspace_uptime_ignores_non_running_containers() {
        let workspace = build_workspace_response(
            &[
                sample_container("workspace", "running", "healthy", 512, Some(300)),
                sample_container("toolbox", "created", "none", 0, Some(63_908_531_152)),
            ],
            HashMap::new(),
        );

        assert_eq!(workspace.uptime_seconds, Some(300));
    }

    #[test]
    fn network_map_normalizes_compose_prefix() {
        let networks = vec![Network {
            name: Some("polis_gateway-bridge".to_string()),
            ipam: Some(Ipam {
                config: Some(vec![IpamConfig {
                    subnet: Some("10.20.0.0/24".to_string()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }];

        assert_eq!(
            network_map(networks),
            HashMap::from([(String::from("gateway-bridge"), String::from("10.20.0.0/24"))])
        );
    }

    #[test]
    fn primary_network_prefers_gateway_bridge() {
        let mut networks = HashMap::new();
        networks.insert(
            "polis_internal-bridge".to_string(),
            EndpointSettings {
                ip_address: Some("10.30.0.20".to_string()),
                ..Default::default()
            },
        );
        networks.insert(
            "polis_gateway-bridge".to_string(),
            EndpointSettings {
                ip_address: Some("10.20.0.10".to_string()),
                ..Default::default()
            },
        );

        let settings = NetworkSettings {
            networks: Some(networks),
            ..Default::default()
        };

        assert_eq!(
            primary_network_and_ip(Some(&settings)),
            (String::from("gateway-bridge"), String::from("10.20.0.10"))
        );
    }

    #[test]
    fn port_mapping_defaults_to_tcp() {
        let ports = vec![Port {
            private_port: 8080,
            public_port: Some(9080),
            typ: None,
            ..Default::default()
        }];

        assert_eq!(
            port_mappings_from_summary(Some(&ports)),
            vec![PortMapping {
                container: 8080,
                host: 9080,
                protocol: "tcp".to_string(),
            }]
        );
    }

    #[test]
    fn stats_snapshot_calculates_cpu_and_memory_usage() {
        let stats: Stats = serde_json::from_value(json!({
            "read": "2026-03-06T08:00:00Z",
            "preread": "2026-03-06T07:59:50Z",
            "num_procs": 12,
            "pids_stats": { "current": 12, "limit": 0 },
            "networks": {
                "eth0": {
                    "rx_bytes": 1024,
                    "rx_packets": 0,
                    "rx_errors": 0,
                    "rx_dropped": 0,
                    "tx_bytes": 2048,
                    "tx_packets": 0,
                    "tx_errors": 0,
                    "tx_dropped": 0
                }
            },
            "memory_stats": {
                "usage": 314_572_800,
                "limit": 536_870_912,
                "stats": {
                    "cache": 41_943_040,
                    "dirty": 0,
                    "mapped_file": 0,
                    "total_inactive_file": 41_943_040,
                    "pgpgout": 0,
                    "rss": 0,
                    "total_mapped_file": 0,
                    "writeback": 0,
                    "unevictable": 0,
                    "pgpgin": 0,
                    "total_unevictable": 0,
                    "pgmajfault": 0,
                    "total_rss": 0,
                    "total_rss_huge": 0,
                    "total_writeback": 0,
                    "total_inactive_anon": 0,
                    "rss_huge": 0,
                    "hierarchical_memory_limit": 0,
                    "total_pgfault": 0,
                    "total_active_file": 0,
                    "active_anon": 0,
                    "total_active_anon": 0,
                    "total_pgpgout": 0,
                    "total_cache": 0,
                    "total_dirty": 0,
                    "inactive_anon": 0,
                    "active_file": 0,
                    "pgfault": 0,
                    "inactive_file": 41_943_040,
                    "total_pgmajfault": 0,
                    "total_pgpgin": 0
                }
            },
            "blkio_stats": {},
            "cpu_stats": {
                "cpu_usage": {
                    "percpu_usage": [100, 100],
                    "usage_in_usermode": 0,
                    "total_usage": 200,
                    "usage_in_kernelmode": 0
                },
                "system_cpu_usage": 1000,
                "online_cpus": 2,
                "throttling_data": {
                    "periods": 0,
                    "throttled_periods": 0,
                    "throttled_time": 0
                }
            },
            "precpu_stats": {
                "cpu_usage": {
                    "percpu_usage": [50, 50],
                    "usage_in_usermode": 0,
                    "total_usage": 100,
                    "usage_in_kernelmode": 0
                },
                "system_cpu_usage": 500,
                "online_cpus": 2,
                "throttling_data": {
                    "periods": 0,
                    "throttled_periods": 0,
                    "throttled_time": 0
                }
            },
            "storage_stats": {},
            "name": "polis-workspace",
            "id": "workspace-id"
        }))
        .expect("stats deserialize");

        let snapshot = stats_snapshot_from_response(&stats);

        assert!((snapshot.resources.cpu_percent - 40.0).abs() < f64::EPSILON);
        assert_eq!(snapshot.resources.memory_usage_mb, 260);
        assert_eq!(snapshot.resources.memory_limit_mb, 512);
        assert_eq!(snapshot.network_rx_bytes, 1_024);
        assert_eq!(snapshot.network_tx_bytes, 2_048);
        assert_eq!(snapshot.pids, 12);
        assert!(!snapshot.stale);
    }

    #[test]
    fn detect_log_level_matches_common_patterns() {
        assert_eq!(detect_log_level("INFO workspace booted"), "info");
        assert_eq!(detect_log_level("WARN retrying socket"), "warn");
        assert_eq!(detect_log_level("ERROR failed to connect"), "error");
        assert_eq!(detect_log_level("panic in worker"), "error");

        // Explicit markers take precedence.
        assert_eq!(
            detect_log_level("[INFO] 10.10.1.3 - AAAA IN gate.msh."),
            "info"
        );
        assert_eq!(detect_log_level("[ERROR] connection refused"), "error");
        assert_eq!(detect_log_level("[WARN] slow query"), "warn");

        // "NOERROR" (DNS rcode) must NOT be classified as error.
        assert_eq!(detect_log_level("rcode NOERROR answer 0"), "info");
        assert_eq!(detect_log_level("[INFO] query AAAA rcode NOERROR"), "info");
    }

    #[test]
    fn parse_docker_log_line_strips_ansi_sequences() {
        let line = "2026-03-08T01:45:52Z \u{1b}[2m2026-03-08T01:45:52Z\u{1b}[0m \u{1b}[33mWARN\u{1b}[0m cp_server: failed to poll agent snapshot";

        let parsed = parse_docker_log_line("control-plane", line).expect("parsed log line");

        assert_eq!(parsed.level, "warn");
        assert!(!parsed.message.contains('\u{1b}'));
        assert!(parsed.message.contains("failed to poll agent snapshot"));
    }

    #[test]
    fn aggregate_log_lines_sorts_filters_and_truncates() {
        let base_time = Utc::now();
        let response = aggregate_log_lines(
            vec![
                LogLine {
                    timestamp: base_time - chrono::TimeDelta::seconds(2),
                    service: "workspace".to_string(),
                    level: "info".to_string(),
                    message: "older".to_string(),
                },
                LogLine {
                    timestamp: base_time,
                    service: "control-plane".to_string(),
                    level: "warn".to_string(),
                    message: "newest".to_string(),
                },
                LogLine {
                    timestamp: base_time - chrono::TimeDelta::seconds(1),
                    service: "workspace".to_string(),
                    level: "warn".to_string(),
                    message: "middle".to_string(),
                },
            ],
            1,
            Some("warn"),
        );

        assert_eq!(response.total, 2);
        assert!(response.truncated);
        assert_eq!(response.lines.len(), 1);
        assert_eq!(response.lines[0].message, "newest");
    }

    #[test]
    fn agent_metadata_requires_workspace_labels() {
        let error =
            agent_metadata_from_labels(None, "polis-workspace").expect_err("missing labels");
        assert_eq!(
            error.to_string(),
            "no active agent detected for `polis-workspace`"
        );
    }

    fn sample_container(
        service: &str,
        status: &str,
        health: &str,
        memory_usage_mb: u64,
        uptime_seconds: Option<u64>,
    ) -> ContainerInfo {
        ContainerInfo {
            name: format!("polis-{service}"),
            service: service.to_string(),
            status: status.to_string(),
            health: health.to_string(),
            uptime_seconds,
            memory_usage_mb,
            memory_limit_mb: memory_usage_mb * 2,
            cpu_percent: 5.0,
            network: "gateway-bridge".to_string(),
            ip: "10.20.0.10".to_string(),
            stale: false,
        }
    }
}
