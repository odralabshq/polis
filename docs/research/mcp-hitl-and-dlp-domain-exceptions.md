# Research: MCP HITL Visibility & DLP Domain Exception Security

**Date**: 2026-02-14
**Status**: Investigation complete
**Branch**: `feature/add-value-based-exceptions`

---

## Problem Statement

Two concerns raised:
1. OpenClaw agent cannot see/use the polis-security MCP server for HITL approval workflow
2. Do the DLP domain exceptions (Telegram, Discord, Slack) create a credential exfiltration bypass?

---

## Finding 1: OpenClaw Cannot See the MCP Server

### Evidence

The MCP server (`polis-toolbox`) is running, healthy, and fully functional:

```
$ docker exec polis-toolbox cat /proc/1/cmdline | tr '\0' ' '
/usr/bin/polis-mcp-agent

$ docker logs polis-toolbox 2>&1 | grep "client initialized"
INFO rmcp::handler::server: client initialized   # responds to tool calls correctly
```

The server exposes 5 tools (`report_block`, `check_request_status`, `list_pending_approvals`, `get_security_status`, `get_security_log`) via Streamable HTTP on port 8080.

**However, OpenClaw v2026.2.13 has no MCP client support:**

1. The `openclaw.json` config has no `mcpServers` key (it was removed in a prior fix as "invalid")
2. The ACP server code explicitly ignores MCP servers passed to sessions:
   ```js
   // /app/dist/acp-cli-BBZVGb_E.js line 648
   if (params.mcpServers.length > 0) this.log(`ignoring ${params.mcpServers.length} MCP servers`);
   ```
