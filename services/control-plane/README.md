# Polis Control Plane

Phase 1 of the Polis control plane provides a governance-focused REST API + SSE
backend, a lightweight embedded web UI, and the `polis dashboard` TUI command.

## Crates

- **cp-api-types** — shared API request/response types used by both the server
  and the CLI dashboard.
- **cp-server** — HTTP/SSE server that reads and mutates governance state in
  Valkey.

## Architecture

The control-plane server (`cp-server`) listens on a single HTTP port and serves:

- **Web dashboard** — embedded HTML/JS UI at `/` for browser-based governance.
- **REST API** — JSON endpoints under `/api/v1/*` for programmatic access.
- **SSE stream** — `/api/v1/stream` pushes real-time state updates to connected
  clients (dashboard, TUI, or custom integrations).

### Workspace & Agent Detection

The control-plane uses **Docker container introspection** (not a registration or
heartbeat protocol) to detect the workspace and any hosted agent. The workspace
is a universal container environment that may host different kinds of agents.

Docker API access is mediated through a **socket proxy** (`docker-proxy`) that
restricts the control-plane to read-only operations plus `SIGHUP` signaling.
The control-plane never mounts the Docker socket directly.

```
control-plane ──TCP:2375──► docker-proxy ──unix socket──► Docker daemon
                            (HAProxy filter)
Allows:  GET /containers/*, GET /networks/*, GET /_ping,
         POST /containers/{id}/kill (SIGHUP only)
Blocks:  exec, create, start, pull, secrets, images, volumes, etc.
```

The `DOCKER_HOST=tcp://docker-proxy:2375` environment variable tells the
Bollard client to connect via the proxy instead of the local socket. When
`POLIS_CP_DOCKER_ENABLED=false`, the Docker client is never instantiated and
the proxy sits idle.

- `/api/v1/workspace` — returns workspace container status (uptime, container
  health summary, network info) detected via the Docker API.
- `/api/v1/agent` — returns agent name, version, display name, health, and
  resource usage detected from Docker labels and health checks on the workspace
  container.
- `/api/v1/containers` — returns detailed per-container status for all services
  in the Polis compose stack.

Agent metadata (name, version, display name) is read from Docker labels on the
workspace container. If Docker introspection is disabled or fails, the API
returns default values with `"detected": false` in the agent config response.

### Security Levels

The control-plane enforces one of three security levels, configured globally
via the governance state in Valkey:

| Level | Behavior |
|---|---|
| **relaxed** | New domains are auto-approved; only known-malicious destinations and credential leaks are blocked. |
| **balanced** | New, unseen domains trigger a human-in-the-loop prompt; known-safe domains pass through. |
| **strict** | All outbound traffic to new destinations is blocked until explicitly approved. |

The active level can be read and changed via the `/api/v1/config/level`
endpoint or through the dashboard UI.

### Authentication & RBAC

Authentication is controlled by `POLIS_CP_AUTH_ENABLED`. When disabled (the
default), all requests receive `admin` privileges.

> **Warning:** Running with authentication disabled grants full admin access to
> every API consumer. Set `POLIS_CP_AUTH_ENABLED=true` and configure token
> secrets before exposing the control-plane outside localhost.

When enabled, the server reads Bearer tokens from the `Authorization` header
(or `?token=` query parameter for SSE connections) and validates them against
tokens seeded from secret files at startup.

**Roles:**

| Role | Permissions |
|---|---|
| **admin** | Full access — read, mutate governance, mutate config |
| **operator** | Read dashboard + blocked list + level; mutate governance |
| **viewer** | Read-only access to dashboard, blocked list, and level |
| **agent** | Read blocked list and security level only |

To enable authentication:

1. Generate token files (one per role) and mount them as Docker secrets.
2. Set `POLIS_CP_AUTH_ENABLED=true`.
3. Uncomment the secret references in `docker-compose.yml`.

## Configuration Reference

All configuration is via environment variables prefixed with `POLIS_CP_`.

| Variable | Default | Description |
|---|---|---|
| `POLIS_CP_LISTEN_ADDR` | `0.0.0.0:9080` | HTTP listen address |
| `POLIS_CP_VALKEY_URL` | `rediss://valkey:6379` | Valkey connection URL (TLS) |
| `POLIS_CP_VALKEY_USER` | `cp-server` | Valkey ACL username |
| `POLIS_CP_VALKEY_PASS_FILE` | `/run/secrets/valkey_cp_server_password` | Path to Valkey password file |
| `POLIS_CP_VALKEY_CA` | `/etc/valkey/tls/ca.crt` | Valkey TLS CA certificate |
| `POLIS_CP_VALKEY_CLIENT_CERT` | `/etc/valkey/tls/client.crt` | Valkey mTLS client certificate |
| `POLIS_CP_VALKEY_CLIENT_KEY` | `/etc/valkey/tls/client.key` | Valkey mTLS client key |
| `POLIS_CP_DOCKER_ENABLED` | `false` | Enable Docker API introspection for workspace/agent detection |
| `POLIS_CP_AUTH_ENABLED` | `false` | Enable Bearer token authentication and RBAC |
| `POLIS_CP_ADMIN_TOKEN_FILE` | `/run/secrets/cp_admin_token` | Path to admin role token file |
| `POLIS_CP_OPERATOR_TOKEN_FILE` | `/run/secrets/cp_operator_token` | Path to operator role token file |
| `POLIS_CP_VIEWER_TOKEN_FILE` | `/run/secrets/cp_viewer_token` | Path to viewer role token file |
| `POLIS_CP_AGENT_TOKEN_FILE` | `/run/secrets/cp_agent_token` | Path to agent role token file |
| `POLIS_CP_CORS_ORIGINS` | `http://localhost:9080,http://127.0.0.1:9080` | Comma-separated list of allowed CORS origins |

## API Endpoints

### Read-only

| Method | Path | Permission | Description |
|---|---|---|---|
| GET | `/health` | — | Health check (returns 200 OK) |
| GET | `/` | — | Embedded web dashboard |
| GET | `/api/v1/status` | ReadDashboard | Governance status summary |
| GET | `/api/v1/workspace` | ReadDashboard | Workspace container status |
| GET | `/api/v1/agent` | ReadDashboard | Detected agent info |
| GET | `/api/v1/containers` | ReadDashboard | All container statuses |
| GET | `/api/v1/blocked` | ReadBlocked | Pending blocked requests |
| GET | `/api/v1/events?limit=N` | ReadDashboard | Recent security events |
| GET | `/api/v1/config/level` | ReadLevel | Current security level |
| GET | `/api/v1/config/rules` | ReadDashboard | Auto-approve rules |
| GET | `/api/v1/stream` | ReadDashboard | SSE event stream (max 30 concurrent) |

### Mutations (rate-limited at 30 req/s)

| Method | Path | Permission | Description |
|---|---|---|---|
| POST | `/api/v1/blocked/{id}/approve` | MutateGovernance | Approve a blocked request |
| POST | `/api/v1/blocked/{id}/deny` | MutateGovernance | Deny a blocked request |
| POST | `/api/v1/blocked/{id}/allow-credential` | MutateGovernance | Allow a detected credential |
| POST | `/api/v1/blocked/{id}/bypass-domain` | MutateGovernance | Bypass a blocked domain |
| PUT | `/api/v1/config/level` | MutateConfig | Set security level |
| POST | `/api/v1/config/rules` | MutateConfig | Add auto-approve rule |
| DELETE | `/api/v1/config/rules?pattern=...` | MutateConfig | Delete auto-approve rule |
