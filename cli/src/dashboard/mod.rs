//! Interactive control-plane dashboard.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::{
    io::{self, Stdout},
    process::ExitCode,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use cp_api_types::{
    ActionResponse, AgentResponse, BlockedItem, BlockedListResponse, ContainerInfo,
    ContainersResponse, ErrorResponse, EventItem, EventsResponse, LevelResponse, LogLine,
    LogsResponse, MetricsResponse, RuleItem, RulesResponse, StatusResponse, WorkspaceResponse,
};
use crossterm::{
    event::{Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, Tabs},
};
use reqwest::Client;
use tokio::{
    select,
    sync::mpsc,
    task::JoinHandle,
    time::{MissedTickBehavior, interval, sleep},
};

use crate::app::AppContext;

const DEFAULT_API_URL: &str = "http://localhost:9080";
const FLASH_TTL: Duration = Duration::from_secs(4);
const RENDER_INTERVAL: Duration = Duration::from_millis(100);
const MAX_EVENT_ROWS: usize = 200;
const DEFAULT_LOG_LINES: usize = 200;
const LOG_SERVICES: [&str; 8] = [
    "gate",
    "sentinel",
    "scanner",
    "resolver",
    "state",
    "toolbox",
    "workspace",
    "control-plane",
];
const LOG_LEVELS: [&str; 3] = ["info", "warn", "error"];
const UNKNOWN_VALUE: &str = "unknown";

#[derive(Debug, Clone, Args)]
pub struct DashboardArgs {
    /// Base URL for the control-plane HTTP API.
    #[arg(long, default_value = DEFAULT_API_URL)]
    pub api_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Dashboard,
    Blocked,
    Events,
    Workspace,
    Logs,
}

impl Tab {
    const ALL: [Self; 5] = [
        Self::Dashboard,
        Self::Blocked,
        Self::Events,
        Self::Workspace,
        Self::Logs,
    ];

    fn next(self) -> Self {
        match self {
            Self::Dashboard => Self::Blocked,
            Self::Blocked => Self::Events,
            Self::Events => Self::Workspace,
            Self::Workspace => Self::Logs,
            Self::Logs => Self::Dashboard,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Dashboard => Self::Logs,
            Self::Blocked => Self::Dashboard,
            Self::Events => Self::Blocked,
            Self::Workspace => Self::Events,
            Self::Logs => Self::Workspace,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Blocked => "Blocked",
            Self::Events => "Events",
            Self::Workspace => "Workspace",
            Self::Logs => "Logs",
        }
    }
}

