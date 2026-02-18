# Polis CLI — User Acceptance Test (UAT) Plan

> **Version:** 1.0  
> **Date:** 2026-02-18  
> **Product:** Polis CLI — Secure workspaces for AI coding agents  
> **Audience:** End users, product owner, developer advocates  
> **Spec reference:** `docs/linear-issues/polis-oss/ux-improvements/ux-improvements-final.md`

---

## 1. Purpose

This UAT plan validates that the `polis` CLI meets real user needs. Tests are written from the perspective of a developer who wants to run AI coding agents securely — not from the perspective of the implementation.

**Acceptance criterion:** A user with no knowledge of Docker, Valkey, TPROXY, or ICAP can install Polis, run an agent, connect to their workspace, and understand what is happening — all within 5 minutes.

---

## 2. Tester Profile

| Role | Description |
|------|-------------|
| Primary tester | Developer who uses AI coding assistants (Claude, GPT, etc.) |
| Secondary tester | DevOps engineer who wants to script Polis in CI |
| Security reviewer | Security-conscious user who reads every prompt carefully |

**Prerequisites for testers:**
- Linux or macOS machine with Docker installed
- Familiarity with terminal usage
- No prior knowledge of Polis internals required

---

## 3. Test Environment

| Item | Requirement |
|------|-------------|
| OS | Linux x86_64 (Ubuntu 22.04+) |
| Polis binary | Installed at `/usr/local/bin/polis` |
| Polis stack | Running (`polis run` brings it up) |
| Test agents | At least one agent available (e.g., `claude-dev`) |
| Network | Internet access available |

---

## 4. Acceptance Criteria (Global)

These apply to every scenario below.

| # | Criterion |
|---|-----------|
| AC-1 | No output ever contains the words: "docker", "container", "VM", "Valkey", "Redis", "g3proxy", "c-icap", "TPROXY", "ICAP", "CoreDNS" |
| AC-2 | Every error message tells the user what to do next |
| AC-3 | Every command has a `--help` with at least one example |
| AC-4 | The binary exits with code 0 on success and non-zero on failure |
| AC-5 | Colors are suppressed when output is piped or `NO_COLOR=1` is set |
| AC-6 | `--quiet` suppresses all non-error output |
| AC-7 | `--json` produces machine-readable output with no ANSI codes |

---

## 5. User Scenarios

### Scenario 1: First-time setup — "I just installed Polis"

**Goal:** User can discover what Polis does and how to use it without reading documentation.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 1.1 | Run `polis --help` | Sees a list of commands with short descriptions. No internal jargon. | |
| 1.2 | Run `polis version` | Sees `polis X.Y.Z` | |
| 1.3 | Run `polis doctor` | Sees a health check with clear ✓/✗ indicators. Understands what is wrong if anything fails. | |
| 1.4 | Run `polis doctor --json` | Gets valid JSON suitable for scripting | |
| 1.5 | Run `polis agents list` | Sees available agents or a helpful "no agents" message with next step | |

**Acceptance:** User can answer "what does Polis do and is my system ready?" without reading any docs.

---

### Scenario 2: Running an agent for the first time — "I want to start coding with Claude"

**Goal:** User can go from zero to a running agent workspace in under 5 minutes.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 2.1 | Run `polis run claude-dev` | Sees progress through stages (image, workspace, credentials, provisioning, agent). Each stage is described in plain English. | |
| 2.2 | Observe stage output | No mention of Docker, containers, or internal services | |
| 2.3 | Wait for completion | Sees "claude-dev is ready" | |
| 2.4 | Run `polis status` | Sees workspace: running, agent: claude-dev (healthy), security status, activity metrics | |
| 2.5 | Run `polis status --json` | Gets valid JSON; can pipe to `jq` | |

**Acceptance:** User reaches a running agent without confusion. Time from `polis run` to "is ready" is under 5 minutes on a fast connection.

---

### Scenario 3: Resuming after interruption — "My laptop crashed mid-setup"

