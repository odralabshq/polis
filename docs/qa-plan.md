# Polis CLI — QA Test Plan

> **Version:** 1.0  
> **Date:** 2026-02-18  
> **Binary under test:** `polis` (Rust CLI, `cli/` crate)  
> **Spec reference:** `docs/linear-issues/polis-oss/ux-improvements/ux-improvements-final.md`  
> **Audience:** QA engineers, CI pipeline

---

## 1. Scope

This plan covers systematic black-box and grey-box testing of the compiled `polis` binary. It does **not** cover unit tests (those live in `cli/tests/`) or infrastructure tests (those live in `tests/`). The goal is to verify every command, flag, exit code, output contract, security property, and failure path of the binary as shipped.

### Out of scope
- Internal container behaviour (covered by `tests/integration/` and `tests/e2e/`)
- Rust unit/property tests (covered by `cargo test`)
- C ICAP module tests (covered by `just test-native`)

---

## 2. Test Environment

| Item | Requirement |
|------|-------------|
| OS | Linux x86_64 (Ubuntu 22.04+) |
| Docker | 24+ with Sysbox runtime |
| Binary | Release build: `cargo build --release -p polis-cli` |
| Binary path | `/usr/local/bin/polis` or `./target/release/polis` |
| Home dir | Isolated per test run (`POLIS_CONFIG` override or temp `$HOME`) |
| Polis stack | Running for integration-level tests; absent for unit-level tests |

### Environment variables used in tests

```bash
NO_COLOR=1          # Disable ANSI codes for output assertion
POLIS_CONFIG=/tmp/polis-test/config.yaml   # Isolate config
HOME=/tmp/polis-test                        # Isolate ~/.polis/
```

---

## 3. Exit Code Contract

All tests must assert the exit code explicitly.

| Exit code | Meaning |
|-----------|---------|
| `0` | Success |
| `1` | Runtime error (command failed, resource unavailable) |
| `2` | Usage error (bad arguments, unknown flag) — clap default |

---

## 4. Test Cases

### 4.1 Global Flags & Help

| ID | Command | Expected stdout | Expected exit |
|----|---------|-----------------|---------------|
| G-01 | `polis --help` | Contains all top-level commands; no "docker", "container", "VM" | 0 |
| G-02 | `polis -h` | Same as G-01 | 0 |
| G-03 | `polis` (no args) | Help text (arg_required_else_help) | 2 |
| G-04 | `polis --unknown-flag` | Error on stderr; usage hint | 2 |
| G-05 | `polis --no-color status` | No ANSI escape codes in output | 0 |
| G-06 | `NO_COLOR=1 polis status` | No ANSI escape codes in output | 0 |
| G-07 | `polis --quiet status` | No informational output; errors only | 0 |
| G-08 | `polis -q status` | Same as G-07 | 0 |
| G-09 | `polis --json status` | Valid JSON on stdout | 0 |
| G-10 | `polis --version` | `polis X.Y.Z` | 0 |
| G-11 | `polis -V` | Same as G-10 | 0 |

**Vocabulary audit (applies to ALL commands):**
```bash
# Must return zero matches for any command output
polis <cmd> 2>&1 | grep -iE '\b(docker|container|vm|multipass|valkey|redis|g3proxy|c-icap|tproxy|icap|coredns)\b'
```

---

### 4.2 `polis version`

| ID | Command | Expected stdout | Expected exit |
|----|---------|-----------------|---------------|
| V-01 | `polis version` | `polis X.Y.Z` (semver) | 0 |
| V-02 | `polis version --json` | `{"version": "X.Y.Z"}` — valid JSON, exactly one field | 0 |
| V-03 | `polis version --json` piped to `jq .version` | Non-empty string | 0 |

---

### 4.3 `polis run`