fn tab_index(tab: Tab) -> usize {
    match tab {
        Tab::Dashboard => 0,
        Tab::Blocked => 1,
        Tab::Events => 2,
        Tab::Workspace => 3,
        Tab::Logs => 4,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnectionState {
    Connected,
    Connecting,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlashKind {
    Success,
    Error,
}

#[derive(Debug, Clone)]
struct FlashMessage {
    text: String,
    kind: FlashKind,
    expires_at: Instant,
}

#[derive(Debug)]
enum DashboardUpdate {
    Server(DashboardServerEvent),
    Connection(ConnectionState),
}

#[derive(Debug)]
enum DashboardServerEvent {
    Status(StatusResponse),
    Blocked(BlockedListResponse),
    EventLog(EventsResponse),
    Rules(RulesResponse),
    Workspace(WorkspaceResponse),
    Agent(AgentResponse),
    Metrics(MetricsResponse),
}

#[derive(Debug, PartialEq, Eq)]
enum UserAction {
    Approve(String),
    Deny(String),
    SetLevel(String),
    RefreshLogs,
    RefreshContainers,
}

#[derive(Debug, Clone, Default)]
struct LogFilter {
    service: Option<String>,
    level: Option<String>,
}

impl LogFilter {
    fn cycle_service(&mut self) {
        self.service = cycle_filter(self.service.as_deref(), &LOG_SERVICES);
    }

    fn cycle_level(&mut self) {
        self.level = cycle_filter(self.level.as_deref(), &LOG_LEVELS);
    }

    fn service_label(&self) -> &str {
        self.service.as_deref().unwrap_or("all services")
    }

    fn level_label(&self) -> &str {
        self.level.as_deref().unwrap_or("all levels")
    }
}

#[derive(Debug)]
struct App {
    current_tab: Tab,
    status: StatusResponse,
    blocked: Vec<BlockedItem>,
    events: Vec<EventItem>,
    rules: Vec<RuleItem>,
    workspace: Option<WorkspaceResponse>,
    agent: Option<AgentResponse>,
    containers: Vec<ContainerInfo>,
    metrics: Option<MetricsResponse>,
    logs: Vec<LogLine>,
    log_filter: LogFilter,
    selected_index: usize,
    connection_state: ConnectionState,
    last_message: Option<FlashMessage>,
    should_quit: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct SseFrame {
    event: String,
    data: String,
}

#[derive(Debug, Default)]
struct SseParser {
    buffer: String,
}

impl SseParser {
    fn push_chunk(&mut self, chunk: &str) -> Vec<SseFrame> {
        self.buffer.push_str(&chunk.replace("\r\n", "\n"));
        let mut frames = Vec::new();

        while let Some(index) = self.buffer.find("\n\n") {
            let raw = self.buffer[..index].to_string();
            self.buffer.drain(..index + 2);
            if let Some(frame) = parse_sse_frame(&raw) {
                frames.push(frame);
            }
        }

        frames
    }
}

#[derive(Debug, Clone, Copy)]
struct ReconnectBackoff {
    next_secs: u64,
}

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self { next_secs: 1 }
    }
}

impl ReconnectBackoff {
    fn next_delay(&mut self) -> Duration {
        let current = self.next_secs;
        self.next_secs = (self.next_secs.saturating_mul(2)).min(30);
        Duration::from_secs(current)
    }

    fn reset(&mut self) {
        self.next_secs = 1;
    }
}

fn empty_status() -> StatusResponse {
    StatusResponse {
        security_level: UNKNOWN_VALUE.to_string(),
        pending_count: 0,
        recent_approvals: 0,
        events_count: 0,
    }
}

impl App {
    fn new() -> Self {
        Self {
            current_tab: Tab::Dashboard,
            status: empty_status(),
            blocked: Vec::new(),
            events: Vec::new(),
            rules: Vec::new(),
            workspace: None,
            agent: None,
            containers: Vec::new(),
            metrics: None,
            logs: Vec::new(),
            log_filter: LogFilter::default(),
            selected_index: 0,
            connection_state: ConnectionState::Connecting,
            last_message: None,
            should_quit: false,
        }
    }

    fn apply_server_event(&mut self, event: DashboardServerEvent) {
        match event {
            DashboardServerEvent::Status(status) => self.status = status,
            DashboardServerEvent::Blocked(blocked) => {
                self.blocked = blocked.items;
                self.clamp_selection();
            }
            DashboardServerEvent::EventLog(events) => {
                self.events = events.events.into_iter().take(MAX_EVENT_ROWS).collect();
                self.clamp_selection();
            }
            DashboardServerEvent::Rules(rules) => self.rules = rules.rules,
            DashboardServerEvent::Workspace(workspace) => self.workspace = Some(workspace),
            DashboardServerEvent::Agent(agent) => self.agent = Some(agent),
            DashboardServerEvent::Metrics(metrics) => {
                self.update_containers_from_metrics(&metrics);
                self.metrics = Some(metrics);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<UserAction> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return None;
        }

        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                None
            }
            KeyCode::Tab => {
                self.current_tab = self.current_tab.next();
                self.selected_index = 0;
                self.tab_enter_action()
            }
            KeyCode::BackTab => {
                self.current_tab = self.current_tab.previous();
                self.selected_index = 0;
                self.tab_enter_action()
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                None
            }
            KeyCode::Char('a') if self.current_tab == Tab::Blocked => {
                self.selected_request_id().map(UserAction::Approve)
            }
            KeyCode::Char('d') if self.current_tab == Tab::Blocked => {
                self.selected_request_id().map(UserAction::Deny)
            }
            KeyCode::Char('1') if self.current_tab == Tab::Dashboard => {
                Some(UserAction::SetLevel("relaxed".to_string()))
            }
            KeyCode::Char('2') if self.current_tab == Tab::Dashboard => {
                Some(UserAction::SetLevel("balanced".to_string()))
            }
            KeyCode::Char('3') if self.current_tab == Tab::Dashboard => {
                Some(UserAction::SetLevel("strict".to_string()))
            }
            KeyCode::Char('f') if self.current_tab == Tab::Logs => {
                self.log_filter.cycle_service();
                self.selected_index = 0;
                Some(UserAction::RefreshLogs)
            }
            KeyCode::Char('l') if self.current_tab == Tab::Logs => {
                self.log_filter.cycle_level();
                self.selected_index = 0;
                Some(UserAction::RefreshLogs)
            }
            KeyCode::Char('r') if self.current_tab == Tab::Logs => Some(UserAction::RefreshLogs),
            _ => None,
        }
    }

    fn tab_enter_action(&self) -> Option<UserAction> {
        match self.current_tab {
            Tab::Workspace => Some(UserAction::RefreshContainers),
            Tab::Logs => Some(UserAction::RefreshLogs),
            _ => None,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.selection_len();
        if len == 0 {
            self.selected_index = 0;
            return;
        }

        let magnitude = delta.unsigned_abs();
        if delta.is_negative() {
            self.selected_index = self.selected_index.saturating_sub(magnitude);
        } else {
            self.selected_index = self
                .selected_index
                .saturating_add(magnitude)
                .min(len.saturating_sub(1));
        }
    }

    fn selection_len(&self) -> usize {
        match self.current_tab {
            Tab::Dashboard => 0,
            Tab::Blocked => self.blocked.len(),
            Tab::Events => self.events.len(),
            Tab::Workspace => self.containers.len(),
            Tab::Logs => self.logs.len(),
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.selection_len();
        if len == 0 {
            self.selected_index = 0;
        } else {
            self.selected_index = self.selected_index.min(len - 1);
        }
    }

    fn selected_request_id(&self) -> Option<String> {
        self.blocked
            .get(self.selected_index)
            .map(|item| item.request_id.clone())
    }

    fn set_connection_state(&mut self, state: ConnectionState) {
        self.connection_state = state;
    }

    fn flash(&mut self, kind: FlashKind, text: impl Into<String>) {
        self.last_message = Some(FlashMessage {
            text: text.into(),
            kind,
            expires_at: Instant::now() + FLASH_TTL,
        });
    }

    fn clear_expired_message(&mut self, now: Instant) {
        if self
            .last_message
            .as_ref()
            .is_some_and(|message| message.expires_at <= now)
        {
            self.last_message = None;
        }
    }

    fn replace_logs(&mut self, response: LogsResponse) {
        self.logs = response.lines;
        self.clamp_selection();
    }

    fn replace_containers(&mut self, response: ContainersResponse) {
        self.containers = sort_containers(response.containers);
        self.clamp_selection();
    }

    fn update_containers_from_metrics(&mut self, metrics: &MetricsResponse) {
        let mut containers = self.containers.clone();

        for metric in &metrics.containers {
            if let Some(existing) = containers
                .iter_mut()
                .find(|container| container.service == metric.service)
            {
                existing.status.clone_from(&metric.status);
                existing.health.clone_from(&metric.health);
                existing.memory_usage_mb = metric.memory_usage_mb;
                existing.memory_limit_mb = metric.memory_limit_mb;
                existing.cpu_percent = metric.cpu_percent;
                existing.stale = false;
            } else {
                containers.push(ContainerInfo {
                    name: format!("polis-{}", metric.service),
                    service: metric.service.clone(),
                    status: metric.status.clone(),
                    health: metric.health.clone(),
                    uptime_seconds: None,
                    memory_usage_mb: metric.memory_usage_mb,
                    memory_limit_mb: metric.memory_limit_mb,
                    cpu_percent: metric.cpu_percent,
                    network: UNKNOWN_VALUE.to_string(),
                    ip: UNKNOWN_VALUE.to_string(),
                    stale: false,
                });
            }
        }

        self.containers = sort_containers(containers);
        self.clamp_selection();
    }
}

fn cycle_filter(current: Option<&str>, options: &[&str]) -> Option<String> {
    match current {
        None => options.first().map(|value| (*value).to_string()),
        Some(current) => options
            .iter()
            .position(|value| *value == current)
            .and_then(|index| options.get(index + 1).map(|value| (*value).to_string())),
    }
}

fn sort_containers(mut containers: Vec<ContainerInfo>) -> Vec<ContainerInfo> {
    containers.sort_by(|left, right| {
        right
            .memory_usage_mb
            .cmp(&left.memory_usage_mb)
            .then_with(|| left.service.cmp(&right.service))
            .then_with(|| left.name.cmp(&right.name))
    });
    containers
}

fn visible_window(selected: usize, total: usize, max_rows: usize) -> std::ops::Range<usize> {
    if total == 0 || max_rows == 0 || total <= max_rows {
        return 0..total;
    }

    let max_start = total - max_rows;
    let start = selected.saturating_sub(max_rows / 2).min(max_start);
    start..(start + max_rows)
}

fn connection_label(state: &ConnectionState) -> (&str, Style) {
    match state {
        ConnectionState::Connected => (
            "Connected",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        ConnectionState::Connecting => (
            "Connecting...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        ConnectionState::Error(_) => (
            "Error",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    }
}

fn security_level_style(level: &str) -> Style {
    match level {
        "relaxed" => Style::default().fg(Color::Green),
        "balanced" => Style::default().fg(Color::Yellow),
        "strict" => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::Gray),
    }
}

fn health_style(health: &str) -> Style {
    match health {
        "healthy" => Style::default().fg(Color::Green),
        "starting" => Style::default().fg(Color::Yellow),
        "unhealthy" => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::Gray),
    }
}

fn log_level_style(level: &str) -> Style {
    match level {
        "error" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "warn" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::White),
    }
}

fn truncate_string(text: &str, max_len: usize) -> String {
    let total_chars = text.chars().count();
    if total_chars <= max_len {
        return text.to_string();
    }
    if max_len <= 3 {
        return ".".repeat(max_len);
    }

    let visible_len = max_len.saturating_sub(3);
    let truncated = text.chars().take(visible_len).collect::<String>();
    format!("{truncated}...")
}

fn level_badge(name: &str, current: &str) -> Span<'static> {
    let style = security_level_style(name);
    if name == current {
        Span::styled(format!("[{name}]"), style.add_modifier(Modifier::BOLD))
    } else {
        Span::styled(name.to_string(), Style::default().fg(Color::DarkGray))
    }
}

fn rule_action_style(action: &str) -> Style {
    match action {
        "allow" => Style::default().fg(Color::Green),
        "block" => Style::default().fg(Color::Red),
        "prompt" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Gray),
    }
}

