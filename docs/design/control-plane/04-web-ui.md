# Control Plane — Web UI Specification

## Approach

Single `index.html` file with embedded `<style>` and `<script>` blocks. No build toolchain, no npm, no bundler. Served from the cp-server binary via `include_str!("../web/index.html")` with `Content-Type: text/html`.

**Rationale:** Keeps the Docker image small, eliminates frontend build complexity, and is trivially replaceable with a WASM frontend (Leptos/Yew) in the future without any backend changes — the REST API is the contract.

## Serving

```rust
// cp-server/src/main.rs
const INDEX_HTML: &str = include_str!("../web/index.html");

async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(INDEX_HTML)
}

// Router
let app = Router::new()
    .route("/", get(index))
    .nest("/api", api_routes)
    .route("/health", get(health));
```

## Page Structure

```html
<body>
  <header>
    <h1>Polis Control Plane</h1>
    <div id="connection-status">● Connected</div>
  </header>

  <nav>
    <button data-tab="dashboard" class="active">Dashboard</button>
    <button data-tab="blocked">Blocked Requests</button>
    <button data-tab="events">Event Log</button>
    <button data-tab="rules">Rules</button>
  </nav>

  <main>
    <section id="dashboard">...</section>
    <section id="blocked" hidden>...</section>
    <section id="events" hidden>...</section>
    <section id="rules" hidden>...</section>
  </main>

  <footer>
    <span>Polis v0.4.0</span>
    <span id="last-update">Updated: just now</span>
  </footer>
</body>
```

## Sections

### Dashboard

- Security level badge (colored: green=relaxed, yellow=balanced, red=strict)
- Three stat cards: Pending Blocked, Recent Approvals, Total Events
- Auto-refreshes every 3 seconds via `GET /api/v1/status`

### Blocked Requests

- Table: ID, Reason, Destination, Blocked At
- Each row has Approve and Deny buttons
- Approve → `POST /api/v1/blocked/{id}/approve` → refresh table
- Deny → `POST /api/v1/blocked/{id}/deny` → refresh table
- Empty state: "No pending blocked requests ✓"
- Auto-refreshes every 3 seconds via `GET /api/v1/blocked`

### Event Log

- Reverse-chronological list of security events
- Columns: Time, Type, Request ID, Details
- Loaded via `GET /api/v1/events?limit=100`
- Manual refresh button (no auto-refresh — events are historical)

### Rules

- Table of auto-approve rules: Pattern, Action
- Delete button per row → `DELETE /api/v1/config/rules?pattern=...`
- Add form: pattern input + action dropdown (allow/prompt/block) + Add button
- Add → `POST /api/v1/config/rules` → refresh table
- Loaded via `GET /api/v1/config/rules`

## Styling

Dark theme using CSS custom properties:

```css
:root {
  --bg: #0d1117;
  --bg-card: #161b22;
  --bg-hover: #1c2128;
  --border: #30363d;
  --text: #e6edf3;
  --text-muted: #7d8590;
  --accent: #58a6ff;
  --success: #3fb950;
  --warning: #d29922;
  --danger: #f85149;
}
```

- Responsive grid layout using CSS Grid
- Cards with subtle borders and rounded corners
- Buttons with hover states
- Monospace font for request IDs and timestamps
- Mobile-friendly: single column below 768px

## JavaScript

Vanilla JS, no framework. Key patterns:

```javascript
// SSE for real-time updates (replaces polling)
const source = new EventSource('/api/v1/stream');

source.addEventListener('status', (e) => {
  const data = JSON.parse(e.data);
  updateDashboard(data);
});

source.addEventListener('blocked', (e) => {
  const data = JSON.parse(e.data);
  renderBlockedTable(data.items);
});

source.addEventListener('event_log', (e) => {
  const data = JSON.parse(e.data);
  prependEvent(data);
});

source.onerror = () => {
  setConnectionStatus('disconnected');
  // EventSource reconnects automatically
};

source.onopen = () => {
  setConnectionStatus('connected');
};

// Mutations still use fetch()
async function approveRequest(id) {
  const res = await fetch(`/api/v1/blocked/${id}/approve`, { method: 'POST' });
  if (!res.ok) { showError(await res.text()); }
  // No need to refresh — SSE will push the updated state
}
```

- `EventSource` for all live data (status, blocked list, events) — no `setInterval` polling
- `fetch()` for mutations (approve, deny, add/delete rules)
- DOM manipulation via `document.getElementById` / `innerHTML`
- Error display: toast-style notification that auto-dismisses after 5s
- Connection status indicator: green dot when SSE connected, red on `onerror`
- `EventSource` handles reconnection automatically with exponential backoff

## Accessibility

- Semantic HTML: `<header>`, `<nav>`, `<main>`, `<section>`, `<table>`, `<footer>`
- ARIA labels on buttons: `aria-label="Approve request req-abc12345"`
- Keyboard navigation: Tab through interactive elements, Enter to activate
- Focus indicators on all interactive elements
- Color is not the only indicator — text labels accompany colored badges
- `prefers-color-scheme` media query (dark is default, light theme future)

## Future

The web UI is intentionally minimal. When the control plane grows to need complex state management, routing, or real-time updates, the `index.html` can be replaced with a compiled WASM frontend (Leptos or Yew) without any backend changes — the REST API contract remains the same. The `include_str!` approach would simply point to the WASM build output instead.