#### Happy paths

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| R-01 | No state file; one agent installed | `polis run` | Runs all 5 stages; prints agent name + "is ready" | 0 |
| R-02 | No state file | `polis run claude-dev` | Same as R-01 with explicit agent | 0 |
| R-03 | State at `Provisioned`; same agent | `polis run claude-dev` | Prints "Resuming from: Provisioned"; runs only AgentReady | 0 |
| R-04 | State at `WorkspaceCreated`; same agent | `polis run claude-dev` | Resumes from WorkspaceCreated; runs remaining stages | 0 |
| R-05 | State with `claude-dev`; user runs `gpt-dev` | `polis run gpt-dev` | Prompts "Switch to gpt-dev?"; on confirm: stops old, starts new | 0 |
| R-06 | Multiple agents installed; no arg | `polis run` | Interactive agent selection prompt | 0 |
| R-07 | `defaults.agent` set in config | `polis run` | Uses default agent without prompt | 0 |

#### Error paths

| ID | Precondition | Command | Expected stderr | Expected exit |
|----|-------------|---------|-----------------|---------------|
| R-08 | No agents installed | `polis run` | "No agents installed. Run: polis agents add <path>" | 1 |
| R-09 | Agent `foo` not installed | `polis run foo` | "Agent 'foo' not found. Available: ..." | 1 |
| R-10 | State file corrupted (invalid JSON) | `polis run claude-dev` | Warning printed; starts fresh | 0 |
| R-11 | Home dir not writable | `polis run claude-dev` | Error mentioning state file path | 1 |

#### State machine verification

After each stage completes, `~/.polis/state.json` must:
- Exist with mode `0600`
- Contain valid JSON with `stage`, `agent`, `workspace_id`, `started_at`
- Have `stage` equal to the last completed stage name

| ID | Check |
|----|-------|
| R-12 | State file created after `ImageReady` |
| R-13 | State file updated after each subsequent stage |
| R-14 | State file mode is `0600` (not `0644` or `0777`) |
| R-15 | `image_sha256` field present after `ImageReady` |

---

### 4.4 `polis start`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| S-01 | Workspace stopped | `polis start` | "Starting workspace..." → "Workspace started" | 0 |
| S-02 | Workspace already running | `polis start` | "Workspace is already running" | 0 |
| S-03 | No state file | `polis start` | "No workspace found. Run: polis run <agent>" on stderr | 1 |

---

### 4.5 `polis stop`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| ST-01 | Workspace running | `polis stop` | "Stopping workspace..." → "Workspace is not running" → "Your data is preserved." | 0 |
| ST-02 | Workspace already stopped | `polis stop` | "Workspace is not running" | 0 |
| ST-03 | No state file | `polis stop` | Error on stderr | 1 |

---

### 4.6 `polis delete`

| ID | Precondition | Command | Input | Expected behaviour | Expected exit |
|----|-------------|---------|-------|-------------------|---------------|
| D-01 | Workspace exists | `polis delete` | `y` | Removes workspace; clears state; prints "Workspace removed" | 0 |
| D-02 | Workspace exists | `polis delete` | `n` | No action; exits cleanly | 0 |
| D-03 | Workspace exists | `polis delete` | `N` (uppercase) | No action | 0 |
| D-04 | Workspace exists | `polis delete` | `yes` (full word) | No action (only `y` accepted) | 0 |
| D-05 | No workspace | `polis delete` | `y` | Clears state; prints "Workspace removed" | 0 |
| D-06 | Workspace exists | `polis delete --all` | `y` | Removes workspace + cached images; prints "All data removed" | 0 |
| D-07 | Workspace exists | `polis delete --all` | `n` | No action | 0 |
| D-08 | Workspace exists | `polis delete` | EOF (Ctrl-D) | Error: "no input provided" on stderr | 1 |

**State preservation check after `polis delete`:**
- `~/.polis/config.yaml` — must still exist
- `~/.polis/ssh_config` — must still exist
- `~/.polis/state.json` — must be removed

**State preservation check after `polis delete --all`:**
- `~/.polis/config.yaml` — must still exist
- `~/.polis/ssh_config` — must be removed
- `~/.polis/state.json` — must be removed

---

### 4.7 `polis status`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| ST-01 | Stack running | `polis status` | Human-readable output; no internal terms | 0 |
| ST-02 | Stack running | `polis status --json` | Valid JSON matching schema below | 0 |
| ST-03 | Stack down | `polis status` | Graceful degradation; no crash | 0 |
| ST-04 | Stack down | `polis status --json` | Valid JSON with error state | 0 |