fn parse_sse_frame(raw: &str) -> Option<SseFrame> {
    let mut event = String::new();
    let mut data = String::new();

    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            data = value.trim().to_string();
        }
    }

    if event.is_empty() && data.is_empty() {
        return None;
    }

    Some(SseFrame { event, data })
}

fn parse_dashboard_event(frame: &SseFrame) -> Result<Option<DashboardServerEvent>> {
    match frame.event.as_str() {
        "status" => Ok(Some(DashboardServerEvent::Status(
            serde_json::from_str(&frame.data).context("failed to parse status event")?,
        ))),
        "blocked" => Ok(Some(DashboardServerEvent::Blocked(
            serde_json::from_str(&frame.data).context("failed to parse blocked event")?,
        ))),
        "event_log" => Ok(Some(DashboardServerEvent::EventLog(
            serde_json::from_str(&frame.data).context("failed to parse events event")?,
        ))),
        "rules" => Ok(Some(DashboardServerEvent::Rules(
            serde_json::from_str(&frame.data).context("failed to parse rules event")?,
        ))),
        "workspace" => Ok(Some(DashboardServerEvent::Workspace(
            serde_json::from_str(&frame.data).context("failed to parse workspace event")?,
        ))),
        "agent" => Ok(Some(DashboardServerEvent::Agent(
            serde_json::from_str(&frame.data).context("failed to parse agent event")?,
        ))),
        "metrics" => Ok(Some(DashboardServerEvent::Metrics(
            serde_json::from_str(&frame.data).context("failed to parse metrics event")?,
        ))),
        _ => Ok(None),
    }
}

fn normalize_api_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

fn classify_connection_error(error: &anyhow::Error) -> ConnectionState {
    if let Some(request_error) = error.downcast_ref::<reqwest::Error>()
        && (request_error.is_connect() || request_error.is_timeout())
    {
        return ConnectionState::Connecting;
    }

    ConnectionState::Error(error.to_string())
}

fn render(frame: &mut Frame<'_>, app: &App) {
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(1),
    ])
    .split(frame.area());

    render_header(frame, outer[0], app);
    match app.current_tab {
        Tab::Dashboard => render_dashboard_tab(frame, outer[1], app),
        Tab::Blocked => render_blocked_tab(frame, outer[1], app),
        Tab::Events => render_events_tab(frame, outer[1], app),
        Tab::Workspace => render_workspace_tab(frame, outer[1], app),
        Tab::Logs => render_logs_tab(frame, outer[1], app),
    }
    render_footer(frame, outer[2], app);
}

fn odralabs_header_title() -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(
            "O",
            Style::default()
                .fg(Color::Rgb(107, 33, 168))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "D",
            Style::default()
                .fg(Color::Rgb(93, 37, 163))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "R",
            Style::default()
                .fg(Color::Rgb(64, 47, 153))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "A",
            Style::default()
                .fg(Color::Rgb(46, 53, 147))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "L",
            Style::default()
                .fg(Color::Rgb(37, 56, 144))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "A",
            Style::default()
                .fg(Color::Rgb(26, 107, 160))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "B",
            Style::default()
                .fg(Color::Rgb(26, 151, 179))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "S",
            Style::default()
                .fg(Color::Rgb(20, 184, 166))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Polis Control Plane",
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let columns = Layout::horizontal([Constraint::Min(48), Constraint::Length(20)]).split(area);
    let titles: Vec<Line<'_>> = Tab::ALL
        .iter()
        .map(|tab| Line::from(Span::raw(tab.title())))
        .collect();

    frame.render_widget(
        Tabs::new(titles)
            .select(tab_index(app.current_tab))
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .title(odralabs_header_title())
                    .borders(Borders::ALL),
            ),
        columns[0],
    );

    let (label, style) = connection_label(&app.connection_state);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(label, style)))
            .block(Block::default().title("Stream").borders(Borders::ALL)),
        columns[1],
    );
}

fn render_dashboard_tab(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(7),
        Constraint::Length(3),
        Constraint::Min(5),
    ])
    .split(area);

    let summary =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(rows[0]);
    render_policy_panel(frame, summary[0], app);
    render_agent_summary_panel(frame, summary[1], app);

    let cards = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(rows[1]);
    render_stat_card(
        frame,
        cards[0],
        "Pending",
        app.status.pending_count,
        Color::Yellow,
    );
    render_stat_card(
        frame,
        cards[1],
        "Approved",
        app.status.recent_approvals,
        Color::Green,
    );
    render_stat_card(
        frame,
        cards[2],
        "Events",
        app.status.events_count,
        Color::Cyan,
    );

    let gauges =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(rows[2]);
    render_cpu_gauge(frame, gauges[0], app.metrics.as_ref());
    render_memory_gauge(frame, gauges[1], app.metrics.as_ref());

    render_rules_panel(frame, rows[3], app);
}

fn render_policy_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let current = &app.status.security_level;
    let line = Line::from(vec![
        Span::styled(" Security: ", Style::default().add_modifier(Modifier::BOLD)),
        level_badge("relaxed", current),
        Span::raw("  "),
        level_badge("balanced", current),
        Span::raw("  "),
        level_badge("strict", current),
    ]);

    frame.render_widget(
        Paragraph::new(line).block(Block::default().title("Policy").borders(Borders::ALL)),
        area,
    );
}

fn inactive_agent_text(app: &App) -> &'static str {
    if app.workspace.is_some() || !app.containers.is_empty() {
        " No active agent detected"
    } else {
        " Awaiting agent snapshot"
    }
}

