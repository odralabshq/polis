# Design: `polis-query.sh` — Fix for Multipass Windows stdout pipe bug

## The problem in plain terms

`polis status` and `polis doctor` are broken on Windows. They always show the
workspace as "starting" and all security features as disabled, even when
everything is healthy.

The root cause: on Windows, `multipass exec polis -- bash -c "cmd1 | cmd2"`
returns empty stdout. Any command that uses a shell pipe (`|`) inside the VM
produces zero bytes back to the caller. Exit code is still 0, so the failure
is completely silent.

The current `workspace_status.rs` runs this command to gather status:

```
bash -c "cat /proc/uptime && echo '---' && docker compose -f /opt/polis/docker-compose.yml ps --format json"
```

`docker compose ps --format json` uses pipes internally. On Windows, the
output never arrives. The CLI sees empty stdout, parses nothing, and reports
everything as down.

---

## Solution comparison

### Option A — In-VM script (`polis-query.sh`)

Deploy a shell script into the VM during provisioning. The CLI calls:

```
multipass exec polis -- /opt/polis/scripts/polis-query.sh status
```

The script runs entirely inside the VM. All pipes stay inside the VM. From
Multipass's perspective on Windows, it's one process writing a single blob
to stdout — no pipes cross the Multipass boundary.

**Pros:**
- Single round trip (one `multipass exec` call)
- Clean separation: the script owns the "what to collect" logic
- Easy to extend — add a new query type without touching Rust
- Testable independently of the CLI (run the script directly in the VM)
- Matches the existing pattern: cloud-init already deploys scripts to `/opt/polis/scripts/`

**Cons:**
- Script must be deployed during provisioning (one extra file in the config bundle)
- Script version must stay in sync with CLI expectations

### Option B — Write to file + transfer

The CLI runs a command that writes output to a temp file inside the VM, then
uses `multipass transfer` to pull the file to the host:

```
multipass exec polis -- bash -c "docker compose ps --format json > /tmp/polis-status.json"
multipass transfer polis:/tmp/polis-status.json /tmp/local-status.json
# read /tmp/local-status.json
```

**Pros:**
- No new file to deploy into the VM
- Completely avoids the stdout channel

**Cons:**
- Two round trips per query (exec + transfer) — slower, especially on Windows
  where each `multipass` invocation has significant overhead
- Requires temp file cleanup logic
- `multipass transfer` has its own quirks on Windows (path separators, permissions)
- The `bash -c "... > /tmp/file"` write step still uses a redirect, which
  may trigger the same underlying pipe issue in some cases
- More moving parts = more failure modes

### Decision: Option A

Option A is better. One round trip, cleaner code, independently testable,
and fits naturally into the existing provisioning model. The only real cost
is keeping the script in sync with the CLI — which is manageable because the
script's output format is a simple JSON contract.

---

## Design

### 1. The script: `scripts/polis-query.sh`

A single script deployed to `/opt/polis/scripts/polis-query.sh` inside the VM.
It accepts a query name as its first argument and writes JSON to stdout.

```bash
#!/bin/bash
# polis-query.sh — CLI query interface for the Polis VM.
# Called by the host CLI via: multipass exec polis -- /opt/polis/scripts/polis-query.sh <query>
# All output is JSON on stdout. Exit code 0 = success, non-zero = error.
set -euo pipefail

COMPOSE_FILE="/opt/polis/docker-compose.yml"
QUERY="${1:-}"

case "${QUERY}" in
  status)
    UPTIME=$(cat /proc/uptime | awk '{print $1}')
    CONTAINERS=$(docker compose -f "${COMPOSE_FILE}" ps --format json 2>/dev/null || echo "[]")
    printf '{"uptime":%s,"containers":%s}\n' "${UPTIME}" "${CONTAINERS}"
    ;;

  health)
    GATE=$(docker compose -f "${COMPOSE_FILE}" ps --format json gate 2>/dev/null || echo "[]")
    printf '{"gate":%s}\n' "${GATE}"
    ;;

  malware-db)
    STAT=$(stat -c '%Y' /var/lib/clamav/daily.cvd 2>/dev/null || echo "0")
    printf '{"daily_cvd_mtime":%s}\n' "${STAT}"
    ;;

  cert-expiry)
    EXPIRY=$(openssl x509 -enddate -noout -in /opt/polis/certs/ca.crt 2>/dev/null \
      | sed 's/notAfter=//' || echo "unknown")
    printf '{"ca_expiry":"%s"}\n' "${EXPIRY}"
    ;;

  *)
    printf '{"error":"unknown query: %s"}\n' "${QUERY}"
    exit 1
    ;;
esac
```