**JSON schema validation for `polis status --json`:**
```json
{
  "workspace": { "status": "<string>", "uptime_seconds": <number|null> },
  "agent": { "name": "<string>", "status": "<string>" } | null,
  "security": {
    "traffic_inspection": <bool>,
    "credential_protection": <bool>,
    "malware_scanning": <bool>
  },
  "metrics": {
    "window_start": "<ISO8601>",
    "requests_inspected": <number>,
    "blocked_credentials": <number>,
    "blocked_malware": <number>
  },
  "events": { "count": <number>, "severity": "<string>" }
}
```

Required assertions:
- `workspace.status` ∈ `{running, stopped, starting, stopping, error}`
- No `uptime_seconds` key when workspace is stopped
- No `agent` key when no agent is running
- All metric counters are non-negative integers

---

### 4.8 `polis logs`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| L-01 | Activity stream has events | `polis logs` | Prints last 100 events; newest last | 0 |
| L-02 | Activity stream empty | `polis logs` | "No activity yet" | 0 |
| L-03 | Stream has events | `polis logs --security` | Only blocked/violation events shown | 0 |
| L-04 | Stream has no security events | `polis logs --security` | Empty output or "No activity yet" | 0 |
| L-05 | Stack running | `polis logs --follow` | Streams new events; exits on Ctrl-C | 0 |
| L-06 | Stack down | `polis logs` | Error on stderr; actionable message | 1 |
| L-07 | Stack down | `polis logs --follow` | Error on stderr; actionable message | 1 |

**Output format assertions for each log line:**
- Contains timestamp in `[HH:MM:SS]` format
- Contains destination domain (no internal IPs)
- Blocked events contain "Blocked:" prefix
- No internal terms (Valkey, ICAP, g3proxy, etc.)

---

### 4.9 `polis shell`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| SH-01 | Workspace running | `polis shell` | Spawns interactive shell in workspace | 0 (on exit) |
| SH-02 | Workspace stopped | `polis shell` | Error: workspace not running | 1 |
| SH-03 | No workspace | `polis shell` | Error: no workspace found | 1 |

---

### 4.10 `polis connect`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| C-01 | SSH not configured | `polis connect` | Prompts to add SSH config; on `y`: creates files | 0 |
| C-02 | SSH not configured | `polis connect` (answer `n`) | "Skipped." message | 0 |
| C-03 | SSH configured | `polis connect` | Shows connection options (ssh, code, cursor) | 0 |
| C-04 | SSH configured | `polis connect --ide vscode` | Launches `code --remote ssh-remote+workspace /workspace` | 0 |
| C-05 | SSH configured | `polis connect --ide cursor` | Launches `cursor --remote ssh-remote+workspace /workspace` | 0 |
| C-06 | SSH configured | `polis connect --ide code` | Same as C-04 (alias) | 0 |
| C-07 | Any | `polis connect --ide unknown` | "Unknown IDE: unknown. Supported: vscode, cursor" on stderr | 1 |
| C-08 | SSH configured | `polis connect --ide vscode` (vscode not installed) | Error: "code is not installed or not in PATH" | 1 |

**SSH config file assertions after `polis connect` (answer `y`):**

```bash
# ~/.polis/ssh_config must contain:
grep "ForwardAgent no"           ~/.polis/ssh_config
grep "StrictHostKeyChecking yes" ~/.polis/ssh_config
grep "User polis"                ~/.polis/ssh_config
grep "ControlPersist 30s"        ~/.polis/ssh_config
grep "IdentitiesOnly yes"        ~/.polis/ssh_config

# Must NOT contain:
grep "ForwardAgent yes"          ~/.polis/ssh_config  # must fail
grep "accept-new"                ~/.polis/ssh_config  # must fail
grep "vscode"                    ~/.polis/ssh_config  # must fail

# File permissions:
stat -c %a ~/.polis/ssh_config   # must be 600
stat -c %a ~/.polis/             # must be 700
stat -c %a ~/.polis/sockets/     # must be 700
```

**`~/.ssh/config` assertion:**
```bash
grep "Include ~/.polis/ssh_config" ~/.ssh/config  # must exist exactly once
```