fn render_agent_summary_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let content = if let Some(agent) = &app.agent {
        Line::from(vec![
            Span::styled(" Agent: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(agent.display_name.clone(), Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("●", health_style(&agent.health)),
            Span::raw(" "),
            Span::styled(agent.health.clone(), health_style(&agent.health)),
        ])
    } else {
        Line::from(Span::styled(
            inactive_agent_text(app),
            Style::default().fg(Color::DarkGray),
        ))
    };

    frame.render_widget(
        Paragraph::new(content).block(Block::default().title("Agent").borders(Borders::ALL)),
        area,
    );
}

fn render_stat_card(frame: &mut Frame<'_>, area: Rect, title: &str, value: usize, color: Color) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("{value}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(title, Style::default().fg(Color::DarkGray))),
    ];

    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(Block::default().title(title).borders(Borders::ALL)),
        area,
    );
}

fn render_cpu_gauge(frame: &mut Frame<'_>, area: Rect, metrics: Option<&MetricsResponse>) {
    let (ratio, label) = metrics.map_or((0, "awaiting metrics".to_string()), |metrics| {
        (
            percentage_ratio(metrics.system.total_cpu_percent, 100.0),
            format!("{:.1}%", metrics.system.total_cpu_percent),
        )
    });

    frame.render_widget(
        Gauge::default()
            .block(Block::default().title("CPU").borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(f64::from(ratio) / 100.0)
            .label(label),
        area,
    );
}

fn render_memory_gauge(frame: &mut Frame<'_>, area: Rect, metrics: Option<&MetricsResponse>) {
    let (ratio, label) = metrics.map_or((0, "awaiting metrics".to_string()), |metrics| {
        (
            ratio_from_totals(
                metrics.system.total_memory_usage_mb,
                metrics.system.total_memory_limit_mb,
            ),
            format!(
                "{} / {}",
                format_memory_mb(metrics.system.total_memory_usage_mb),
                format_memory_mb(metrics.system.total_memory_limit_mb)
            ),
        )
    });

    frame.render_widget(
        Gauge::default()
            .block(Block::default().title("Memory").borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Magenta))
            .ratio(f64::from(ratio) / 100.0)
            .label(label),
        area,
    );
}

fn render_rules_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.rules.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No auto-approve rules configured",
                Style::default().fg(Color::DarkGray),
            )))
            .block(
                Block::default()
                    .title("Auto-Approve Rules")
                    .borders(Borders::ALL),
            ),
            area,
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Pattern").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Action").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let rows: Vec<Row<'_>> = app
        .rules
        .iter()
        .map(|rule| {
            Row::new(vec![
                Cell::from(rule.pattern.clone()),
                Cell::from(Span::styled(
                    rule.action.clone(),
                    rule_action_style(&rule.action),
                )),
            ])
        })
        .collect();

    frame.render_widget(
        Table::new(
            rows,
            [Constraint::Percentage(70), Constraint::Percentage(30)],
        )
        .header(header)
        .block(
            Block::default()
                .title("Auto-Approve Rules")
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn render_blocked_tab(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.blocked.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No blocked requests",
                Style::default().fg(Color::DarkGray),
            )))
            .block(
                Block::default()
                    .title("Blocked Requests")
                    .borders(Borders::ALL),
            ),
            area,
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Request ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Destination").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Reason").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let inner_height = area.height.saturating_sub(3) as usize;
    let window = visible_window(app.selected_index, app.blocked.len(), inner_height);
    let rows: Vec<Row<'_>> = app.blocked[window.clone()]
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let global_index = window.start + index;
            let style = if global_index == app.selected_index {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(truncate_string(&item.request_id, 16)),
                Cell::from(truncate_string(&item.destination, 30)),
                Cell::from(item.reason.clone()),
                Cell::from(item.status.clone()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Min(20),
            Constraint::Length(24),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(format!("Blocked Requests ({})", app.blocked.len()))
            .borders(Borders::ALL),
    );

    let mut state = ratatui::widgets::TableState::default();
    if !app.blocked.is_empty() {
        state.select(Some(app.selected_index - window.start));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_events_tab(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.events.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No events recorded",
                Style::default().fg(Color::DarkGray),
            )))
            .block(Block::default().title("Event Log").borders(Borders::ALL)),
            area,
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Time").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Type").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Details").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let inner_height = area.height.saturating_sub(3) as usize;
    let window = visible_window(app.selected_index, app.events.len(), inner_height);
    let rows: Vec<Row<'_>> = app.events[window.clone()]
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let global_index = window.start + index;
            let style = if global_index == app.selected_index {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(item.timestamp.format("%H:%M:%S").to_string()),
                Cell::from(item.event_type.clone()),
                Cell::from(truncate_string(&item.details, 60)),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(20),
            Constraint::Min(30),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(format!("Event Log ({})", app.events.len()))
            .borders(Borders::ALL),
    );

    let mut state = ratatui::widgets::TableState::default();
    if !app.events.is_empty() {
        state.select(Some(app.selected_index - window.start));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_workspace_tab(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(5),
    ])
    .split(area);

    render_workspace_summary(frame, rows[0], app.workspace.as_ref());
    render_agent_summary(frame, rows[1], app);
    render_workspace_containers(frame, rows[2], app);
}

fn render_workspace_summary(
    frame: &mut Frame<'_>,
    area: Rect,
    workspace: Option<&WorkspaceResponse>,
) {
    let workspace_line = if let Some(workspace) = workspace {
        let uptime = workspace
            .uptime_seconds
            .map_or_else(|| UNKNOWN_VALUE.to_string(), format_duration);
        Line::from(vec![
            Span::styled(
                format!(" Workspace: {}  ", workspace.status),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("Uptime: {uptime}"),
                Style::default().fg(Color::White),
            ),
        ])
    } else {
        Line::from(Span::styled(
            " Awaiting workspace snapshot",
            Style::default().fg(Color::DarkGray),
        ))
    };
    frame.render_widget(
        Paragraph::new(workspace_line)
            .block(Block::default().title("Workspace").borders(Borders::ALL)),
        area,
    );
}