**Goal:** User can resume a partially-completed run without starting over.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 3.1 | Simulate interruption at `WorkspaceCreated` stage (kill process) | State file exists at `~/.polis/state.json` | |
| 3.2 | Run `polis run claude-dev` again | Sees "Resuming from: Workspace created" | |
| 3.3 | Observe that already-completed stages are skipped | Only remaining stages run | |
| 3.4 | Completion | "claude-dev is ready" | |

**Acceptance:** User does not have to start from scratch after an interruption.

---

### Scenario 4: Switching agents — "I want to try GPT instead of Claude"

**Goal:** User can switch agents without losing workspace data.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 4.1 | Workspace running with `claude-dev` | `polis status` shows claude-dev healthy | |
| 4.2 | Run `polis run gpt-dev` | Sees "Workspace is running claude-dev." and a prompt: "Switch to gpt-dev? This will restart the agent." | |
| 4.3 | Answer `y` | Sees "Stopping claude-dev..." then "Starting gpt-dev..." then "gpt-dev is ready" | |
| 4.4 | Answer `n` | No change; original agent still running | |
| 4.5 | Run `polis status` after switch | Shows gpt-dev as the active agent | |

**Acceptance:** Agent switching is safe, reversible, and clearly communicated.

---

### Scenario 5: Stopping and restarting — "I'm done for the day"

**Goal:** User can stop and restart the workspace without losing data.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 5.1 | Run `polis stop` | "Stopping workspace..." → "Workspace is not running" → "Your data is preserved. Run: polis start" | |
| 5.2 | Run `polis status` | Shows workspace: stopped | |
| 5.3 | Run `polis start` | "Starting workspace..." → "Workspace started" → "Run: polis status" | |
| 5.4 | Run `polis status` | Shows workspace: running | |
| 5.5 | Verify data preserved | Files created before stop are still present | |

**Acceptance:** Stop/start cycle is safe and data is never lost.

---

### Scenario 6: Deleting a workspace — "I want to start fresh"

**Goal:** User understands exactly what will be deleted before confirming.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 6.1 | Run `polis delete` | Sees clear warning: "This will remove the workspace and all agent data. Configuration and cached images are preserved." | |
| 6.2 | Answer `n` (or press Enter) | No action taken | |
| 6.3 | Run `polis delete` again; answer `y` | Workspace removed; "Run: polis run <agent> to create a new workspace" | |
| 6.4 | Verify config preserved | `~/.polis/config.yaml` still exists | |
| 6.5 | Run `polis delete --all` | Sees warning: "This will remove everything including cached images (~3.5 GB). Only configuration is preserved." | |
| 6.6 | Answer `y` | Everything removed except config | |

**Acceptance:** User is never surprised by what gets deleted. Default answer is always "no".

---

### Scenario 7: Viewing agent activity — "What is my agent doing?"

**Goal:** User can see what network requests the agent is making and whether anything was blocked.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 7.1 | Agent has made some requests | `polis logs` shows recent activity with timestamps and destinations | |
| 7.2 | Observe log format | Each line has `[HH:MM:SS]`, destination, and status. No internal IPs or service names. | |
| 7.3 | Run `polis logs --security` | Shows only blocked/flagged events | |
| 7.4 | No security events | `polis logs --security` shows empty or "No activity yet" | |
| 7.5 | Run `polis logs --follow` | New events appear in real time; Ctrl-C exits cleanly | |
| 7.6 | `polis status` with security events | Shows "⚠ N security events — Run: polis logs --security" | |

**Acceptance:** User can understand agent activity without knowing anything about the underlying security stack.

---

### Scenario 8: Connecting to the workspace — "I want to use VS Code"