---

### 4.11 `polis agents`

#### `polis agents list`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| A-01 | Agents installed | `polis agents list` | Table with name, provider, version | 0 |
| A-02 | No agents | `polis agents list` | "No agents installed." + hint | 0 |
| A-03 | Agents installed | `polis agents list --json` | JSON array; each entry has `name`, `provider`, `version`, `capabilities` | 0 |
| A-04 | No agents | `polis agents list --json` | `[]` | 0 |

#### `polis agents info <name>`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| A-05 | Agent exists | `polis agents info claude-dev` | Shows name, provider, version, description, capabilities | 0 |
| A-06 | Agent not found | `polis agents info nonexistent` | "Agent 'nonexistent' not found" on stderr | 1 |

#### `polis agents add <path>`

| ID | Precondition | Command | Input | Expected behaviour | Expected exit |
|----|-------------|---------|-------|-------------------|---------------|
| A-07 | Valid signed agent dir | `polis agents add ./my-agent` | — | "Signature valid"; agent installed | 0 |
| A-08 | Valid unsigned agent dir | `polis agents add ./my-agent` | `y` | Warning shown; default prompt is `N`; on `y`: installed | 0 |
| A-09 | Valid unsigned agent dir | `polis agents add ./my-agent` | `n` (default) | Agent NOT installed | 0 |
| A-10 | Invalid signature | `polis agents add ./my-agent` | — | "Signature verification failed" on stderr | 1 |
| A-11 | Missing `agent.yaml` | `polis agents add ./my-agent` | — | "agent.yaml not found" on stderr | 1 |
| A-12 | Invalid `agent.yaml` (missing required fields) | `polis agents add ./my-agent` | — | Validation error on stderr | 1 |
| A-13 | Path does not exist | `polis agents add /nonexistent` | — | Error on stderr | 1 |

**Security assertion for A-08/A-09:**
- Default prompt answer must be `N` (deny), not `Y`
- User must explicitly type `y` to proceed with unsigned agent

---

### 4.12 `polis config`

#### `polis config show`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| CF-01 | Default config | `polis config show` | Shows `security.level: balanced`; no internal terms | 0 |
| CF-02 | Custom config | `polis config show` | Reflects saved values | 0 |
| CF-03 | No config file | `polis config show` | Shows defaults | 0 |

#### `polis config set`

| ID | Command | Expected behaviour | Expected exit |
|----|---------|-------------------|---------------|
| CF-04 | `polis config set security.level strict` | Sets level; persists to config file | 0 |
| CF-05 | `polis config set security.level balanced` | Sets level; persists | 0 |
| CF-06 | `polis config set security.level relaxed` | Error: "relaxed is not a valid security level" on stderr | 1 |
| CF-07 | `polis config set security.level STRICT` | Error or normalised to `strict` (spec-defined) | 1 or 0 |
| CF-08 | `polis config set defaults.agent claude-dev` | Sets default agent | 0 |
| CF-09 | `polis config set unknown.key value` | Error: unknown key on stderr | 1 |
| CF-10 | `polis config set security.level` (missing value) | Usage error on stderr | 2 |

**Security level constraint (V-003):**
- `relaxed` must NEVER be accepted as a valid value
- Only `balanced` and `strict` are valid

---

### 4.13 `polis doctor`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| DR-01 | All healthy | `polis doctor` | "Everything looks good!" | 0 |
| DR-02 | All healthy | `polis doctor --json` | Valid JSON; `status: "healthy"`; `issues: []` | 0 |
| DR-03 | Low disk (<10 GB) | `polis doctor` | Disk issue shown with `✗` | 0 |
| DR-04 | No internet | `polis doctor` | Internet check fails | 0 |
| DR-05 | DNS broken | `polis doctor` | DNS check fails; issue in list | 0 |
| DR-06 | Certs expired | `polis doctor` | Certificate issue shown | 0 |
| DR-07 | Certs expiring in 7 days | `polis doctor` | Warning `⚠` shown; NOT in issues list | 0 |
| DR-08 | Multiple failures | `polis doctor --json` | `issues` array contains all failures | 0 |