3. The `mcporter` binary (OpenClaw's MCP bridge tool, referenced in docs CLI) is not installed
4. Only 2 files in the entire `/app/dist/` directory reference "mcp" — both are the ACP CLI that ignores it
5. The SOUL.md tells the agent it has `polis-security` MCP tools, but no actual transport connects them

### Root Cause

OpenClaw's ACP protocol accepts `mcpServers` in session creation but the implementation discards them. The MCP client bridge (`mcporter`) is either:
- A separate package not bundled in the Docker image
- A planned feature not yet shipped in v2026.2.13

### Resolution Options

| Option | Effort | Description |
|--------|--------|-------------|
| A. HTTP skill wrapper | Low | Write an OpenClaw skill that makes direct HTTP calls to `http://toolbox:8080/mcp` using the Streamable HTTP protocol. The skill translates natural-language tool invocations into MCP JSON-RPC calls. |
| B. Install mcporter | Low | If `mcporter` is available as an npm/binary package, install it in the workspace container and configure OpenClaw to use it. |
| C. Sidecar proxy | Medium | Run a lightweight Node.js process in the workspace that acts as an MCP-to-HTTP bridge, exposing tools as simple REST endpoints that OpenClaw can call via `fetch`. |
| D. Native commands | Low | Register the 5 MCP tools as OpenClaw native commands (shell scripts) that use `curl` to call the MCP server directly. The SOUL.md already describes the tool interfaces. |

**Recommended**: Option D (native commands via shell scripts) for immediate unblocking, then Option A (HTTP skill) for a cleaner long-term solution. Option D works because:
- OpenClaw can execute shell commands natively
- The MCP server's Streamable HTTP endpoint accepts standard HTTP POST
- No additional dependencies needed
- The SOUL.md already teaches the agent the tool semantics

### Implementation Sketch (Option D)

Create shell scripts in the workspace that wrap `curl` calls:

```bash
#!/bin/bash
# /usr/local/bin/polis-report-block
# Usage: polis-report-block <request_id> <reason> <destination> [pattern]
curl -s -X POST http://toolbox:8080/mcp \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":1,
       \"params\":{\"name\":\"report_block\",\"arguments\":{
         \"request_id\":\"$1\",\"reason\":\"$2\",\"destination\":\"$3\"}}}"
```

Then update SOUL.md to reference these as shell commands instead of MCP tools.

---

## Finding 2: DLP Domain Exception Security Analysis

### Architecture

The DLP module has two independent security layers:

```
Request arrives
    │
    ▼
┌─────────────────────────────────────────┐
│ Layer 1: check_patterns()               │
│ Scans body for credential patterns      │
│ (sk-ant-*, sk-proj-*, ghp_*, AKIA*, etc)│
│                                         │
│ If credential found:                    │
│   - always_block? → BLOCK              │
│   - allow_domain matches host? → PASS  │
│   - allow_domain doesn't match? → BLOCK│
│   - no allow rule? → BLOCK             │
└────────────────┬────────────────────────┘
                 │ has_credential = data->blocked
                 ▼
┌─────────────────────────────────────────┐
│ Layer 2: apply_security_policy()        │
│                                         │
│ If has_credential (already blocked):    │
│   → return 1 (prompt, already handled)  │
│                                         │
│ If NOT has_credential:                  │
│   is_new_domain(host)?                  │
│     known → ALLOW                       │
│     new + RELAXED → ALLOW               │
│     new + BALANCED → PROMPT (HITL)      │
│     new + STRICT → BLOCK                │
└─────────────────────────────────────────┘
```

### Test Case: Credential to Messaging Domain

Scenario: Agent sends OpenAI API key (`sk-proj-...`) to `api.telegram.org`

1. `check_patterns()` matches `pattern.openai`
2. `allow.openai` = `^api\.openai\.com$` — does NOT match `api.telegram.org`
3. **Request BLOCKED** — host doesn't match allow rule
4. `data->blocked = 1`
5. `apply_security_policy(host, has_credential=1)` returns 1 (prompt)
6. HTTP 403 returned with `X-polis-Block: true`

**Result: Credentials to messaging domains ARE blocked.** The domain exceptions in `is_new_domain()` do NOT create a credential bypass.

### Test Case: Credential to Correct Destination

Scenario: Agent sends OpenAI API key to `api.openai.com`

1. `check_patterns()` matches `pattern.openai`
2. `allow.openai` = `^api\.openai\.com$` — MATCHES `api.openai.com`
3. `check_patterns()` continues to next pattern (this match is allowed)
4. No other patterns match → `data->blocked = 0`
5. `apply_security_policy(host, has_credential=0)` — `api.openai.com` is known → returns 0
6. Request ALLOWED

**Result: Correct. Credentials going to their expected API endpoints are allowed.**

### Test Case: Non-Credential Data to Messaging Domain

Scenario: Agent sends arbitrary text (no credential patterns) to `api.telegram.org`

1. `check_patterns()` — no patterns match → `data->blocked = 0`
2. `apply_security_policy(host, has_credential=0)`
3. `is_new_domain("api.telegram.org")` → returns 0 (known domain, in list)
4. Returns 0 (allow)

**Result: Non-credential data to messaging domains is auto-allowed.** This is a design trade-off:
- Required for HITL: the agent must send messages to Telegram to request approvals
- Risk: agent could exfiltrate non-credential sensitive data (source code, configs, etc.) to messaging platforms

### Security Assessment

| Scenario | Credential Detected? | Domain | Result | Correct? |
|----------|---------------------|--------|--------|----------|
| OpenAI key → api.openai.com | Yes | Allowed dest | ALLOW | ✅ |
| OpenAI key → api.telegram.org | Yes | Wrong dest | BLOCK | ✅ |
| OpenAI key → evil.com | Yes | Unknown | BLOCK | ✅ |
| Private key → anywhere | Yes (always_block) | Any | BLOCK | ✅ |
| Source code → api.telegram.org | No | Known | ALLOW | ⚠️ Trade-off |
| Source code → evil.com | No | Unknown+BALANCED | PROMPT | ✅ |
| Normal chat → api.telegram.org | No | Known | ALLOW | ✅ |

### The Trade-off Row (⚠️)

Non-credential data to messaging domains bypasses the new-domain check. This is inherent to the HITL design — the agent needs unrestricted messaging access to request approvals. Mitigations:

1. **Content-length limiting**: The HITL approval messages are small. Large payloads to messaging APIs could be flagged.
2. **Rate limiting**: Unusual volume of messages could trigger alerts.
3. **Audit logging**: All traffic through the proxy is logged for post-hoc review.
4. **Messaging API constraints**: Telegram/Slack/Discord APIs have message size limits (~4KB for Telegram) that naturally limit bulk exfiltration.

**This is an acceptable risk** given that:
- The primary threat (credential exfiltration) is fully mitigated
- Messaging APIs are rate-limited and size-constrained by the platforms themselves
- The alternative (blocking messaging) would break the HITL approval flow entirely

---

## Recommendations

### Immediate (unblock HITL)
1. Create shell-script wrappers for the 5 MCP tools as native commands
2. Update SOUL.md to reference shell commands instead of MCP tools
3. Test the full approval flow end-to-end

### Short-term
4. Investigate `mcporter` availability for proper MCP client integration
5. Add content-length monitoring for messaging domain traffic (alert on payloads > 10KB)

### Long-term
6. Implement MCP client support in OpenClaw (upstream feature request)
7. Add per-domain request body size limits in the DLP module
8. Consider a dedicated "messaging allowlist" separate from the general known_domains list
