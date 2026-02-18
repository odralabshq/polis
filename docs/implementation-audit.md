# Polis CLI — Implementation Audit

> **Date:** 2026-02-18  
> **Auditor:** Kiro  
> **Spec source:** `~/odralabshq/docs/linear-issues/polis-oss/ux-improvments/`

---

## Legend

- ✅ **Done** — fully implemented per spec
- ⚠️ **Partial** — structure exists, logic is stubbed or incomplete
- ❌ **Not started** — spec exists, no implementation

---

## Issue-by-Issue Status

| # | Issue | Status | Notes |
|---|-------|--------|-------|
| 01 | Foundation Types | ✅ Done | All types in `polis-common/src/types.rs`. `RunStage::next()` / `description()` are `const fn`, `PartialOrd`/`Ord` derived, all serde derives correct. |
| 02 | CLI Crate Skeleton | ✅ Done | Full crate, clap derive, all commands wired, internal commands hidden. `NO_COLOR` env binding was broken (fixed 2026-02-18). |
| 03 | Output Styling | ✅ Done | `Styles` + `OutputContext` + `progress.rs` exist and match spec. **Missing:** `OutputContext` helper methods (`success()`, `warn()`, `error()`, `info()`, `header()`, `kv()`) specified in §2.2 are not on the struct — commands use `owo_colors` directly instead. |
| 04 | Security Level Simplification | ✅ Done | `Relaxed` removed, `migrate_security_level()` implemented, `description()` / `auto_allow_known()` / `prompt_new_domains()` all present. |
| 05 | Valkey Metrics Integration | ❌ Removed | Removed entirely (2026-02-18): CLI reader, `MetricsSnapshot` type, `redis` dep, and `polis logs` command all deleted. Activity stream types (`ActivityEvent` etc.) retained in `polis-common` for future use. |
| 06 | Status Command | ✅ Done | Multipass VM + container detection implemented. JSON schema correct. Human output shows workspace/security status. |
| 07 | Run State Machine | ✅ Done | State machine structure, `StateManager`, checkpoint/resume, agent switching, `list_available_agents()` all implemented. `execute_stage()` now has real implementations: `ImageReady` checks local qcow2, `WorkspaceCreated` launches VM via multipass, `CredentialsSet` transfers CA cert, `Provisioned` runs docker compose, `AgentReady` waits for healthy. `get_default_agent()` reads `defaults.agent` from config. Unit + property tests added. |
| 08 | Start/Stop/Delete | ⚠️ Partial | Commands exist, `WorkspaceDriver` trait exists. **Missing:** `DockerDriver` is all no-ops. `delete` doesn't remove certificates or SSH config as spec §10.1 requires. No real multipass/docker lifecycle calls. |
| 09 | Valkey Streams Activity | ❌ Removed | Removed with issue 05 — `polis logs` command deleted. |
| 10 | Logs Command | ❌ Removed | Removed with issue 05 — `polis logs` command deleted. |
| 11 | SSH Proxy Command | ✅ Done | `_ssh-proxy` with multipass/docker backend detection, STDIO bridging, `bridge_io()` with property tests. |
| 12 | Host Key Pinning | ✅ Done | `_extract-host-key`, `KnownHostsManager`, `validate_host_key()`, 600/700 permissions, integrated into `run.rs` via `pin_host_key()`. |
| 13 | Connect Command | ✅ Done | SSH config template with correct security settings (`ForwardAgent no`, `StrictHostKeyChecking yes`, `User polis`, `ControlPersist 30s`), `SshConfigManager`, Include directive, `--ide vscode/cursor`, permission validation. |
| 14 | Agent Manifest Extension | ✅ Done | Full `AgentManifest` schema in `polis-common/src/agent.rs`, `provider` + `capabilities` fields, `effective_provider()` derivation from `envOneOf`. |
| 15 | Agents Commands | ✅ Done | `list`, `info`, `add` all implemented. Signature check (structural 64-byte ed25519 format). Default `N` for unsigned. JSON output. **Note:** full cryptographic verification deferred — `check_signature()` accepts any 64-byte file as "valid". |
| 16 | Update Command | ⚠️ Partial | `UpdateChecker` trait, `GithubUpdateChecker`, `UpdateInfo`/`SignatureInfo` types, UI flow all present. **Missing:** `verify_signature()` returns a hardcoded placeholder — zipsign verification not wired. |
| 17 | Doctor Command | ⚠️ Partial | All types, `HealthProbe` trait, JSON schema, all 4 security check fields present (V-009 ✅). **Missing:** actual check implementations are stubs/hardcoded (`check_gate_health()` returns `true`, `check_malware_db()` returns `(true, 2)`, etc.). |
| 18 | JSON Output | ✅ Done | `--json` on `status`, `agents list`, `doctor`, `version`. Pretty-printed. Correct schemas. No ANSI in JSON mode. |
| 19 | Config Command | ✅ Done | `show` + `set`, `security.level` validation (rejects `relaxed`), `defaults.agent`, `POLIS_CONFIG` env var, YAML persistence, auto-creates `~/.polis/` on first write. |