**Goal:** User can connect their IDE to the workspace with minimal friction.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 8.1 | First time: run `polis connect` | Prompted: "Add SSH configuration to ~/.ssh/config? [Y/n]" | |
| 8.2 | Answer `y` | "SSH configured" message; shows connection commands | |
| 8.3 | Run `ssh workspace` | Connects to workspace shell | |
| 8.4 | Run `polis connect --ide vscode` | VS Code opens with workspace | |
| 8.5 | Run `polis connect --ide cursor` | Cursor opens with workspace | |
| 8.6 | Run `polis connect --ide unknown` | Clear error: "Unknown IDE: unknown. Supported: vscode, cursor" | |
| 8.7 | Run `polis connect` again (already configured) | Shows connection options directly; no re-prompt | |

**Acceptance:** User can connect their preferred IDE in one command. SSH is set up correctly and securely without user needing to know SSH config syntax.

---

### Scenario 9: Managing agents — "I want to add my own agent"

**Goal:** User can add, inspect, and manage agents safely.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 9.1 | Run `polis agents list` | Shows installed agents with provider and version | |
| 9.2 | Run `polis agents info claude-dev` | Shows name, provider, version, description, capabilities | |
| 9.3 | Add a signed agent: `polis agents add ./my-signed-agent` | "Signature valid (signed by: ...)" → "Agent 'my-signed-agent' added" | |
| 9.4 | Add an unsigned agent: `polis agents add ./my-unsigned-agent` | Warning shown; default prompt is `[y/N]` (deny by default) | |
| 9.5 | Accept unsigned agent (type `y`) | Agent installed with warning acknowledged | |
| 9.6 | Reject unsigned agent (press Enter) | Agent NOT installed | |
| 9.7 | Add agent with missing `agent.yaml` | Clear error message | |

**Acceptance:** Users are protected from accidentally running unsigned agents. The default is always to reject.

---

### Scenario 10: Configuring Polis — "I want stricter security"

**Goal:** User can understand and change security settings without knowing implementation details.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 10.1 | Run `polis config show` | Shows current config in readable format | |
| 10.2 | Run `polis config set security.level strict` | "Security level set to: strict" | |
| 10.3 | Run `polis config show` | Shows `security.level: strict` | |
| 10.4 | Run `polis config set security.level relaxed` | Error: "relaxed is not a valid security level. Use: balanced, strict" | |
| 10.5 | Run `polis config set security.level --help` | Shows documentation for security levels in plain English | |
| 10.6 | Run `polis config set defaults.agent claude-dev` | Sets default agent | |
| 10.7 | Run `polis run` (no args) | Uses `claude-dev` without prompting | |

**Acceptance:** User can configure Polis without knowing what "balanced" or "strict" means at the implementation level. The help text explains it in user terms.

---

### Scenario 11: Updating Polis — "I want the latest version"

**Goal:** User can update Polis safely with clear information about what is changing.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 11.1 | Run `polis update` (up to date) | "Already up to date" | |
| 11.2 | Run `polis update` (update available) | Shows current version, new version, and release notes | |
| 11.3 | Observe signature info | Shows "Signed by: Odra Labs (key: 0x...)" and SHA-256 | |
| 11.4 | Answer `y` | Download progress shown; "Updated to vX.Y.Z" | |
| 11.5 | Answer `n` | No download; exits cleanly | |
| 11.6 | Tampered update (bad signature) | "Signature verification failed" — binary NOT replaced | |

**Acceptance:** User is never asked to install an update without seeing what it contains and who signed it.

---

### Scenario 12: Diagnosing problems — "Something isn't working"

**Goal:** User can self-diagnose issues without contacting support.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 12.1 | Run `polis doctor` (all healthy) | "Everything looks good!" with ✓ for each check | |
| 12.2 | Run `polis doctor` (low disk) | Shows disk issue with ✗ and how much space is available | |
| 12.3 | Run `polis doctor` (no internet) | Shows internet check failed | |
| 12.4 | Run `polis doctor` (cert expiring in 7 days) | Shows ⚠ warning (not ✗ error) | |
| 12.5 | Run `polis doctor --json` | Valid JSON; can be used in scripts | |
| 12.6 | Every failing check | Has an actionable message (not just "failed") | |