**JSON schema for `polis doctor --json`:**
```json
{
  "status": "healthy" | "unhealthy",
  "checks": {
    "workspace": { "ready": bool, "disk_space_gb": number, "disk_space_ok": bool },
    "network": { "internet": bool, "dns": bool },
    "security": {
      "process_isolation": bool,
      "traffic_inspection": bool,
      "malware_db_current": bool,
      "malware_db_age_hours": number,
      "certificates_valid": bool,
      "certificates_expire_days": number
    }
  },
  "issues": ["string", ...]
}
```

**Security check assertions (V-009):**
- `security.process_isolation` must be present
- `security.traffic_inspection` must be present
- `security.malware_db_current` must be present
- `security.certificates_valid` must be present

---

### 4.14 `polis update`

| ID | Precondition | Command | Expected behaviour | Expected exit |
|----|-------------|---------|-------------------|---------------|
| U-01 | Already up to date | `polis update` | "Already up to date" | 0 |
| U-02 | Update available | `polis update` (answer `y`) | Downloads; verifies signature; replaces binary | 0 |
| U-03 | Update available | `polis update` (answer `n`) | No download | 0 |
| U-04 | Update available; bad signature | `polis update` | "Signature verification failed" on stderr; no binary replaced | 1 |
| U-05 | No network | `polis update` | Error: cannot check for updates | 1 |

**Signature verification assertions (V-008):**
- Binary must NEVER be replaced if signature verification fails
- Signature info (signer, key ID, SHA-256) must be shown before prompt

---

### 4.15 Internal Commands

| ID | Command | Expected behaviour | Expected exit |
|----|---------|-------------------|---------------|
| I-01 | `polis _ssh-proxy` | Not shown in `polis --help` | — |
| I-02 | `polis _provision` | Not shown in `polis --help` | — |
| I-03 | `polis _extract-host-key` | Not shown in `polis --help` | — |
| I-04 | `polis _ssh-proxy` (workspace running) | Bridges STDIO to workspace sshd | 0 |
| I-05 | `polis _extract-host-key` (workspace running) | Outputs `workspace ssh-ed25519 AAAA...` | 0 |
| I-06 | `polis _extract-host-key` (workspace stopped) | Error on stderr | 1 |

---

## 5. Security Test Cases

These tests verify the security audit remediations (V-001 through V-011).

| ID | Audit ID | Test | Pass Condition |
|----|----------|------|----------------|
| SEC-01 | V-001 | `grep "ForwardAgent" ~/.polis/ssh_config` | Value is `no` |
| SEC-02 | V-002 | `grep "StrictHostKeyChecking" ~/.polis/ssh_config` | Value is `yes` |
| SEC-03 | V-002 | `grep "accept-new" ~/.polis/ssh_config` | No match |
| SEC-04 | V-003 | `polis config set security.level relaxed` | Exit 1; error message |
| SEC-05 | V-004 | `stat -c %a ~/.polis/ssh_config` | `600` |
| SEC-06 | V-004 | `stat -c %a ~/.polis/` | `700` |
| SEC-07 | V-004 | `stat -c %a ~/.polis/sockets/` | `700` |
| SEC-08 | V-005 | `polis agents add ./unsigned-agent` (default answer) | Agent NOT installed |
| SEC-09 | V-007 | `grep "ControlPersist" ~/.polis/ssh_config` | Value is `30s` |
| SEC-10 | V-008 | Tamper binary archive; run `polis update` | Exit 1; no binary replaced |
| SEC-11 | V-011 | `grep "User" ~/.polis/ssh_config` | Value is `polis` |
| SEC-12 | V-011 | `grep "vscode" ~/.polis/ssh_config` | No match |

### Vocabulary audit (all commands)

```bash
#!/usr/bin/env bash
# Run against every command; all must return exit 1 (no matches)
FORBIDDEN='docker|container|\bvm\b|multipass|valkey|redis|g3proxy|c-icap|tproxy|coredns'
COMMANDS=(
  "polis version"
  "polis status"
  "polis doctor"
  "polis agents list"
  "polis config show"
  "polis logs"
)
for cmd in "${COMMANDS[@]}"; do
  if $cmd 2>&1 | grep -iE "$FORBIDDEN"; then
    echo "FAIL: forbidden term in output of: $cmd"
    exit 1
  fi
done
echo "PASS: no forbidden terms"
```