fn render_agent_summary(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let agent_line = if let Some(agent) = &app.agent {
        Line::from(vec![
            Span::styled(
                format!(" {} v{} ", agent.display_name, agent.version),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled("●", health_style(&agent.health)),
            Span::raw(" "),
            Span::styled(agent.health.clone(), health_style(&agent.health)),
            Span::raw("  "),
            Span::styled(
                format!(
                    "Memory: {} / {}  CPU: {:.1}%",
                    format_memory_mb(agent.resources.memory_usage_mb),
                    format_memory_mb(agent.resources.memory_limit_mb),
                    agent.resources.cpu_percent
                ),
                Style::default().fg(Color::White),
            ),
            if agent.stale {
                Span::styled("  stale", Style::default().fg(Color::Yellow))
            } else {
                Span::raw("")
            },
        ])
    } else {
        Line::from(Span::styled(
            inactive_agent_text(app),
            Style::default().fg(Color::DarkGray),
        ))
    };
    frame.render_widget(
        Paragraph::new(agent_line).block(Block::default().title("Agent").borders(Borders::ALL)),
        area,
    );
}

fn render_workspace_containers(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.containers.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No container metrics available yet",
                Style::default().fg(Color::DarkGray),
            )))
            .block(Block::default().title("Containers").borders(Borders::ALL)),
            area,
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Container").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Health").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Memory").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("CPU").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let inner_height = usize::from(area.height.saturating_sub(3));
    let window = visible_window(app.selected_index, app.containers.len(), inner_height);
    let table_rows: Vec<Row<'_>> = app.containers[window.clone()]
        .iter()
        .enumerate()
        .map(|(index, container)| {
            let global_index = window.start + index;
            let row_style = if global_index == app.selected_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(container.name.clone()),
                Cell::from(Span::styled(
                    container.status.clone(),
                    Style::default().fg(Color::White),
                )),
                Cell::from(Span::styled(
                    container.health.clone(),
                    health_style(&container.health),
                )),
                Cell::from(format!(
                    "{} / {}",
                    format_memory_mb(container.memory_usage_mb),
                    format_memory_mb(container.memory_limit_mb)
                )),
                Cell::from(format!("{:.1}%", container.cpu_percent)),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        table_rows,
        [
            Constraint::Length(20),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(18),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(Block::default().title("Containers").borders(Borders::ALL));

    let mut state = ratatui::widgets::TableState::default();
    state.select(Some(app.selected_index.saturating_sub(window.start)));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_logs_tab(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(area);
    let filter_line = Line::from(vec![
        Span::styled(" Service: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            app.log_filter.service_label().to_string(),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("   "),
        Span::styled(" Level: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            app.log_filter.level_label().to_string(),
            Style::default().fg(Color::Yellow),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(filter_line).block(Block::default().title("Filters").borders(Borders::ALL)),
        rows[0],
    );

    if app.logs.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No logs loaded. Press r to refresh.",
                Style::default().fg(Color::DarkGray),
            )))
            .block(Block::default().title("Logs").borders(Borders::ALL)),
            rows[1],
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Time").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Service").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Level").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Message").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let inner_height = rows[1].height.saturating_sub(3) as usize;
    let window = visible_window(app.selected_index, app.logs.len(), inner_height);
    let table_rows: Vec<Row<'_>> = app.logs[window.clone()]
        .iter()
        .enumerate()
        .map(|(index, log)| {
            let global_index = window.start + index;
            let row_style = if global_index == app.selected_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(log.timestamp.format("%H:%M:%S").to_string()),
                Cell::from(Span::styled(
                    truncate_string(&log.service, 14),
                    Style::default().fg(Color::Cyan),
                )),
                Cell::from(Span::styled(
                    log.level.to_uppercase(),
                    log_level_style(&log.level),
                )),
                Cell::from(truncate_string(&log.message, 90)),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        table_rows,
        [
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Length(8),
            Constraint::Min(30),
        ],
    )
    .header(header)
    .block(Block::default().title("Logs").borders(Borders::ALL));

    let mut state = ratatui::widgets::TableState::default();
    state.select(Some(app.selected_index.saturating_sub(window.start)));
    frame.render_stateful_widget(table, rows[1], &mut state);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if let Some(message) = &app.last_message {
        let style = match message.kind {
            FlashKind::Success => Style::default().fg(Color::Green),
            FlashKind::Error => Style::default().fg(Color::Red),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" > ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(message.text.as_str(), style),
            ])),
            area,
        );
        return;
    }

    let hints = match app.current_tab {
        Tab::Dashboard => "1 relaxed | 2 balanced | 3 strict | Tab switch | q quit",
        Tab::Blocked => "a approve | d deny | j/k navigate | Tab switch | q quit",
        Tab::Events | Tab::Workspace => "j/k navigate | Tab switch | q quit",
        Tab::Logs => "f service | l level | r refresh | j/k scroll | Tab switch | q quit",
    };

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {hints}"),
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn percentage_ratio(value: f64, max: f64) -> u16 {
    if !value.is_finite() || value <= 0.0 || max <= 0.0 {
        return 0;
    }

    ((value / max) * 100.0).clamp(0.0, 100.0).round() as u16
}

fn ratio_from_totals(current: u64, total: u64) -> u16 {
    if total == 0 {
        return 0;
    }

    let capped = current.min(total);
    let percent = capped.saturating_mul(100).saturating_add(total / 2) / total;
    u16::try_from(percent).unwrap_or(100)
}

fn format_memory_mb(memory_mb: u64) -> String {
    if memory_mb >= 1_024 {
        let mut whole_gb = memory_mb / 1_024;
        let mut tenths = ((memory_mb % 1_024).saturating_mul(10).saturating_add(512)) / 1_024;
        if tenths == 10 {
            whole_gb = whole_gb.saturating_add(1);
            tenths = 0;
        }
        format!("{whole_gb}.{tenths} GB")
    } else {
        format!("{memory_mb} MB")
    }
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("failed to create terminal")
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;
    Ok(())
}

async fn approve_request(client: &Client, api_url: &str, id: &str) -> Result<ActionResponse> {
    send_action(
        client
            .post(format!("{api_url}/api/v1/blocked/{id}/approve"))
            .header(reqwest::header::CONTENT_TYPE, "application/json"),
    )
    .await
}

async fn deny_request(client: &Client, api_url: &str, id: &str) -> Result<ActionResponse> {
    send_action(
        client
            .post(format!("{api_url}/api/v1/blocked/{id}/deny"))
            .header(reqwest::header::CONTENT_TYPE, "application/json"),
    )
    .await
}

async fn set_security_level(client: &Client, api_url: &str, level: &str) -> Result<ActionResponse> {
    let response = client
        .put(format!("{api_url}/api/v1/config/level"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({ "level": level }))
        .send()
        .await
        .context("failed to set security level")?;
    let status = response.status();

    if status.is_success() {
        let level_response = response
            .json::<LevelResponse>()
            .await
            .context("failed to decode level response")?;
        return Ok(ActionResponse {
            message: format!("Security level set to {}", level_response.level),
        });
    }

    Err(anyhow!(
        parse_error_response(response)
            .await
            .unwrap_or_else(|_| format!("failed (status {status})"))
    ))
}

async fn send_action(request: reqwest::RequestBuilder) -> Result<ActionResponse> {
    let response = request.send().await.context("failed to send request")?;
    let status = response.status();
    if status.is_success() {
        return response
            .json::<ActionResponse>()
            .await
            .context("failed to decode response");
    }

    Err(anyhow!(
        parse_error_response(response)
            .await
            .unwrap_or_else(|_| format!("request failed (status {status})"))
    ))
}

async fn parse_error_response(response: reqwest::Response) -> Result<String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .context("failed to read error response")?;
    if let Ok(error) = serde_json::from_str::<ErrorResponse>(&text) {
        return Ok(error.error);
    }

    if text.trim().is_empty() {
        Ok(format!("request failed with status {status}"))
    } else {
        Ok(text)
    }
}

