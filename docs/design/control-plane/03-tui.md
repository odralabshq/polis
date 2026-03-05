# Control Plane — TUI Dashboard Specification

## Delivery: `polis dashboard` Subcommand

The TUI is a subcommand of the existing `polis` CLI binary — not a separate binary. This means:
- No separate crate, build, or release artifact
- Users run `polis dashboard` the same way they run `polis status` or `polis security pending`
- The CLI already handles cross-compilation for Linux and Windows in `release.yml`
- Installation scripts (`install.sh`, `install.ps1`) need zero changes

### CLI Integration

Add to `cli/src/commands/mod.rs`:
```rust
pub mod dashboard;
```

Add to `cli/src/cli.rs`:
```rust
/// Live security dashboard
Dashboard(commands::dashboard::DashboardArgs),
```

```
polis dashboard [OPTIONS]

Options:
  --api-url <URL>    Control plane API URL [default: http://localhost:9080]
  -h, --help         Print help
```

### Source Location

```
cli/
├── Cargo.toml                    # add ratatui, crossterm, reqwest deps
└── src/commands/
    └── dashboard.rs              # all TUI code in one module
```

The dashboard command is self-contained in a single file. It uses `cp-api-types` for response deserialization and `reqwest` for HTTP + SSE streaming.

## Technology

| Component | Choice | Rationale |
|---|---|---|
| TUI framework | ratatui 0.30 | 18.8k stars, immediate-mode fits SSE-driven updates |
| Terminal backend | crossterm | Cross-platform (Linux/macOS/Windows) |
| Async runtime | tokio | Already used by the CLI |
| HTTP + SSE client | reqwest | Async, streaming response support |
| API types | cp-api-types | Shared with cp-server |

## Architecture

SSE-driven event loop using `tokio::select!`:

```
┌──────────────────────────────────────────────────┐
│                  Event Loop                       │
│                                                   │
│  tokio::select! {                                 │
│    key_event = crossterm::event::read()           │
│      → handle_key(key) → update app state         │
│                                                   │
│    sse_event = sse_stream.next()                  │
│      → match event.type:                          │
│          "status"    → update status panel         │
│          "blocked"   → update blocked table        │
│          "event_log" → prepend to event list       │
│                                                   │
│    _ = render_timer.tick() (every 100ms)          │
│      → terminal.draw(|f| ui::render(f, &app))    │
│  }                                                │
└──────────────────────────────────────────────────┘
```

No polling timer — the SSE stream from `GET /api/v1/stream` pushes all updates. The render timer redraws the UI at ~10fps for smooth interaction. Key events trigger HTTP POST/PUT/DELETE calls for mutations (approve, deny).

## Application State

```rust
pub struct App {
    pub current_tab: Tab,
    pub status: Option<StatusResponse>,
    pub blocked: Vec<BlockedItem>,
    pub events: Vec<EventItem>,
    pub selected_index: usize,
    pub connection_state: ConnectionState,
    pub last_message: Option<(String, Instant)>,  // flash message + expiry
    pub should_quit: bool,
}

pub enum Tab { Dashboard, Blocked, Events }

pub enum ConnectionState { Connected, Connecting, Error(String) }
```

## Layout

### Dashboard Tab (default)

```
┌─ Polis Control Plane ──────────────────────────────────┐
│ [Dashboard]  Blocked  Events                           │
├────────────────────────────────────────────────────────┤
│                                                        │
│  Security Level: ██ BALANCED                           │
│                                                        │
│  ┌─ Pending ──┐  ┌─ Approved ─┐  ┌─ Events ──┐       │
│  │     3      │  │     12     │  │    47     │       │
│  └────────────┘  └────────────┘  └───────────┘       │
│                                                        │
├────────────────────────────────────────────────────────┤
│ ● Connected │ q:quit Tab:switch                        │
└────────────────────────────────────────────────────────┘
```

### Blocked Requests Tab

```
┌─ Polis Control Plane ──────────────────────────────────┐
│  Dashboard  [Blocked]  Events                          │
├────────────────────────────────────────────────────────┤
│ ID           │ Reason              │ Destination       │
│──────────────┼─────────────────────┼───────────────────│
│▸req-abc12345 │ credential_detected │ api.example.com   │
│ req-def67890 │ url_blocked         │ evil.com          │
│ req-11223344 │ credential_detected │ upload.io         │
│                                                        │
├────────────────────────────────────────────────────────┤
│ a:approve  d:deny  j/k:navigate  q:quit               │
└────────────────────────────────────────────────────────┘
```

### Events Tab

```
┌─ Polis Control Plane ──────────────────────────────────┐
│  Dashboard  Blocked  [Events]                          │
├────────────────────────────────────────────────────────┤
│ 19:05:32  block_reported    req-abc12345  api.example… │
│ 19:04:11  approved_via_cli  req-def67890  evil.com     │
│ 19:03:45  block_reported    req-def67890  evil.com     │
│ 19:01:02  level_changed     —             strict→bala… │
│                                                        │
├────────────────────────────────────────────────────────┤
│ j/k:scroll  q:quit                                     │
└────────────────────────────────────────────────────────┘
```

## Keybindings

| Key | Context | Action |
|---|---|---|
| `Tab` | Global | Next tab |
| `Shift+Tab` | Global | Previous tab |
| `q` / `Ctrl+C` | Global | Quit |
| `j` / `↓` | Blocked, Events | Move selection down |
| `k` / `↑` | Blocked, Events | Move selection up |
| `a` | Blocked tab | Approve selected request (POST) |
| `d` | Blocked tab | Deny selected request (POST) |

## SSE Client

```rust
// Inside dashboard.rs
async fn connect_sse(api_url: &str) -> Result<impl Stream<Item = SseEvent>> {
    let url = format!("{}/api/v1/stream", api_url);
    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await?;
    // Parse text/event-stream frames from the byte stream
    Ok(parse_sse_stream(response.bytes_stream()))
}
```

On connection loss, the TUI sets `ConnectionState::Connecting` and retries with exponential backoff (1s, 2s, 4s, max 30s). On reconnect, the server sends a full state snapshot as the first SSE events.

## Color Scheme

| Element | Color |
|---|---|
| Header/borders | White |
| Tab active | Bold Cyan |
| Tab inactive | Dark Gray |
| Status: connected | Green |
| Status: error | Red |
| Blocked request (pending) | Yellow |
| Approved | Green |
| Denied/blocked reason | Red |
| Security level badge | Cyan |
| Footer keybindings | Dark Gray |

## Error Handling

- **Connection refused:** Show `● Connecting...` in footer, retry with exponential backoff
- **SSE stream dropped:** Automatic reconnect, same backoff strategy
- **Approve/deny failure:** Flash error message in footer for 3 seconds
- **Terminal resize:** Handled automatically by ratatui layout constraints

## CLI Dependencies

Add to `cli/Cargo.toml`:

```toml
ratatui = { version = "0.30", optional = true }
crossterm = { version = "0.28", optional = true }
reqwest = { version = "0.12", features = ["stream"], optional = true }
cp-api-types = { path = "../services/control-plane/crates/cp-api-types", optional = true }

[features]
dashboard = ["ratatui", "crossterm", "reqwest", "cp-api-types"]
```

The `dashboard` feature keeps these dependencies optional — users who don't need the TUI don't pay the compile cost. The feature is enabled by default in release builds.