Key design decisions:
- `printf` instead of `echo` — avoids newline/buffering differences across shells
- All pipes stay inside the script — never cross the Multipass boundary
- Each query outputs a single JSON object on one line — easy to parse
- Unknown queries return a JSON error and exit 1 — detectable by the CLI
- `set -euo pipefail` — any internal failure exits non-zero

### 2. Deploying the script

The script is bundled into the config tarball that the CLI transfers to the VM
during `polis start`. This is the same mechanism used for `docker-compose.yml`,
certs, and agent artifacts.

In `workspace_start.rs`, the existing `transfer_config()` function bundles
`/opt/polis/scripts/`. Add `polis-query.sh` to that directory.

After extraction inside the VM, the existing `chmod +x` step covers it:
```bash
find /opt/polis -name '*.sh' -exec chmod +x {} \;
```
(This already exists to fix the Windows tar execute-bit stripping issue.)

### 3. CLI changes

#### `workspace_status.rs`

Replace `GATHER_STATUS_SCRIPT` with a call to the query script:

```rust
// Before (broken on Windows — pipe crosses Multipass boundary):
const GATHER_STATUS_SCRIPT: &str =
    "cat /proc/uptime && echo '---' && docker compose -f {} ps --format json";

// After:
const QUERY_SCRIPT: &str = "/opt/polis/scripts/polis-query.sh";

async fn gather_remote_info(
    mp: &impl ShellExecutor,
) -> (Option<u64>, HashMap<String, ContainerInfo>) {
    let output = mp.exec(&[QUERY_SCRIPT, "status"]).await;
    // ... parse JSON response
}
```

The JSON response from `status` query:
```json
{
  "uptime": 1764.75,
  "containers": [
    {"Service": "workspace", "State": "running", "Health": "healthy"},
    {"Service": "gate",      "State": "running", "Health": ""},
    {"Service": "sentinel",  "State": "running", "Health": ""},
    {"Service": "scanner",   "State": "running", "Health": ""}
  ]
}
```

Parsing in Rust:
```rust
#[derive(serde::Deserialize)]
struct StatusResponse {
    uptime: Option<f64>,
    containers: Vec<ContainerEntry>,
}

#[derive(serde::Deserialize)]
struct ContainerEntry {
    #[serde(rename = "Service")]
    service: String,
    #[serde(rename = "State")]
    state: String,
    #[serde(rename = "Health")]
    health: Option<String>,
}
```

#### `workspace_doctor.rs`

Replace the three affected exec calls:

| Current call | Replacement |
|---|---|
| `docker compose ps --format json gate` | `polis-query.sh health` |
| `docker exec polis-scanner sh -c "stat ... \| sort \| head"` | `polis-query.sh malware-db` |
| `openssl x509 -enddate ...` | `polis-query.sh cert-expiry` |

### 4. Fallback for missing script

The script won't exist on VMs provisioned before this change. The CLI should
detect this gracefully:

```rust
async fn gather_remote_info(mp: &impl ShellExecutor) -> ... {
    let output = mp.exec(&[QUERY_SCRIPT, "status"]).await;

    match output {
        Err(_) => return defaults(),
        Ok(o) if !o.status.success() => {
            // Script missing or failed — could be old VM
            // Log a warning, return empty status
            return defaults();
        }
        Ok(o) => parse_response(&o.stdout),
    }
}
```

---

## Testing

### Unit tests (no VM required)

The `ShellExecutor` trait is already mockable. Add test cases to
`cli/tests/unit/` that cover the new parsing logic:

```rust
// Test: script returns valid JSON → parsed correctly
#[tokio::test]
async fn status_parses_healthy_response() {
    let mock = MockShellExecutor::new()
        .with_exec_response(
            &["/opt/polis/scripts/polis-query.sh", "status"],
            r#"{"uptime":1764.75,"containers":[
                {"Service":"workspace","State":"running","Health":"healthy"},
                {"Service":"gate","State":"running","Health":""},
                {"Service":"sentinel","State":"running","Health":""},
                {"Service":"scanner","State":"running","Health":""}
            ]}"#,
        );

    let result = gather_status(&mock).await;
    assert_eq!(result.workspace.status, WorkspaceState::Running);
    assert!(result.security.traffic_inspection);
    assert!(result.security.credential_protection);
    assert!(result.security.malware_scanning);
}

// Test: script missing (old VM) → graceful degradation
#[tokio::test]
async fn status_degrades_gracefully_when_script_missing() {
    let mock = MockShellExecutor::new()
        .with_exec_failure(&["/opt/polis/scripts/polis-query.sh", "status"]);

    let result = gather_status(&mock).await;
    assert_eq!(result.workspace.status, WorkspaceState::Starting);
    assert!(!result.security.traffic_inspection);
}

// Test: script returns malformed JSON → no panic, returns defaults
#[tokio::test]
async fn status_handles_malformed_json() {
    let mock = MockShellExecutor::new()
        .with_exec_response(
            &["/opt/polis/scripts/polis-query.sh", "status"],
            "not json at all",
        );

    let result = gather_status(&mock).await;
    assert_eq!(result.workspace.status, WorkspaceState::Starting);
}
```

### Script unit tests (inside VM)

Add a BATS test file `tests/unit/polis-query.bats` that runs the script
directly inside a test environment:

```bash
#!/usr/bin/env bats

setup() {
    load '../lib/common'
    SCRIPT="/opt/polis/scripts/polis-query.sh"
}

@test "status query returns valid JSON" {
    run "${SCRIPT}" status
    assert_success
    echo "${output}" | jq . > /dev/null  # valid JSON
}

@test "status query contains uptime field" {
    run "${SCRIPT}" status
    assert_success
    result=$(echo "${output}" | jq '.uptime')
    [ "${result}" != "null" ]
}

@test "status query contains containers array" {
    run "${SCRIPT}" status
    assert_success
    result=$(echo "${output}" | jq '.containers | length')
    [ "${result}" -gt 0 ]
}

@test "unknown query exits non-zero" {
    run "${SCRIPT}" unknown-query
    assert_failure
    echo "${output}" | jq '.error' | grep -q "unknown query"
}

@test "malware-db query returns mtime" {
    run "${SCRIPT}" malware-db
    assert_success
    result=$(echo "${output}" | jq '.daily_cvd_mtime')
    [ "${result}" != "null" ]
}
```

### Manual verification on Windows

After deploying:
```powershell
# Should return JSON, not empty string
multipass exec polis -- /opt/polis/scripts/polis-query.sh status

# polis status should show Running + all security features active
polis status
```

---

## Implementation order

1. Add `scripts/polis-query.sh` to the repo
2. Add it to the config bundle in `workspace_start.rs`
3. Update `workspace_status.rs` to call the script and parse JSON
4. Update `workspace_doctor.rs` to use `health`, `malware-db`, `cert-expiry` queries
5. Add unit tests for the new parsing logic
6. Add BATS tests for the script itself
7. Manual test on Windows: `polis status` shows correct state

---

## Files changed

| File | Change |
|---|---|
| `scripts/polis-query.sh` | New — the query script |
| `cli/src/application/services/workspace_status.rs` | Replace `GATHER_STATUS_SCRIPT` with script call + JSON parsing |
| `cli/src/application/services/workspace_doctor.rs` | Replace 3 piped exec calls with script queries |
| `cli/src/application/services/workspace_start.rs` | Include `polis-query.sh` in config bundle |
| `cli/tests/unit/status_command.rs` | New unit tests for JSON parsing |
| `tests/unit/polis-query.bats` | New BATS tests for the script |