async fn fetch_status(client: &Client, api_url: &str) -> Result<StatusResponse> {
    client
        .get(format!("{api_url}/api/v1/status"))
        .send()
        .await
        .context("failed to fetch status")?
        .error_for_status()
        .context("status endpoint returned error")?
        .json::<StatusResponse>()
        .await
        .context("failed to decode status")
}

async fn fetch_rules(client: &Client, api_url: &str) -> Result<RulesResponse> {
    client
        .get(format!("{api_url}/api/v1/config/rules"))
        .send()
        .await
        .context("failed to fetch rules")?
        .error_for_status()
        .context("rules endpoint returned error")?
        .json::<RulesResponse>()
        .await
        .context("failed to decode rules")
}

async fn fetch_containers(client: &Client, api_url: &str) -> Result<ContainersResponse> {
    client
        .get(format!("{api_url}/api/v1/containers"))
        .send()
        .await
        .context("failed to fetch containers")?
        .error_for_status()
        .context("containers endpoint returned error")?
        .json::<ContainersResponse>()
        .await
        .context("failed to decode containers")
}

async fn fetch_logs(client: &Client, api_url: &str, filter: &LogFilter) -> Result<LogsResponse> {
    let endpoint = if let Some(service) = filter.service.as_deref() {
        format!("{api_url}/api/v1/logs/{service}")
    } else {
        format!("{api_url}/api/v1/logs")
    };
    let mut url = reqwest::Url::parse(&endpoint).context("failed to build logs URL")?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("lines", &DEFAULT_LOG_LINES.to_string());
        if let Some(level) = filter.level.as_deref() {
            query.append_pair("level", level);
        }
    }

    client
        .get(url)
        .send()
        .await
        .context("failed to fetch logs")?
        .error_for_status()
        .context("logs endpoint returned error")?
        .json::<LogsResponse>()
        .await
        .context("failed to decode logs")
}

fn spawn_sse_task(
    client: Client,
    api_url: String,
    sender: mpsc::UnboundedSender<DashboardUpdate>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = ReconnectBackoff::default();

        loop {
            let _ = sender.send(DashboardUpdate::Connection(ConnectionState::Connecting));
            match connect_sse(&client, &api_url, &sender).await {
                Ok(()) => backoff.reset(),
                Err(error) => {
                    let _ = sender.send(DashboardUpdate::Connection(classify_connection_error(
                        &error,
                    )));
                }
            }
            sleep(backoff.next_delay()).await;
        }
    })
}

async fn connect_sse(
    client: &Client,
    api_url: &str,
    sender: &mpsc::UnboundedSender<DashboardUpdate>,
) -> Result<()> {
    if let Ok(rules) = fetch_rules(client, api_url).await {
        let _ = sender.send(DashboardUpdate::Server(DashboardServerEvent::Rules(rules)));
    }

    let response = client
        .get(format!("{api_url}/api/v1/stream"))
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .send()
        .await
        .context("failed to connect to SSE endpoint")?
        .error_for_status()
        .context("SSE endpoint returned error")?;

    let _ = sender.send(DashboardUpdate::Connection(ConnectionState::Connected));
    let mut parser = SseParser::default();
    let mut byte_stream = response.bytes_stream();
    let mut rules_tick = interval(Duration::from_secs(5));
    rules_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        select! {
            maybe_chunk = byte_stream.next() => {
                match maybe_chunk {
                    Some(Ok(chunk)) => {
                        for frame in parser.push_chunk(&String::from_utf8_lossy(&chunk)) {
                            if frame.event == "config" {
                                if let Ok(status) = fetch_status(client, api_url).await {
                                    let _ = sender.send(DashboardUpdate::Server(DashboardServerEvent::Status(status)));
                                }
                                if let Ok(rules) = fetch_rules(client, api_url).await {
                                    let _ = sender.send(DashboardUpdate::Server(DashboardServerEvent::Rules(rules)));
                                }
                                continue;
                            }

                            if let Some(event) = parse_dashboard_event(&frame)? {
                                let _ = sender.send(DashboardUpdate::Server(event));
                            }
                        }
                    }
                    Some(Err(error)) => return Err(error.into()),
                    None => return Ok(()),
                }
            }
            _ = rules_tick.tick() => {
                if let Ok(rules) = fetch_rules(client, api_url).await {
                    let _ = sender.send(DashboardUpdate::Server(DashboardServerEvent::Rules(rules)));
                }
            }
        }
    }
}

/// Run the interactive control-plane dashboard.
///
/// # Errors
///
/// Returns an error if the dashboard terminal, HTTP client, or event loop fails.
pub async fn run(args: &DashboardArgs, app: &AppContext) -> Result<ExitCode> {
    if app.is_json() {
        bail!("dashboard does not support --json output");
    }

    let api_url = normalize_api_url(&args.api_url);
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .context("failed to build control-plane HTTP client")?;
    let mut terminal = init_terminal()?;
    let result = run_dashboard_loop(&mut terminal, client, api_url).await;
    let restore_result = restore_terminal(&mut terminal);
    restore_result?;
    result?;
    Ok(ExitCode::SUCCESS)
}

