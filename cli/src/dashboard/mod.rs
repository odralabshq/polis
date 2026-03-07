//! Interactive control-plane dashboard.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::{
    io::{self, Stdout},
    process::ExitCode,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use cp_api_types::{
    ActionResponse, BlockedItem, BlockedListResponse, ErrorResponse, EventItem, EventsResponse,
    LevelResponse, RuleItem, RulesResponse, StatusResponse,
};
use crossterm::{
    event::{Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame, Terminal,
};
use reqwest::Client;
use tokio::{
    select,
    sync::mpsc,
    task::JoinHandle,
    time::{interval, sleep, MissedTickBehavior},
};

use crate::app::AppContext;

const DEFAULT_API_URL: &str = "http://localhost:9080";
const FLASH_TTL: Duration = Duration::from_secs(4);
const RENDER_INTERVAL: Duration = Duration::from_millis(100);
const MAX_EVENT_ROWS: usize = 200;

#[derive(Debug, Clone, Args)]
pub struct DashboardArgs {
    /// Base URL for the control-plane HTTP API.
    #[arg(long, default_value = DEFAULT_API_URL)]
    pub api_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab { Dashboard, Blocked, Events }
impl Tab {
    const ALL: [Self; 3] = [Self::Dashboard, Self::Blocked, Self::Events];
    fn next(self) -> Self { match self { Self::Dashboard => Self::Blocked, Self::Blocked => Self::Events, Self::Events => Self::Dashboard } }
    fn previous(self) -> Self { match self { Self::Dashboard => Self::Events, Self::Blocked => Self::Dashboard, Self::Events => Self::Blocked } }
    fn title(self) -> &'static str { match self { Self::Dashboard => "Dashboard", Self::Blocked => "Blocked", Self::Events => "Events" } }
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnectionState { Connected, Connecting, Error(String) }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlashKind { Success, Error }
#[derive(Debug, Clone)]
struct FlashMessage { text: String, kind: FlashKind, expires_at: Instant }
#[derive(Debug)]
enum DashboardUpdate { Server(DashboardServerEvent), Connection(ConnectionState) }
#[derive(Debug)]
enum DashboardServerEvent { Status(StatusResponse), Blocked(BlockedListResponse), EventLog(EventsResponse), Rules(RulesResponse) }
#[derive(Debug, PartialEq, Eq)]
enum UserAction { Approve(String), Deny(String), SetLevel(String) }
#[derive(Debug, PartialEq, Eq)]
struct SseFrame { event: String, data: String }
#[derive(Debug, Default)]
struct SseParser { buffer: String }
impl SseParser {
    fn push_chunk(&mut self, chunk: &str) -> Vec<SseFrame> {
        self.buffer.push_str(&chunk.replace("\r\n", "\n"));
        let mut frames = Vec::new();
        while let Some(i) = self.buffer.find("\n\n") {
            let raw = self.buffer[..i].to_string();
            self.buffer.drain(..i + 2);
            if let Some(f) = parse_sse_frame(&raw) { frames.push(f); }
        }
        frames
    }
}
#[derive(Debug, Clone, Copy)]
struct ReconnectBackoff { next_secs: u64 }
impl Default for ReconnectBackoff { fn default() -> Self { Self { next_secs: 1 } } }
impl ReconnectBackoff {
    fn next_delay(&mut self) -> Duration { let c = self.next_secs; self.next_secs = (self.next_secs.saturating_mul(2)).min(30); Duration::from_secs(c) }
    fn reset(&mut self) { self.next_secs = 1; }
}
#[derive(Debug, Clone)]
struct App { current_tab: Tab, status: StatusResponse, blocked: Vec<BlockedItem>, events: Vec<EventItem>, rules: Vec<RuleItem>, selected_index: usize, connection_state: ConnectionState, last_message: Option<FlashMessage>, should_quit: bool }
fn empty_status() -> StatusResponse { StatusResponse { security_level: "unknown".to_string(), pending_count: 0, recent_approvals: 0, events_count: 0 } }
impl App {
    fn new() -> Self { Self { current_tab: Tab::Dashboard, status: empty_status(), blocked: Vec::new(), events: Vec::new(), rules: Vec::new(), selected_index: 0, connection_state: ConnectionState::Connecting, last_message: None, should_quit: false } }
    fn apply_server_event(&mut self, event: DashboardServerEvent) { match event { DashboardServerEvent::Status(s) => self.status = s, DashboardServerEvent::Blocked(b) => { self.blocked = b.items; self.clamp_selection(); } DashboardServerEvent::EventLog(e) => { self.events = e.events.into_iter().take(MAX_EVENT_ROWS).collect(); self.clamp_selection(); } DashboardServerEvent::Rules(r) => self.rules = r.rules } }
    fn handle_key(&mut self, key: KeyEvent) -> Option<UserAction> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) { self.should_quit = true; return None; }
        match key.code {
            KeyCode::Char('q') => { self.should_quit = true; None }
            KeyCode::Tab => { self.current_tab = self.current_tab.next(); self.selected_index = 0; None }
            KeyCode::BackTab => { self.current_tab = self.current_tab.previous(); self.selected_index = 0; None }
            KeyCode::Char('j') | KeyCode::Down => { self.move_selection(1); None }
            KeyCode::Char('k') | KeyCode::Up => { self.move_selection(-1); None }
            KeyCode::Char('a') if self.current_tab == Tab::Blocked => self.selected_request_id().map(UserAction::Approve),
            KeyCode::Char('d') if self.current_tab == Tab::Blocked => self.selected_request_id().map(UserAction::Deny),
            KeyCode::Char('1') if self.current_tab == Tab::Dashboard => Some(UserAction::SetLevel("relaxed".to_string())),
            KeyCode::Char('2') if self.current_tab == Tab::Dashboard => Some(UserAction::SetLevel("balanced".to_string())),
            KeyCode::Char('3') if self.current_tab == Tab::Dashboard => Some(UserAction::SetLevel("strict".to_string())),
            _ => None,
        }
    }
    fn move_selection(&mut self, delta: isize) { let len = self.selection_len(); if len == 0 { self.selected_index = 0; return; } let mag = delta.unsigned_abs(); if delta.is_negative() { self.selected_index = self.selected_index.saturating_sub(mag); } else { self.selected_index = self.selected_index.saturating_add(mag).min(len.saturating_sub(1)); } }
    fn selection_len(&self) -> usize { match self.current_tab { Tab::Dashboard => 0, Tab::Blocked => self.blocked.len(), Tab::Events => self.events.len() } }
    fn clamp_selection(&mut self) { let len = self.selection_len(); if len == 0 { self.selected_index = 0; } else { self.selected_index = self.selected_index.min(len - 1); } }
    fn selected_request_id(&self) -> Option<String> { self.blocked.get(self.selected_index).map(|i| i.request_id.clone()) }
    fn set_connection_state(&mut self, state: ConnectionState) { self.connection_state = state; }
    fn flash(&mut self, kind: FlashKind, text: impl Into<String>) { self.last_message = Some(FlashMessage { text: text.into(), kind, expires_at: Instant::now() + FLASH_TTL }); }
    fn clear_expired_message(&mut self, now: Instant) { if self.last_message.as_ref().is_some_and(|m| m.expires_at <= now) { self.last_message = None; } }
}
fn visible_window(selected: usize, total: usize, max_rows: usize) -> std::ops::Range<usize> { if total == 0 || max_rows == 0 || total <= max_rows { return 0..total; } let max_start = total - max_rows; let start = selected.saturating_sub(max_rows / 2).min(max_start); start..(start + max_rows) }
fn connection_label(state: &ConnectionState) -> (&str, Style) { match state { ConnectionState::Connected => ("Connected", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)), ConnectionState::Connecting => ("Connecting...", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)), ConnectionState::Error(_) => ("Error", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)) } }
fn security_level_style(level: &str) -> Style { match level { "relaxed" => Style::default().fg(Color::Green), "balanced" => Style::default().fg(Color::Yellow), "strict" => Style::default().fg(Color::Red), _ => Style::default().fg(Color::Gray) } }
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
fn level_badge(name: &str, current: &str) -> Span<'static> { let style = security_level_style(name); if name == current { Span::styled(format!("[{name}]"), style.add_modifier(Modifier::BOLD)) } else { Span::styled(name.to_string(), Style::default().fg(Color::DarkGray)) } }
fn rule_action_style(action: &str) -> Style { match action { "allow" => Style::default().fg(Color::Green), "block" => Style::default().fg(Color::Red), "prompt" => Style::default().fg(Color::Yellow), _ => Style::default().fg(Color::Gray) } }
fn parse_sse_frame(raw: &str) -> Option<SseFrame> { let mut event = String::new(); let mut data = String::new(); for line in raw.lines() { if let Some(v) = line.strip_prefix("event:") { event = v.trim().to_string(); } else if let Some(v) = line.strip_prefix("data:") { data = v.trim().to_string(); } } if event.is_empty() && data.is_empty() { return None; } Some(SseFrame { event, data }) }
fn parse_dashboard_event(frame: &SseFrame) -> Result<Option<DashboardServerEvent>> { match frame.event.as_str() { "status" => Ok(Some(DashboardServerEvent::Status(serde_json::from_str(&frame.data).context("failed to parse status event")?))), "blocked" => Ok(Some(DashboardServerEvent::Blocked(serde_json::from_str(&frame.data).context("failed to parse blocked event")?))), "event_log" => Ok(Some(DashboardServerEvent::EventLog(serde_json::from_str(&frame.data).context("failed to parse events event")?))), "rules" => Ok(Some(DashboardServerEvent::Rules(serde_json::from_str(&frame.data).context("failed to parse rules event")?))), _ => Ok(None) } }
fn normalize_api_url(url: &str) -> String { url.trim_end_matches('/').to_string() }
fn classify_connection_error(error: &anyhow::Error) -> ConnectionState { if let Some(e) = error.downcast_ref::<reqwest::Error>() && (e.is_connect() || e.is_timeout()) { return ConnectionState::Connecting; } ConnectionState::Error(error.to_string()) }
fn render(frame: &mut Frame<'_>, app: &App) {
    let outer = Layout::vertical([Constraint::Length(3), Constraint::Min(10), Constraint::Length(1)]).split(frame.area());
    render_header(frame, outer[0], app);
    match app.current_tab { Tab::Dashboard => render_dashboard_tab(frame, outer[1], app), Tab::Blocked => render_blocked_tab(frame, outer[1], app), Tab::Events => render_events_tab(frame, outer[1], app) }
    render_footer(frame, outer[2], app);
}
fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let cols = Layout::horizontal([Constraint::Min(40), Constraint::Length(20)]).split(area);
    let titles: Vec<Line<'_>> = Tab::ALL.iter().map(|t| { let style = if *t == app.current_tab { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) }; Line::styled(t.title(), style) }).collect();
    frame.render_widget(Tabs::new(titles).block(Block::default().title("Polis Control Plane").borders(Borders::ALL)).highlight_style(Style::default().fg(Color::Cyan)), cols[0]);
    let (label, style) = connection_label(&app.connection_state);
    frame.render_widget(Paragraph::new(Line::from(Span::styled(label, style))).block(Block::default().title("Stream").borders(Borders::ALL)), cols[1]);
}
fn render_dashboard_tab(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Length(7), Constraint::Min(5)]).split(area);
    let current = &app.status.security_level;
    let badge_line = Line::from(vec![Span::styled(" Security: ", Style::default().add_modifier(Modifier::BOLD)), level_badge("relaxed", current), Span::raw("  "), level_badge("balanced", current), Span::raw("  "), level_badge("strict", current), Span::styled("  (press 1/2/3 to change)", Style::default().fg(Color::DarkGray))]);
    frame.render_widget(Paragraph::new(badge_line).block(Block::default().title("Policy").borders(Borders::ALL)), rows[0]);
    let card_cols = Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)]).split(rows[1]);
    render_stat_card(frame, card_cols[0], "Pending", app.status.pending_count, Color::Yellow);
    render_stat_card(frame, card_cols[1], "Approved", app.status.recent_approvals, Color::Green);
    render_stat_card(frame, card_cols[2], "Events", app.status.events_count, Color::Cyan);
    render_rules_panel(frame, rows[2], app);
}
fn render_stat_card(frame: &mut Frame<'_>, area: Rect, title: &str, value: usize, color: Color) {
    let text = vec![Line::from(""), Line::from(Span::styled(format!("{value}"), Style::default().fg(color).add_modifier(Modifier::BOLD))), Line::from(Span::styled(title, Style::default().fg(Color::DarkGray)))];
    frame.render_widget(Paragraph::new(text).alignment(ratatui::layout::Alignment::Center).block(Block::default().title(title).borders(Borders::ALL)), area);
}
fn render_rules_panel(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.rules.is_empty() { frame.render_widget(Paragraph::new(Line::from(Span::styled(" No auto-approve rules configured", Style::default().fg(Color::DarkGray)))).block(Block::default().title("Auto-Approve Rules").borders(Borders::ALL)), area); return; }
    let header = Row::new(vec![Cell::from("Pattern").style(Style::default().add_modifier(Modifier::BOLD)), Cell::from("Action").style(Style::default().add_modifier(Modifier::BOLD))]);
    let rows: Vec<Row<'_>> = app.rules.iter().map(|r| Row::new(vec![Cell::from(r.pattern.clone()), Cell::from(Span::styled(r.action.clone(), rule_action_style(&r.action)))])).collect();
    frame.render_widget(Table::new(rows, [Constraint::Percentage(70), Constraint::Percentage(30)]).header(header).block(Block::default().title("Auto-Approve Rules").borders(Borders::ALL)), area);
}
fn render_blocked_tab(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.blocked.is_empty() { frame.render_widget(Paragraph::new(Line::from(Span::styled(" No blocked requests", Style::default().fg(Color::DarkGray)))).block(Block::default().title("Blocked Requests").borders(Borders::ALL)), area); return; }
    let header = Row::new(vec![Cell::from("Request ID").style(Style::default().add_modifier(Modifier::BOLD)), Cell::from("Destination").style(Style::default().add_modifier(Modifier::BOLD)), Cell::from("Reason").style(Style::default().add_modifier(Modifier::BOLD)), Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD))]);
    let inner_height = area.height.saturating_sub(3) as usize;
    let window = visible_window(app.selected_index, app.blocked.len(), inner_height);
    let rows: Vec<Row<'_>> = app.blocked[window.clone()].iter().enumerate().map(|(i, item)| { let g = window.start + i; let style = if g == app.selected_index { Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD) } else { Style::default() }; Row::new(vec![Cell::from(truncate_string(&item.request_id, 16)), Cell::from(truncate_string(&item.destination, 30)), Cell::from(item.reason.clone()), Cell::from(item.status.clone())]).style(style) }).collect();
    let table = Table::new(rows, [Constraint::Length(18), Constraint::Min(20), Constraint::Length(24), Constraint::Length(10)]).header(header).block(Block::default().title(format!("Blocked Requests ({})", app.blocked.len())).borders(Borders::ALL));
    let mut ts = ratatui::widgets::TableState::default();
    if !app.blocked.is_empty() { ts.select(Some(app.selected_index - window.start)); }
    frame.render_stateful_widget(table, area, &mut ts);
}
fn render_events_tab(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if app.events.is_empty() { frame.render_widget(Paragraph::new(Line::from(Span::styled(" No events recorded", Style::default().fg(Color::DarkGray)))).block(Block::default().title("Event Log").borders(Borders::ALL)), area); return; }
    let header = Row::new(vec![Cell::from("Time").style(Style::default().add_modifier(Modifier::BOLD)), Cell::from("Type").style(Style::default().add_modifier(Modifier::BOLD)), Cell::from("Details").style(Style::default().add_modifier(Modifier::BOLD))]);
    let inner_height = area.height.saturating_sub(3) as usize;
    let window = visible_window(app.selected_index, app.events.len(), inner_height);
    let rows: Vec<Row<'_>> = app.events[window.clone()].iter().enumerate().map(|(i, item)| { let g = window.start + i; let style = if g == app.selected_index { Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD) } else { Style::default() }; Row::new(vec![Cell::from(item.timestamp.format("%H:%M:%S").to_string()), Cell::from(item.event_type.clone()), Cell::from(truncate_string(&item.details, 60))]).style(style) }).collect();
    let table = Table::new(rows, [Constraint::Length(10), Constraint::Length(20), Constraint::Min(30)]).header(header).block(Block::default().title(format!("Event Log ({})", app.events.len())).borders(Borders::ALL));
    let mut ts = ratatui::widgets::TableState::default();
    if !app.events.is_empty() { ts.select(Some(app.selected_index - window.start)); }
    frame.render_stateful_widget(table, area, &mut ts);
}
fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if let Some(message) = &app.last_message {
        let style = match message.kind { FlashKind::Success => Style::default().fg(Color::Green), FlashKind::Error => Style::default().fg(Color::Red) };
        frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(" > ", Style::default().add_modifier(Modifier::BOLD)), Span::styled(message.text.as_str(), style)])), area);
    } else {
        let hints = match app.current_tab { Tab::Dashboard => "1 relaxed | 2 balanced | 3 strict | Tab switch | q quit", Tab::Blocked => "a approve | d deny | j/k navigate | Tab switch | q quit", Tab::Events => "j/k navigate | Tab switch | q quit" };
        frame.render_widget(Paragraph::new(Line::from(Span::styled(format!(" {hints}"), Style::default().fg(Color::DarkGray)))), area);
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
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;
    Ok(())
}
async fn approve_request(client: &Client, api_url: &str, id: &str) -> Result<ActionResponse> { send_action(client.post(format!("{api_url}/api/v1/blocked/{id}/approve")).header(reqwest::header::CONTENT_TYPE, "application/json")).await }
async fn deny_request(client: &Client, api_url: &str, id: &str) -> Result<ActionResponse> { send_action(client.post(format!("{api_url}/api/v1/blocked/{id}/deny")).header(reqwest::header::CONTENT_TYPE, "application/json")).await }
async fn set_security_level(client: &Client, api_url: &str, level: &str) -> Result<ActionResponse> {
    let resp = client.put(format!("{api_url}/api/v1/config/level")).header(reqwest::header::CONTENT_TYPE, "application/json").json(&serde_json::json!({ "level": level })).send().await.context("failed to set security level")?;
    let status = resp.status();
    if status.is_success() { let lr = resp.json::<LevelResponse>().await.context("failed to decode level response")?; return Ok(ActionResponse { message: format!("Security level set to {}", lr.level) }); }
    Err(anyhow!(parse_error_response(resp).await.unwrap_or_else(|_| format!("failed (status {status})"))))
}
async fn send_action(request: reqwest::RequestBuilder) -> Result<ActionResponse> {
    let resp = request.send().await.context("failed to send request")?;
    let status = resp.status();
    if status.is_success() { return resp.json::<ActionResponse>().await.context("failed to decode response"); }
    Err(anyhow!(parse_error_response(resp).await.unwrap_or_else(|_| format!("request failed (status {status})"))))
}
async fn parse_error_response(response: reqwest::Response) -> Result<String> {
    let status = response.status();
    let text = response.text().await.context("failed to read error response")?;
    if let Ok(e) = serde_json::from_str::<ErrorResponse>(&text) { return Ok(e.error); }
    if text.trim().is_empty() { Ok(format!("request failed with status {status}")) } else { Ok(text) }
}
async fn fetch_rules(client: &Client, api_url: &str) -> Result<RulesResponse> { client.get(format!("{api_url}/api/v1/config/rules")).send().await.context("failed to fetch rules")?.json::<RulesResponse>().await.context("failed to decode rules") }
fn spawn_sse_task(client: Client, api_url: String, sender: mpsc::UnboundedSender<DashboardUpdate>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = ReconnectBackoff::default();
        loop {
            let _ = sender.send(DashboardUpdate::Connection(ConnectionState::Connecting));
            match connect_sse(&client, &api_url, &sender).await { Ok(()) => backoff.reset(), Err(e) => { let _ = sender.send(DashboardUpdate::Connection(classify_connection_error(&e))); } }
            sleep(backoff.next_delay()).await;
        }
    })
}
async fn connect_sse(client: &Client, api_url: &str, sender: &mpsc::UnboundedSender<DashboardUpdate>) -> Result<()> {
    if let Ok(rules) = fetch_rules(client, api_url).await { let _ = sender.send(DashboardUpdate::Server(DashboardServerEvent::Rules(rules))); }
    let response = client.get(format!("{api_url}/api/v1/stream")).header(reqwest::header::ACCEPT, "text/event-stream").send().await.context("failed to connect to SSE endpoint")?.error_for_status().context("SSE endpoint returned error")?;
    let _ = sender.send(DashboardUpdate::Connection(ConnectionState::Connected));
    let mut parser = SseParser::default();
    let mut byte_stream = response.bytes_stream();
    let mut rules_tick = interval(Duration::from_secs(5));
    rules_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        select! {
            maybe_chunk = byte_stream.next() => { match maybe_chunk { Some(Ok(chunk)) => { for frame in parser.push_chunk(&String::from_utf8_lossy(&chunk)) { if let Some(event) = parse_dashboard_event(&frame)? { let _ = sender.send(DashboardUpdate::Server(event)); } } } Some(Err(e)) => return Err(e.into()), None => return Ok(()) } }
            _ = rules_tick.tick() => { if let Ok(rules) = fetch_rules(client, api_url).await { let _ = sender.send(DashboardUpdate::Server(DashboardServerEvent::Rules(rules))); } }
        }
    }
}
/// Run the interactive control-plane dashboard.
/// # Errors
/// Returns an error if the dashboard terminal, HTTP client, or event loop fails.
pub async fn run(args: &DashboardArgs, app: &AppContext) -> Result<ExitCode> {
    if app.is_json() { bail!("dashboard does not support --json output"); }
    let api_url = normalize_api_url(&args.api_url);
    let client = Client::builder().connect_timeout(Duration::from_secs(5)).build().context("failed to build control-plane HTTP client")?;
    let mut terminal = init_terminal()?;
    let result = run_dashboard_loop(&mut terminal, client, api_url).await;
    let restore_result = restore_terminal(&mut terminal);
    restore_result?; result?;
    Ok(ExitCode::SUCCESS)
}
async fn run_dashboard_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, client: Client, api_url: String) -> Result<()> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let sse_task = spawn_sse_task(client.clone(), api_url.clone(), sender);
    let mut app = App::new();
    let mut key_events = EventStream::new();
    let mut render_tick = interval(RENDER_INTERVAL);
    render_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        terminal.draw(|frame| render(frame, &app))?;
        select! {
            maybe_update = receiver.recv() => { match maybe_update { Some(DashboardUpdate::Server(event)) => app.apply_server_event(event), Some(DashboardUpdate::Connection(state)) => app.set_connection_state(state), None => { app.set_connection_state(ConnectionState::Error("channel closed".to_string())); app.should_quit = true; } } }
            maybe_event = key_events.next() => {
                match maybe_event {
                    Some(Ok(CrosstermEvent::Key(key))) if key.kind == KeyEventKind::Press => {
                        if let Some(action) = app.handle_key(key) {
                            let result = match action { UserAction::Approve(id) => approve_request(&client, &api_url, &id).await, UserAction::Deny(id) => deny_request(&client, &api_url, &id).await, UserAction::SetLevel(level) => set_security_level(&client, &api_url, &level).await };
                            match result { Ok(r) => app.flash(FlashKind::Success, r.message), Err(e) => app.flash(FlashKind::Error, e.to_string()) }
                        }
                    }
                    Some(Ok(_)) => {} Some(Err(e)) => app.flash(FlashKind::Error, format!("terminal input error: {e}")),
                    None => { app.set_connection_state(ConnectionState::Error("input stream closed".to_string())); app.should_quit = true; }
                }
            }
            _ = render_tick.tick() => { app.clear_expired_message(Instant::now()); }
        }
        if app.should_quit { break; }
    }
    sse_task.abort();
    Ok(())
}
#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use super::*;
    fn sample_blocked(id: &str) -> BlockedItem { BlockedItem { request_id: id.to_string(), reason: "credential_detected".to_string(), destination: "https://example.com".to_string(), blocked_at: Utc.with_ymd_and_hms(2026, 3, 6, 12, 0, 0).single().expect("timestamp"), status: "pending".to_string() } }
    fn sample_event() -> EventItem { EventItem { timestamp: Utc.with_ymd_and_hms(2026, 3, 6, 12, 0, 0).single().expect("timestamp"), event_type: "block_reported".to_string(), request_id: Some("req-abc12345".to_string()), details: "Blocked request".to_string() } }
    fn sample_rule(pattern: &str, action: &str) -> RuleItem { RuleItem { pattern: pattern.to_string(), action: action.to_string() } }
    #[test]
    fn app_new_uses_expected_defaults() { let app = App::new(); assert_eq!(app.current_tab, Tab::Dashboard); assert_eq!(app.status.pending_count, 0); assert!(app.blocked.is_empty()); assert!(app.events.is_empty()); assert!(app.rules.is_empty()); assert_eq!(app.selected_index, 0); assert_eq!(app.connection_state, ConnectionState::Connecting); assert!(app.last_message.is_none()); assert!(!app.should_quit); }
    #[test]
    fn tab_switching_cycles_through_all_tabs() { let mut app = App::new(); assert_eq!(app.current_tab, Tab::Dashboard); app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); assert_eq!(app.current_tab, Tab::Blocked); app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); assert_eq!(app.current_tab, Tab::Events); app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); assert_eq!(app.current_tab, Tab::Dashboard); }
    #[test]
    fn tab_switching_resets_selection() { let mut app = App::new(); app.blocked = vec![sample_blocked("req-1"), sample_blocked("req-2")]; app.current_tab = Tab::Blocked; app.selected_index = 1; app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); assert_eq!(app.current_tab, Tab::Events); assert_eq!(app.selected_index, 0); }
    #[test]
    fn key_navigation_and_actions_update_state() { let mut app = App::new(); app.current_tab = Tab::Blocked; app.blocked = vec![sample_blocked("req-1"), sample_blocked("req-2")]; app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)); assert_eq!(app.selected_index, 1); let action = app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)); assert_eq!(action, Some(UserAction::Approve("req-2".to_string()))); app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)); assert!(app.should_quit); }
    #[test]
    fn security_level_keys_produce_set_level_actions() { let mut app = App::new(); assert_eq!(app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)), Some(UserAction::SetLevel("relaxed".to_string()))); assert_eq!(app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)), Some(UserAction::SetLevel("balanced".to_string()))); assert_eq!(app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE)), Some(UserAction::SetLevel("strict".to_string()))); }
    #[test]
    fn security_level_keys_ignored_on_other_tabs() { let mut app = App::new(); app.current_tab = Tab::Blocked; assert_eq!(app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)), None); app.current_tab = Tab::Events; assert_eq!(app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)), None); }
    #[test]
    fn apply_server_events_replaces_snapshots() { let mut app = App::new(); app.apply_server_event(DashboardServerEvent::Status(StatusResponse { security_level: "strict".to_string(), pending_count: 2, recent_approvals: 4, events_count: 8 })); app.apply_server_event(DashboardServerEvent::Blocked(BlockedListResponse { items: vec![sample_blocked("req-1")] })); app.apply_server_event(DashboardServerEvent::EventLog(EventsResponse { events: vec![sample_event()] })); app.apply_server_event(DashboardServerEvent::Rules(RulesResponse { rules: vec![sample_rule("*.example.com", "allow")] })); assert_eq!(app.status.security_level, "strict"); assert_eq!(app.blocked.len(), 1); assert_eq!(app.events.len(), 1); assert_eq!(app.rules.len(), 1); }
    #[test]
    fn sse_parser_handles_chunk_boundaries() { let status_json = serde_json::to_string(&StatusResponse { security_level: "balanced".to_string(), pending_count: 1, recent_approvals: 2, events_count: 3 }).expect("json"); let blocked_json = serde_json::to_string(&BlockedListResponse { items: vec![sample_blocked("req-1")] }).expect("json"); let mut parser = SseParser::default(); let first = parser.push_chunk(&format!("event: status\ndata: {status_json}\n\n")); let second = parser.push_chunk(&format!("event: blocked\ndata: {blocked_json}\n")); let third = parser.push_chunk("\n"); assert_eq!(first.len(), 1); assert_eq!(first[0].event, "status"); assert!(second.is_empty()); assert_eq!(third.len(), 1); assert_eq!(third[0].event, "blocked"); }
    #[test]
    fn parse_dashboard_event_decodes_rules_payload() { let frame = SseFrame { event: "rules".to_string(), data: serde_json::to_string(&RulesResponse { rules: vec![sample_rule("*.test.com", "block")] }).expect("json") }; match parse_dashboard_event(&frame).expect("parse") { Some(DashboardServerEvent::Rules(p)) => assert_eq!(p.rules.len(), 1), _ => panic!("unexpected event type") } }
    #[test]
    fn reconnect_backoff_doubles_and_caps() { let mut b = ReconnectBackoff::default(); let d: Vec<u64> = (0..7).map(|_| b.next_delay().as_secs()).collect(); assert_eq!(d, [1, 2, 4, 8, 16, 30, 30]); b.reset(); assert_eq!(b.next_delay(), Duration::from_secs(1)); }
    #[test]
    fn truncate_string_works() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("a long string here", 10), "a long ...");
        assert_eq!(truncate_string("emoji🙂text", 8), "emoji...");
        assert_eq!(truncate_string("truncate", 3), "...");
    }
    #[test]
    fn normalize_api_url_strips_trailing_slash() { assert_eq!(normalize_api_url("http://localhost:9080/"), "http://localhost:9080"); assert_eq!(normalize_api_url("http://localhost:9080"), "http://localhost:9080"); }
    #[test]
    fn visible_window_handles_edge_cases() { assert_eq!(visible_window(0, 0, 10), 0..0); assert_eq!(visible_window(0, 5, 10), 0..5); assert_eq!(visible_window(0, 20, 10), 0..10); assert_eq!(visible_window(15, 20, 10), 10..20); }
    #[test]
    fn deny_action_on_blocked_tab() { let mut app = App::new(); app.current_tab = Tab::Blocked; app.blocked = vec![sample_blocked("req-1")]; assert_eq!(app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)), Some(UserAction::Deny("req-1".to_string()))); }
    #[test]
    fn ctrl_c_quits() { let mut app = App::new(); app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)); assert!(app.should_quit); }
}