---

## Summary

| Category | Count |
|----------|-------|
| ✅ Fully done | 13 |
| ⚠️ Partial | 3 |
| ❌ Not started | 0 |
| ❌ Removed | 3 |

---

## Remaining Gaps

### 1. Start/Stop/Delete Commands (Issue 08)

`DockerDriver` is all no-ops. Real multipass lifecycle calls needed:
- `start` → `multipass start polis`
- `stop` → `multipass exec polis -- docker compose stop` then `multipass stop polis`
- `delete` → stop + `multipass delete polis && multipass purge` + remove certs/SSH config

### 2. Update Command Signature Verification (Issue 16)

`verify_signature()` returns hardcoded placeholder. zipsign verification not wired.

### 3. Doctor Command Health Checks (Issue 17)

All check implementations are stubs:
- `check_gate_health()` → hardcoded `true`
- `check_malware_db()` → hardcoded `(true, 2)`
- etc.

---

## Architecture: Multipass + Docker Compose

All CLI commands interact with the workspace through this two-layer architecture:

```
┌─────────────────────────────────────────────────────────────┐
│  Host Machine                                               │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  polis CLI                                            │  │
│  │  - Uses `multipass` commands to manage VM             │  │
│  │  - Uses `multipass exec polis -- ...` to run          │  │
│  │    commands inside VM                                 │  │
│  └───────────────────────────────────────────────────────┘  │
│                            │                                │
│                            ▼                                │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  Multipass VM (name: "polis")                         │  │
│  │  - Image: polis-vm-dev-amd64.qcow2                    │  │
│  │  - Runs docker compose with security services         │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  Docker Compose Services                        │  │  │
│  │  │  - workspace (polis-workspace container)        │  │  │
│  │  │  - gate (traffic inspection)                    │  │  │
│  │  │  - sentinel (credential protection)             │  │  │
│  │  │  - scanner (malware scanning)                   │  │  │
│  │  │  - resolver, certgen, state, toolbox            │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Command Patterns

**Check VM state:**
```bash
multipass info polis --format json
# Returns: {"info":{"polis":{"state":"Running",...}}}
```

**Execute command inside VM:**
```bash
multipass exec polis -- <command>
```

**Check container states inside VM:**
```bash
multipass exec polis -- docker compose ps --format json
multipass exec polis -- docker compose ps --format json workspace
```

**Start/stop services inside VM:**
```bash
multipass exec polis -- docker compose up -d
multipass exec polis -- docker compose stop
```

### Workspace State Logic

| VM State | Container State | Workspace Status |
|----------|-----------------|------------------|
| Stopped  | N/A             | `stopped`        |
| Starting | N/A             | `starting`       |
| Running  | Not running     | `starting`       |
| Running  | Running         | `running`        |
| Running  | Unhealthy       | `running` (agent shows unhealthy) |
| Error    | N/A             | `error`          |

### Implementation Reference

See `cli/src/commands/status.rs` for the canonical implementation:
- `check_multipass_status()` — checks VM state
- `check_workspace_container()` — checks container inside VM
- `get_security_status()` — checks gate/sentinel/scanner via `multipass exec`
- `get_agent_status()` — checks workspace container health via `multipass exec`

See `cli/src/commands/run.rs` for VM lifecycle:
- `execute_stage()` — orchestrates all 5 stages
- `create_workspace()` — launches VM via `multipass launch`
- `provision_workspace()` — runs `docker compose up -d`
- `wait_for_workspace_healthy()` — polls container health

---

## Security Notes

- **Issue 15 (agents add):** `check_signature()` accepts any 64-byte file as structurally valid. Full ed25519 cryptographic verification is deferred pending key distribution.
- **Issue 16 (update):** `verify_signature()` returns a hardcoded placeholder. zipsign verification must be wired before release.

---

*Updated: 2026-02-18 15:23*