async fn run_dashboard_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    client: Client,
    api_url: String,
) -> Result<()> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let sse_task = spawn_sse_task(client.clone(), api_url.clone(), sender);
    let mut app = App::new();
    let mut key_events = EventStream::new();
    let mut render_tick = interval(RENDER_INTERVAL);
    render_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        terminal.draw(|frame| render(frame, &app))?;

        select! {
            maybe_update = receiver.recv() => {
                match maybe_update {
                    Some(DashboardUpdate::Server(event)) => app.apply_server_event(event),
                    Some(DashboardUpdate::Connection(state)) => app.set_connection_state(state),
                    None => {
                        app.set_connection_state(ConnectionState::Error("channel closed".to_string()));
                        app.should_quit = true;
                    }
                }
            }
            maybe_event = key_events.next() => {
                match maybe_event {
                    Some(Ok(CrosstermEvent::Key(key))) if key.kind == KeyEventKind::Press => {
                        if let Some(action) = app.handle_key(key) {
                            let result = match action {
                                UserAction::Approve(id) => approve_request(&client, &api_url, &id).await.map(Some),
                                UserAction::Deny(id) => deny_request(&client, &api_url, &id).await.map(Some),
                                UserAction::SetLevel(level) => set_security_level(&client, &api_url, &level).await.map(Some),
                                UserAction::RefreshLogs => {
                                    fetch_logs(&client, &api_url, &app.log_filter).await.map(|logs| {
                                        app.replace_logs(logs);
                                        None
                                    })
                                }
                                UserAction::RefreshContainers => {
                                    fetch_containers(&client, &api_url).await.map(|containers| {
                                        app.replace_containers(containers);
                                        None
                                    })
                                }
                            };

                            match result {
                                Ok(Some(response)) => app.flash(FlashKind::Success, response.message),
                                Ok(None) => {}
                                Err(error) => app.flash(FlashKind::Error, error.to_string()),
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => app.flash(FlashKind::Error, format!("terminal input error: {error}")),
                    None => {
                        app.set_connection_state(ConnectionState::Error("input stream closed".to_string()));
                        app.should_quit = true;
                    }
                }
            }
            _ = render_tick.tick() => {
                app.clear_expired_message(Instant::now());
            }
        }

        if app.should_quit {
            break;
        }
    }

    sse_task.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn sample_blocked(id: &str) -> BlockedItem {
        BlockedItem {
            request_id: id.to_string(),
            reason: "credential_detected".to_string(),
            destination: "https://example.com".to_string(),
            blocked_at: Utc
                .with_ymd_and_hms(2026, 3, 6, 12, 0, 0)
                .single()
                .expect("timestamp"),
            status: "pending".to_string(),
        }
    }

    fn sample_event() -> EventItem {
        EventItem {
            timestamp: Utc
                .with_ymd_and_hms(2026, 3, 6, 12, 0, 0)
                .single()
                .expect("timestamp"),
            event_type: "block_reported".to_string(),
            request_id: Some("req-abc12345".to_string()),
            details: "Blocked request".to_string(),
        }
    }

    fn sample_rule(pattern: &str, action: &str) -> RuleItem {
        RuleItem {
            pattern: pattern.to_string(),
            action: action.to_string(),
        }
    }

    fn sample_workspace() -> WorkspaceResponse {
        WorkspaceResponse {
            status: "running".to_string(),
            uptime_seconds: Some(3_600),
            containers: cp_api_types::ContainerSummary {
                total: 2,
                healthy: 2,
                unhealthy: 0,
                starting: 0,
            },
            networks: std::collections::HashMap::new(),
        }
    }

    fn sample_agent() -> AgentResponse {
        AgentResponse {
            name: "openclaw".to_string(),
            display_name: "OpenClaw".to_string(),
            version: "1.0.0".to_string(),
            status: "running".to_string(),
            health: "healthy".to_string(),
            uptime_seconds: Some(3_540),
            ports: Vec::new(),
            resources: cp_api_types::ResourceUsage {
                memory_usage_mb: 512,
                memory_limit_mb: 4_096,
                cpu_percent: 8.1,
            },
            stale: false,
        }
    }

    fn sample_metrics() -> MetricsResponse {
        MetricsResponse {
            timestamp: Utc
                .with_ymd_and_hms(2026, 3, 6, 12, 0, 0)
                .single()
                .expect("timestamp"),
            system: cp_api_types::SystemMetrics {
                total_memory_usage_mb: 768,
                total_memory_limit_mb: 4_096,
                total_cpu_percent: 12.5,
                container_count: 2,
            },
            containers: vec![
                cp_api_types::ContainerMetrics {
                    service: "workspace".to_string(),
                    status: "running".to_string(),
                    health: "healthy".to_string(),
                    memory_usage_mb: 512,
                    memory_limit_mb: 4_096,
                    cpu_percent: 8.1,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    pids: 42,
                },
                cp_api_types::ContainerMetrics {
                    service: "sentinel".to_string(),
                    status: "running".to_string(),
                    health: "healthy".to_string(),
                    memory_usage_mb: 256,
                    memory_limit_mb: 3_072,
                    cpu_percent: 3.5,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    pids: 8,
                },
            ],
        }
    }

    fn sample_logs() -> LogsResponse {
        LogsResponse {
            lines: vec![
                LogLine {
                    timestamp: Utc
                        .with_ymd_and_hms(2026, 3, 6, 12, 0, 1)
                        .single()
                        .expect("timestamp"),
                    service: "workspace".to_string(),
                    level: "info".to_string(),
                    message: "workspace booted".to_string(),
                },
                LogLine {
                    timestamp: Utc
                        .with_ymd_and_hms(2026, 3, 6, 12, 0, 2)
                        .single()
                        .expect("timestamp"),
                    service: "control-plane".to_string(),
                    level: "warn".to_string(),
                    message: "retrying socket".to_string(),
                },
            ],
            total: 2,
            truncated: false,
        }
    }

    #[test]
    fn app_new_uses_expected_defaults() {
        let app = App::new();

        assert_eq!(app.current_tab, Tab::Dashboard);
        assert_eq!(app.status.pending_count, 0);
        assert!(app.blocked.is_empty());
        assert!(app.events.is_empty());
        assert!(app.rules.is_empty());
        assert!(app.workspace.is_none());
        assert!(app.agent.is_none());
        assert!(app.containers.is_empty());
        assert!(app.metrics.is_none());
        assert!(app.logs.is_empty());
        assert!(app.log_filter.service.is_none());
        assert!(app.log_filter.level.is_none());
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.connection_state, ConnectionState::Connecting);
        assert!(app.last_message.is_none());
        assert!(!app.should_quit);
    }

    #[test]
    fn tab_index_matches_display_order() {
        assert_eq!(tab_index(Tab::Dashboard), 0);
        assert_eq!(tab_index(Tab::Blocked), 1);
        assert_eq!(tab_index(Tab::Events), 2);
        assert_eq!(tab_index(Tab::Workspace), 3);
        assert_eq!(tab_index(Tab::Logs), 4);
    }

    #[test]
    fn inactive_agent_text_reflects_dashboard_state() {
        let mut app = App::new();
        assert_eq!(inactive_agent_text(&app), " Awaiting agent snapshot");

        app.workspace = Some(sample_workspace());
        assert_eq!(inactive_agent_text(&app), " No active agent detected");
    }

    #[test]
    fn tab_switching_cycles_through_all_tabs() {
        let mut app = App::new();

        assert_eq!(app.current_tab, Tab::Dashboard);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.current_tab, Tab::Blocked);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.current_tab, Tab::Events);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.current_tab, Tab::Workspace);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.current_tab, Tab::Logs);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.current_tab, Tab::Dashboard);
    }

    #[test]
    fn tab_switching_requests_workspace_and_logs_refresh() {
        let mut app = App::new();

        app.current_tab = Tab::Events;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(UserAction::RefreshContainers)
        );
        assert_eq!(app.current_tab, Tab::Workspace);
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(UserAction::RefreshLogs)
        );
        assert_eq!(app.current_tab, Tab::Logs);
    }

    #[test]
    fn tab_switching_resets_selection() {
        let mut app = App::new();
        app.blocked = vec![sample_blocked("req-1"), sample_blocked("req-2")];
        app.current_tab = Tab::Blocked;
        app.selected_index = 1;

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(app.current_tab, Tab::Events);
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn blocked_actions_and_quit_keys_work() {
        let mut app = App::new();
        app.current_tab = Tab::Blocked;
        app.blocked = vec![sample_blocked("req-1"), sample_blocked("req-2")];

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_index, 1);
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
            Some(UserAction::Approve("req-2".to_string()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
            Some(UserAction::Deny("req-2".to_string()))
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn security_level_keys_produce_set_level_actions() {
        let mut app = App::new();

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)),
            Some(UserAction::SetLevel("relaxed".to_string()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            Some(UserAction::SetLevel("balanced".to_string()))
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE)),
            Some(UserAction::SetLevel("strict".to_string()))
        );
    }

    #[test]
    fn log_filter_keys_cycle_filters_and_refresh() {
        let mut app = App::new();
        app.current_tab = Tab::Logs;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)),
            Some(UserAction::RefreshLogs)
        );
        assert_eq!(app.log_filter.service.as_deref(), Some("gate"));

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)),
            Some(UserAction::RefreshLogs)
        );
        assert_eq!(app.log_filter.level.as_deref(), Some("info"));

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            Some(UserAction::RefreshLogs)
        );
    }

    #[test]
    fn apply_server_events_replaces_snapshots() {
        let mut app = App::new();

        app.apply_server_event(DashboardServerEvent::Status(StatusResponse {
            security_level: "strict".to_string(),
            pending_count: 2,
            recent_approvals: 4,
            events_count: 8,
        }));
        app.apply_server_event(DashboardServerEvent::Blocked(BlockedListResponse {
            items: vec![sample_blocked("req-1")],
        }));
        app.apply_server_event(DashboardServerEvent::EventLog(EventsResponse {
            events: vec![sample_event()],
        }));
        app.apply_server_event(DashboardServerEvent::Rules(RulesResponse {
            rules: vec![sample_rule("*.example.com", "allow")],
        }));
        app.apply_server_event(DashboardServerEvent::Workspace(sample_workspace()));
        app.apply_server_event(DashboardServerEvent::Agent(sample_agent()));
        app.apply_server_event(DashboardServerEvent::Metrics(sample_metrics()));

        assert_eq!(app.status.security_level, "strict");
        assert_eq!(app.blocked.len(), 1);
        assert_eq!(app.events.len(), 1);
        assert_eq!(app.rules.len(), 1);
        assert_eq!(
            app.workspace
                .as_ref()
                .map(|workspace| workspace.status.as_str()),
            Some("running")
        );
        assert_eq!(
            app.agent.as_ref().map(|agent| agent.name.as_str()),
            Some("openclaw")
        );
        assert_eq!(
            app.metrics.as_ref().map(|metrics| metrics.containers.len()),
            Some(2)
        );
        assert_eq!(app.containers.len(), 2);
        assert_eq!(app.containers[0].service, "workspace");
    }

    #[test]
    fn replace_logs_and_containers_updates_state() {
        let mut app = App::new();
        app.current_tab = Tab::Logs;
        app.replace_logs(sample_logs());
        assert_eq!(app.logs.len(), 2);

        app.current_tab = Tab::Workspace;
        app.replace_containers(ContainersResponse {
            containers: vec![
                ContainerInfo {
                    name: "polis-sentinel".to_string(),
                    service: "sentinel".to_string(),
                    status: "running".to_string(),
                    health: "healthy".to_string(),
                    uptime_seconds: Some(5),
                    memory_usage_mb: 128,
                    memory_limit_mb: 256,
                    cpu_percent: 2.0,
                    network: "internal-bridge".to_string(),
                    ip: "10.0.0.2".to_string(),
                    stale: false,
                },
                ContainerInfo {
                    name: "polis-workspace".to_string(),
                    service: "workspace".to_string(),
                    status: "running".to_string(),
                    health: "healthy".to_string(),
                    uptime_seconds: Some(5),
                    memory_usage_mb: 512,
                    memory_limit_mb: 1024,
                    cpu_percent: 8.0,
                    network: "internal-bridge".to_string(),
                    ip: "10.0.0.3".to_string(),
                    stale: false,
                },
            ],
        });

        assert_eq!(app.containers[0].service, "workspace");
    }

    #[test]
    fn sse_parser_handles_chunk_boundaries() {
        let status_json = serde_json::to_string(&StatusResponse {
            security_level: "balanced".to_string(),
            pending_count: 1,
            recent_approvals: 2,
            events_count: 3,
        })
        .expect("json");
        let blocked_json = serde_json::to_string(&BlockedListResponse {
            items: vec![sample_blocked("req-1")],
        })
        .expect("json");

        let mut parser = SseParser::default();
        let first = parser.push_chunk(&format!("event: status\ndata: {status_json}\n\n"));
        let second = parser.push_chunk(&format!("event: blocked\ndata: {blocked_json}\n"));
        let third = parser.push_chunk("\n");

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].event, "status");
        assert!(second.is_empty());
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].event, "blocked");
    }

    #[test]
    fn parse_dashboard_event_decodes_phase2_payloads() {
        let workspace_frame = SseFrame {
            event: "workspace".to_string(),
            data: serde_json::to_string(&sample_workspace()).expect("json"),
        };
        let metrics_frame = SseFrame {
            event: "metrics".to_string(),
            data: serde_json::to_string(&sample_metrics()).expect("json"),
        };

        match parse_dashboard_event(&workspace_frame).expect("parse") {
            Some(DashboardServerEvent::Workspace(workspace)) => {
                assert_eq!(workspace.status, "running");
            }
            _ => panic!("unexpected workspace event"),
        }

        match parse_dashboard_event(&metrics_frame).expect("parse") {
            Some(DashboardServerEvent::Metrics(metrics)) => {
                assert_eq!(metrics.containers.len(), 2);
            }
            _ => panic!("unexpected metrics event"),
        }
    }

    #[test]
    fn reconnect_backoff_doubles_and_caps() {
        let mut backoff = ReconnectBackoff::default();
        let delays: Vec<u64> = (0..7).map(|_| backoff.next_delay().as_secs()).collect();

        assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30]);
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn truncate_string_works() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("a long string here", 10), "a long ...");
        assert_eq!(truncate_string("emoji🙂text", 8), "emoji...");
        assert_eq!(truncate_string("truncate", 3), "...");
    }

    #[test]
    fn normalize_api_url_strips_trailing_slash() {
        assert_eq!(
            normalize_api_url("http://localhost:9080/"),
            "http://localhost:9080"
        );
        assert_eq!(
            normalize_api_url("http://localhost:9080"),
            "http://localhost:9080"
        );
    }

    #[test]
    fn visible_window_handles_edge_cases() {
        assert_eq!(visible_window(0, 0, 10), 0..0);
        assert_eq!(visible_window(0, 5, 10), 0..5);
        assert_eq!(visible_window(0, 20, 10), 0..10);
        assert_eq!(visible_window(15, 20, 10), 10..20);
    }

    #[test]
    fn ctrl_c_quits() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn formatting_helpers_render_expected_values() {
        assert_eq!(format_memory_mb(512), "512 MB");
        assert_eq!(format_memory_mb(2_048), "2.0 GB");
        assert_eq!(format_duration(5), "5s");
        assert_eq!(format_duration(125), "2m 5s");
        assert_eq!(format_duration(3_900), "1h 5m");
    }
}
