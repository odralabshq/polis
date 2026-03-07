# Polis Control Plane

Phase 1 of the Polis control plane adds a governance-focused REST API + SSE
backend, a lightweight embedded web UI, and the `polis dashboard` TUI command.

This workspace currently contains:

- `cp-api-types` — shared API response/request types for the control-plane
  server and CLI dashboard.
- `cp-server` — the HTTP/SSE server that reads and mutates governance state in
  Valkey.