**Acceptance:** User can identify and understand any system issue from `polis doctor` output alone.

---

### Scenario 13: Scripting and automation — "I want to use Polis in CI"

**Goal:** DevOps user can integrate Polis into automated pipelines.

**Steps:**

| Step | Action | Expected result | Pass? |
|------|--------|-----------------|-------|
| 13.1 | `polis status --json \| jq .workspace.status` | Returns `"running"` or `"stopped"` | |
| 13.2 | `polis doctor --json \| jq .status` | Returns `"healthy"` or `"unhealthy"` | |
| 13.3 | `polis agents list --json \| jq '.[].name'` | Returns agent names | |
| 13.4 | `polis version --json \| jq .version` | Returns version string | |
| 13.5 | `NO_COLOR=1 polis status` | No ANSI codes in output | |
| 13.6 | `polis status --quiet` | No output on success; only errors | |
| 13.7 | Failed command exit code | `echo $?` returns non-zero | |
| 13.8 | `polis status --json` piped to `jq` | No parse errors | |

**Acceptance:** All `--json` outputs are stable, parseable, and schema-consistent across versions.

---

## 6. Non-Functional Acceptance Criteria

| # | Criterion | Target |
|---|-----------|--------|
| NF-1 | Time from `polis run` to "is ready" | < 5 minutes on 100 Mbps connection |
| NF-2 | `polis status` response time | < 2 seconds |
| NF-3 | `polis doctor` response time | < 5 seconds |
| NF-4 | `polis --help` response time | < 0.5 seconds |
| NF-5 | SSH connection on first try | Works without manual intervention |
| NF-6 | Forbidden terms in any output | 0 occurrences |
| NF-7 | Commands with `--help` examples | 100% |
| NF-8 | Error messages with actionable next step | 100% |

---

## 7. UAT Sign-off Checklist

Before release, all of the following must be confirmed by a non-engineer tester:

- [ ] Scenario 1 complete: first-time setup works without docs
- [ ] Scenario 2 complete: agent running in < 5 minutes
- [ ] Scenario 3 complete: resume after interruption works
- [ ] Scenario 4 complete: agent switching is safe
- [ ] Scenario 5 complete: stop/start preserves data
- [ ] Scenario 6 complete: delete warnings are clear; default is deny
- [ ] Scenario 7 complete: logs are readable without internal knowledge
- [ ] Scenario 8 complete: IDE connection works first try
- [ ] Scenario 9 complete: unsigned agent default is deny
- [ ] Scenario 10 complete: `relaxed` security level is rejected
- [ ] Scenario 11 complete: update shows signature before installing
- [ ] Scenario 12 complete: doctor output is self-explanatory
- [ ] Scenario 13 complete: all `--json` outputs are parseable
- [ ] AC-1 confirmed: no forbidden terms in any output
- [ ] AC-2 confirmed: every error has a next step
- [ ] NF-2 confirmed: `polis status` < 2 seconds

---

## 8. Defect Severity Classification

| Severity | Definition | Example |
|----------|------------|---------|
| Critical | Blocks a core user workflow | `polis run` crashes; workspace never starts |
| High | Core workflow works but with significant friction | Resume doesn't work; user must delete and restart |
| Medium | Non-core workflow broken or confusing | `polis connect --ide cursor` fails |
| Low | Minor UX issue | Typo in help text; inconsistent spacing |
| Info | Observation, no action required | Suggestion for improved wording |

**Release gate:** Zero Critical or High defects. Medium defects require product owner sign-off.

---

## 9. Out of Scope for UAT

- Internal container networking (tested in `tests/integration/`)
- ICAP/DLP scanning accuracy (tested in `tests/e2e/`)
- Valkey data persistence (tested in `tests/e2e/`)
- Performance under load
- Multi-user scenarios

---

*Plan version: 1.0 — update when new commands are added or user workflows change*