---

## 6. Output Mode Tests

| ID | Scenario | Assertion |
|----|----------|-----------|
| OUT-01 | TTY output | ANSI codes present in status/doctor output |
| OUT-02 | Piped output (`polis status \| cat`) | No ANSI codes |
| OUT-03 | `--no-color` flag | No ANSI codes |
| OUT-04 | `NO_COLOR=1` env var | No ANSI codes |
| OUT-05 | `--quiet` flag | No informational output; errors still on stderr |
| OUT-06 | `--json` flag | No ANSI codes; valid JSON on stdout |
| OUT-07 | `--json --quiet` | JSON on stdout; no other output |
| OUT-08 | Progress bars | Only shown on TTY; not in piped output |

---

## 7. Edge Cases & Boundary Conditions

| ID | Scenario | Expected behaviour |
|----|----------|--------------------|
| E-01 | `~/.polis/` does not exist, first write operation (e.g., `config set`) | Created automatically with mode 700 |
| E-02 | `~/.polis/state.json` is a directory | Error with actionable message |
| E-03 | `~/.polis/config.yaml` is malformed YAML | Error with actionable message |
| E-04 | `$HOME` not set | Error: "Cannot determine home directory" |
| E-05 | `polis run` with 0-byte agent.yaml | Validation error |
| E-06 | Agent name with path traversal (`../evil`) | Rejected; error |
| E-07 | `polis config set security.level ""` (empty value) | Error |
| E-08 | `polis logs` with very large stream (>10000 entries) | Reads last 100; no OOM |
| E-09 | `polis update` with no write permission to binary path | Error: permission denied |
| E-10 | Concurrent `polis run` invocations | Second invocation detects existing state |
| E-11 | `polis delete` with stdin closed | Exit 1; "no input provided" |
| E-12 | `polis connect --ide vscode` (IDE not in PATH) | Error: "code is not installed or not in PATH" |

---

## 8. Regression Test Matrix

Run after every code change to the `cli/` crate.

| Test group | Command | Minimum assertions |
|------------|---------|-------------------|
| Smoke | `polis --help` | Exit 0; all commands listed |
| Smoke | `polis version` | Exit 0; semver output |
| Smoke | `polis version --json` | Exit 0; valid JSON |
| Smoke | `polis doctor --json` | Exit 0; valid JSON |
| Security | SSH config properties | All SEC-01 through SEC-12 pass |
| Vocabulary | All commands | No forbidden terms |
| Exit codes | All error paths | Non-zero on failure |
| JSON schema | `status --json`, `doctor --json`, `agents list --json`, `version --json` | Schema valid |

---

## 9. CI Integration

```yaml
# Suggested CI step (add to .github/workflows/ci.yml)
- name: QA binary tests
  run: |
    export NO_COLOR=1
    export HOME=$(mktemp -d)
    BINARY=./target/release/polis

    # Smoke
    $BINARY --help
    $BINARY version
    $BINARY version --json | jq .version

    # Vocabulary audit
    for cmd in "version" "doctor --json" "agents list"; do
      if $BINARY $cmd 2>&1 | grep -iE 'docker|container|\bvm\b|valkey|redis'; then
        echo "FAIL: forbidden term in: polis $cmd"; exit 1
      fi
    done

    # Security level
    $BINARY config set security.level relaxed && exit 1 || true

    # JSON schemas
    $BINARY doctor --json | jq '.status, .checks, .issues'
    $BINARY version --json | jq '.version'
```

---

## 10. Known Limitations / TODOs

- `polis status` currently returns stub data (Valkey not connected in unit context) — integration tests required for live metrics
- `polis shell` implementation pending — test cases SH-01 through SH-03 require full stack
- `polis update` signature verification requires a real release asset — mock in unit tests, real in e2e
- `polis _provision` is a hidden internal command — test via `polis run` integration path only

---

*Plan version: 1.0 — update when new commands are added or spec changes*
