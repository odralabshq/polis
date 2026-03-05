# OpenClaw Startup Fixes — Clear Change Summary

## Branch and commits

- **New branch:** `tomasz/openclaw-fixes-handoff`
- **Included latest commits from source branch:**
  - `b5f55b9` — OpenClaw startup reliability fixes
  - `0d32d89` — CRLF stripping + docker-compose overlay in installer
  - `bbe13ec` — Valkey network alias TLS fix

---

## What was breaking the platform

1. First OpenClaw startup took much longer than configured health timeouts.
2. OpenClaw gateway config could miss a required origin fallback key on reused volumes.
3. CLI could lose agent state if health wait timed out.
4. Systemd restart limits were tight during slow startup.
5. Windows CRLF in transferred config files could break Linux tooling.
6. Toolbox TLS could fail due to hostname mismatch (`state` vs `valkey`).

---

## Changes made (with code snippets)

### 1) Increased OpenClaw health start period (first install is slow)
**File:** `agents/openclaw/agent.yaml`

```yaml
health:
  command: "curl -sf http://127.0.0.1:18789/health"
  interval: 30s
  timeout: 10s
  retries: 5
  startPeriod: 900s
```

**Why:** first install can take ~10+ minutes, so `120s` was too short and caused false failures.

---

### 2) Made OpenClaw config patch unconditional on restart
**File:** `agents/openclaw/scripts/init.sh`

```bash
if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
    jq '.gateway.controlUi.dangerouslyAllowHostHeaderOriginFallback = true' "$CONFIG_FILE" > "${CONFIG_FILE}.tmp" \
        && mv "${CONFIG_FILE}.tmp" "$CONFIG_FILE"
    chmod 600 "$CONFIG_FILE"
    echo "[openclaw-init] Ensured controlUi: dangerouslyAllowHostHeaderOriginFallback=true"
fi
```

**Why:** this guarantees the required key is present even with stale persisted data.

---

### 3) Increased CLI health timeout default
**File:** `cli/src/application/services/vm/health.rs`

```rust
let timeout_secs: u64 = std::env::var("POLIS_HEALTH_TIMEOUT")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(900);
```

**Why:** default `300s` could timeout before OpenClaw became healthy on first run.

---

### 4) Persisted state before waiting for health
**File:** `cli/src/application/services/workspace_start.rs`

```rust
// Persist state before health wait so the CLI tracks the agent
// even if health polling times out (e.g. first-time install).
let mut state = state_mgr
    .load_async()
    .await?
    .unwrap_or_else(|| WorkspaceState { ... });
state.active_agent = Some(name.to_owned());
state_mgr.save_async(&state).await?;

reporter.step("waiting for workspace to become healthy...");
wait_ready(provisioner, reporter, false).await?;
```

**Why:** this prevents losing `active_agent` when startup is slow.

---

### 5) Relaxed systemd start burst limit for OpenClaw
**File:** `cli/src/domain/agent/artifacts.rs`

```rust
out.push_str("StartLimitIntervalSec=300\n");
out.push_str("StartLimitBurst=5\n");
```

**Why:** allows more retries during slow initialization.

---

### 6) Added Valkey network alias for TLS hostname compatibility
**File:** `docker-compose.yml`

```yaml
networks:
  gateway-bridge:
    aliases:
      - valkey
```

**Why:** fixes TLS/certificate hostname mismatch for services connecting using `valkey`.

---

### 7) Hardened CRLF cleanup in VM provisioning
**File:** `cli/src/application/services/vm/provision.rs`

```rust
mp.exec(&[
    "find", "/opt/polis", "-type", "f",
    "(",
    "-name", "*.sh", "-o",
    "-name", "*.yaml", "-o",
    "-name", "*.yml", "-o",
    "-name", "*.env", "-o",
    "-name", "*.service", "-o",
    "-name", "*.toml", "-o",
    "-name", "*.conf",
    ")",
    "-exec", "sed", "-i", "s/\\r$//", "{}", "+",
])
```

**Why:** ensures Linux-side files are LF-only and avoids script/systemd/docker parsing issues.

---

### 8) Installer now overlays latest compose and strips CRLF broadly
**File:** `scripts/install-dev.ps1`

```powershell
# Overlay repo's docker-compose.yml (may be newer than tarball)
$composeFile = Join-Path $RepoDir "docker-compose.yml"
& multipass transfer $composeFile polis:/opt/polis/docker-compose.yml

# Fix script permissions and strip Windows CRLF line endings from all text config files
& multipass exec polis -- bash -c "find /opt/polis -type f -name '*.sh' -exec chmod +x '{}' +"
& multipass exec polis -- bash -c "find /opt/polis -type f \( -name '*.sh' -o -name '*.yaml' -o -name '*.yml' -o -name '*.env' -o -name '*.service' -o -name '*.toml' -o -name '*.conf' \) -exec sed -i 's/\r$//' '{}' +"
```

**Why:** deploys freshest compose file and avoids hidden CRLF failures in Linux VM.

---

## Outcome

- OpenClaw startup is now reliable on slow first install paths.
- CLI correctly remembers active agent state.
- Config and service startup are more resilient.
- Windows-to-Linux line-ending issues are handled more safely.
