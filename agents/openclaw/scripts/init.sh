#!/bin/bash
# =============================================================================
# OpenClaw Initialization Script
# =============================================================================
# Runs before the gateway starts. On first run, generates a gateway token
# and creates the configuration. Token is stored for easy retrieval.
#
# Auto-detects available API keys and configures the default model:
#   - ANTHROPIC_API_KEY -> anthropic/claude-sonnet-4-20250514
#   - OPENAI_API_KEY    -> openai/gpt-4o
#   - OPENROUTER_API_KEY -> openrouter/anthropic/claude-sonnet-4-20250514
# =============================================================================
set -euo pipefail

CONFIG_DIR="/home/polis/.openclaw"
CONFIG_FILE="${CONFIG_DIR}/openclaw.json"
TOKEN_FILE="${CONFIG_DIR}/gateway-token.txt"
ENV_FILE="${CONFIG_DIR}/.env"
FIRST_RUN_MARKER="${CONFIG_DIR}/.initialized"

echo "[openclaw-init] Starting initialization..."

# =============================================================================
# Inject Polis MITM CA into system trust store
# =============================================================================
# The polis-ca.pem is the MITM CA used by g3proxy for TLS interception.
# The system CA bundle (/etc/ssl/certs/ca-certificates.crt) resets on every
# container restart, so we must re-inject every time.
# This ensures ALL TLS clients (node-fetch, undici/globalThis.fetch, curl, etc.)
# trust the proxy's certificates.
POLIS_CA="/etc/ssl/certs/polis-ca.pem"
SYSTEM_CA_BUNDLE="/etc/ssl/certs/ca-certificates.crt"
if [[ -f "$POLIS_CA" ]]; then
    if ! openssl verify -CAfile "$SYSTEM_CA_BUNDLE" "$POLIS_CA" >/dev/null 2>&1; then
        echo "[openclaw-init] Injecting polis CA into system trust store..."
        cat "$POLIS_CA" >> "$SYSTEM_CA_BUNDLE"
        echo "[openclaw-init] Polis CA added to ${SYSTEM_CA_BUNDLE}"
    else
        echo "[openclaw-init] Polis CA already in system trust store"
    fi
else
    echo "[openclaw-init] WARNING: Polis CA not found at ${POLIS_CA}"
fi

# =============================================================================
# Patch OpenClaw WebSocket auth: grant operator scopes to no-device connections
# =============================================================================
# OpenClaw clears scopes to [] when dangerouslyDisableDeviceAuth is enabled
# (no device identity). This causes "missing scope: operator.write" errors.
# We patch the gateway JS to grant operator.read + operator.write instead.
GATEWAY_JS=$(find /app/dist -name 'gateway-cli-*.js' -exec grep -l 'scopes = \[\];' {} \; 2>/dev/null | head -1 || true)
if [[ -n "$GATEWAY_JS" ]]; then
    sed -i 's/scopes = \[\];/scopes = ["operator.read", "operator.write"];/' "$GATEWAY_JS"
    echo "[openclaw-init] Patched WebSocket auth in ${GATEWAY_JS}: operator scopes granted"
else
    echo "[openclaw-init] WebSocket auth patch already applied or gateway JS not found"
fi

# Ensure directories exist with correct permissions
mkdir -p "${CONFIG_DIR}/workspace" \
         "${CONFIG_DIR}/agents" \
         "${CONFIG_DIR}/sessions"

# Generate token function
generate_token() {
    if command -v openssl &>/dev/null; then
        openssl rand -hex 32
    else
        head -c 32 /dev/urandom | xxd -p | tr -d '\n'
    fi
}

# Helper to get env var from container (systemd doesn't inherit container env)
# In sysbox containers with systemd, /proc/1/environ doesn't have Docker env vars
# So we read from the mounted .env file at /run/openclaw-env
get_container_env_early() {
    local var_name="$1"
    local val="${!var_name:-}"
    
    # First try shell env
    if [[ -n "$val" ]]; then
        echo "$val"
        return
    fi
    
    # Try mounted .env file (docker-compose mounts .env to /run/openclaw-env)
    if [[ -f /run/openclaw-env ]]; then
        val=$(grep "^${var_name}=" /run/openclaw-env 2>/dev/null | cut -d= -f2- | head -1 || echo "")
        if [[ -n "$val" ]]; then
            echo "$val"
            return
        fi
    fi
    
    # Fallback: try /proc/1/environ (works in non-sysbox containers)
    if [[ -f /proc/1/environ ]]; then
        val=$(cat /proc/1/environ 2>/dev/null | tr '\0' '\n' | grep "^${var_name}=" | cut -d= -f2- || echo "")
    fi
    
    echo "$val"
}

# Inject polis security instructions into workspace SOUL.md (idempotent)
# OpenClaw loads SOUL.md from the workspace dir as the agent's system prompt.
# We append our security tool docs so the agent knows how to use them.
inject_polis_soul() {
    local ws_soul="${CONFIG_DIR}/workspace/SOUL.md"
    local marker="## Polis Security Workspace"
    
    mkdir -p "${CONFIG_DIR}/workspace"
    
    # Skip if already injected with current shell-only section
    if [[ -f "$ws_soul" ]] && grep -qF "## Security Tools" "$ws_soul" 2>/dev/null; then
        echo "[openclaw-init] Polis security section already in workspace SOUL.md"
        return 0
    fi
    
    # Remove old section (MCP or shell-only) if present — upgrade path
    if [[ -f "$ws_soul" ]] && grep -qF "$marker" "$ws_soul" 2>/dev/null; then
        sed -i "/^---$/,\${ /^## Polis Security Workspace$/,\$d; /^---$/d }" "$ws_soul"
        echo "[openclaw-init] Removed old security section, will re-inject"
    fi
    
    # Append polis security section (works whether file exists or not)
    cat >> "$ws_soul" << 'POLIS_SOUL'

---

## Polis Security Workspace

You are running inside a Polis secure workspace. Your outbound network traffic is monitored by a DLP (Data Loss Prevention) system that protects against credential exfiltration and unauthorized data transfers.

### How the Security System Works

All your HTTP requests pass through a transparent proxy with DLP inspection. The DLP module may **block** a request for two reasons:

1. **Credential detected** — The request body contains a credential pattern (API key, private key, etc.) heading to an unauthorized destination.
2. **New domain** — The request targets a domain not in the known-good list. Behavior depends on the active security level:
   - `relaxed` — new domains are auto-allowed
   - `balanced` (default) — new domains require human approval
   - `strict` — new domains are blocked outright

When a request is blocked, the proxy returns HTTP 403 with headers:
- `X-polis-Block: true`
- `X-polis-Reason: <reason>`
- `X-polis-Pattern: <pattern_name>`
- `X-polis-Request-Id: <request_id>` (format: `req-` + 8 hex chars)

### Security Tools

When you receive a 403 with `X-polis-Block: true`, use these shell commands:

```
polis-report-block <request_id> <reason> <destination> [pattern]
polis-check-status <request_id>
polis-list-pending
polis-security-status
polis-security-log
```

All commands output JSON. They communicate with the polis-toolbox service over HTTPS.

### Approval Workflow

1. Your request gets blocked (HTTP 403 + X-polis headers)
2. Run `polis-report-block <request_id> <reason> <destination>` to register it
3. **Send the approval command as a message to the user**: Include `/polis-approve <request_id>` in your response
4. The proxy rewrites the request_id into a one-time token (OTT) before it reaches the user. The user sees something like `/polis-approve ott-x7k9m2p4`
5. **Tell the user to wait ~5 seconds** before sending the OTT code back — the system has a short security delay
6. The user types the OTT code back in the chat to approve
7. Run `polis-check-status <request_id>` to confirm approval
8. Retry the original request once approved

**When presenting the approval code, always tell the user:**
- They will see a rewritten code starting with `ott-` — that's normal
- They must copy and send that `ott-` code back in the chat
- They should wait about 5 seconds after seeing the code before sending it back

**Example message:**
> My request to httpbin.org was blocked under request ID `req-abc12345`. To approve it, send `/polis-approve req-abc12345`. You'll see a rewritten code starting with `ott-` — wait about 5 seconds, then send that code back to complete the approval.

**If status is still "pending" after the user sent the OTT:**
1. Do NOT run `polis-report-block` again — that creates a new request ID
2. Ask the user to resend the same `ott-` code (it's valid for 10 minutes)
3. Remind them about the 5-second wait — they likely sent it too quickly
4. Only generate a new code after 2-3 failed retries

### Proactive Monitoring

Periodically run `polis-list-pending` or `polis-security-status` to check for blocked requests you haven't handled yet.

**You cannot approve requests yourself.** Only a human can complete the approval by typing the OTT code back.
POLIS_SOUL
    
    chmod 644 "$ws_soul"
    echo "[openclaw-init] Injected polis security section into workspace SOUL.md"
    
    # Remove BOOTSTRAP.md if present — the agent is pre-configured, skip onboarding
    rm -f "${CONFIG_DIR}/workspace/BOOTSTRAP.md"
    
    # Set identity and user files so the agent knows its role
    cat > "${CONFIG_DIR}/workspace/IDENTITY.md" << 'IDEOF'
# IDENTITY.md

- **Name**: Polis Agent
- **Nature**: AI security agent running inside a Polis secure workspace
- **Vibe**: Direct, technical, action-oriented
- **Emoji**: 🛡️
IDEOF
    chmod 644 "${CONFIG_DIR}/workspace/IDENTITY.md"
    
    cat > "${CONFIG_DIR}/workspace/USER.md" << 'USEREOF'
# USER.md

- **Role**: Workspace operator
- **Environment**: Polis secure workspace (Linux container)
- **Preferences**: Direct answers, run tools without asking permission for read-only operations
USEREOF
    chmod 644 "${CONFIG_DIR}/workspace/USER.md"
}

# Auto-detect available API key and select appropriate model
detect_model() {
    local anthropic_key=$(get_container_env_early "ANTHROPIC_API_KEY")
    local openai_key=$(get_container_env_early "OPENAI_API_KEY")
    local openrouter_key=$(get_container_env_early "OPENROUTER_API_KEY")
    
    if [[ -n "$anthropic_key" ]]; then
        echo "anthropic/claude-sonnet-4-20250514"
        echo "[openclaw-init] Detected ANTHROPIC_API_KEY, using Claude" >&2
    elif [[ -n "$openai_key" ]]; then
        echo "openai/gpt-5.1-codex-mini"
        echo "[openclaw-init] Detected OPENAI_API_KEY, using GPT-5.1-codex-mini" >&2
    elif [[ -n "$openrouter_key" ]]; then
        echo "openrouter/anthropic/claude-sonnet-4-20250514"
        echo "[openclaw-init] Detected OPENROUTER_API_KEY, using OpenRouter" >&2
    else
        # Default fallback - user will need to configure manually
        echo "openai/gpt-4o"
        echo "[openclaw-init] WARNING: No API key detected! Set ANTHROPIC_API_KEY, OPENAI_API_KEY, or OPENROUTER_API_KEY" >&2
    fi
}

# First run: create config with auto-generated token
if [[ ! -f "$FIRST_RUN_MARKER" ]]; then
    echo "[openclaw-init] First run detected, setting up OpenClaw..."
    
    # Generate gateway token
    GATEWAY_TOKEN=$(generate_token)
    echo "[openclaw-init] Generated gateway token"
    
    # Save token to file for easy retrieval
    echo "$GATEWAY_TOKEN" > "$TOKEN_FILE"
    chmod 600 "$TOKEN_FILE"
    echo "[openclaw-init] Token saved to ${TOKEN_FILE}"
    
    # Detect which model to use based on available API keys
    DEFAULT_MODEL=$(detect_model)
    
    # Create OpenClaw configuration with the token
    # Note: allowInsecureAuth enables token-only auth for HTTP access (no device identity)
    # This is required for Docker container access where HTTPS is not available
    # dangerouslyDisableDeviceAuth: allows Control UI access without device pairing
    # gateway.mode=local: skip Tailscale/cloud setup
    # chatCompletions: enables /v1/chat/completions HTTP endpoint for API access
    cat > "$CONFIG_FILE" << CONFIGEOF
{
  "gateway": {
    "bind": "lan",
    "port": 18789,
    "mode": "local",
    "auth": {
      "mode": "token",
      "token": "${GATEWAY_TOKEN}"
    },
    "controlUi": {
      "enabled": true,
      "allowInsecureAuth": true,
      "dangerouslyDisableDeviceAuth": true,
      "dangerouslyAllowHostHeaderOriginFallback": true
    },
    "http": {
      "endpoints": {
        "chatCompletions": {
          "enabled": true
        }
      }
    }
  },
  "agents": {
    "defaults": {
      "model": {
        "primary": "${DEFAULT_MODEL}"
      },
      "sandbox": {
        "mode": "off"
      }
    },
    "list": [
      {
        "id": "default",
        "workspace": "/home/polis/.openclaw/workspace"
      }
    ]
  },
  "session": {
    "dmScope": "per-peer"
  },
  "tools": {
    "web": {
      "search": {}
    },
    "exec": {
      "security": "full"
    }
  }
}
CONFIGEOF
    chmod 600 "$CONFIG_FILE"
    echo "[openclaw-init] Created openclaw.json with model: ${DEFAULT_MODEL}"
    
    # Configure exec approvals: auto-approve all commands.
    # Polis provides its own security boundary (DLP/TPROXY), so the OpenClaw
    # exec approval layer is redundant and would require a paired device that
    # doesn't exist in headless mode.
    EXEC_APPROVALS_FILE="${CONFIG_DIR}/exec-approvals.json"
    cat > "$EXEC_APPROVALS_FILE" << 'EAEOF'
{"version":1,"defaults":{"security":"full"}}
EAEOF
    chmod 600 "$EXEC_APPROVALS_FILE"
    echo "[openclaw-init] Configured exec approvals: security=full (auto-approve)"

    # Mark as initialized
    touch "$FIRST_RUN_MARKER"
    
    # Copy SOUL.md (agent instructions for HITL security workflow)
    SOUL_SRC="/usr/local/share/openclaw/SOUL.md"
    SOUL_DST="${CONFIG_DIR}/agents/default/SOUL.md"
    if [[ -f "$SOUL_SRC" ]]; then
        mkdir -p "${CONFIG_DIR}/agents/default"
        cp "$SOUL_SRC" "$SOUL_DST"
        chmod 644 "$SOUL_DST"
        echo "[openclaw-init] Installed SOUL.md (HITL security instructions)"
    fi

    # Inject polis security section into workspace SOUL.md
    # OpenClaw loads bootstrap files from the workspace dir, not the agent dir.
    # We append our security instructions so the agent knows about its tools.
    inject_polis_soul

    # Install polis security CLI wrappers (bridge to MCP toolbox server)
    POLIS_SCRIPTS_SRC="/usr/local/share/openclaw/scripts"
    POLIS_BIN_DIR="/home/polis/.local/bin"
    mkdir -p "$POLIS_BIN_DIR"
    if [[ -d "$POLIS_SCRIPTS_SRC" ]]; then
        for script in polis-toolbox-call.sh polis-report-block.sh polis-check-status.sh \
                      polis-list-pending.sh polis-security-status.sh polis-security-log.sh; do
            if [[ -f "${POLIS_SCRIPTS_SRC}/${script}" ]]; then
                cp "${POLIS_SCRIPTS_SRC}/${script}" "${POLIS_BIN_DIR}/${script}"
                chmod 755 "${POLIS_BIN_DIR}/${script}"
            fi
        done
        echo "[openclaw-init] Installed polis security CLI wrappers to ${POLIS_BIN_DIR}"
    fi
    
else
    echo "[openclaw-init] Already initialized, checking config..."

    # Always ensure controlUi has dangerouslyAllowHostHeaderOriginFallback (needed for non-loopback bind).
    # The gateway may rewrite config on startup, so unconditionally re-apply.
    if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        jq '.gateway.controlUi.dangerouslyAllowHostHeaderOriginFallback = true' "$CONFIG_FILE" > "${CONFIG_FILE}.tmp" \
            && mv "${CONFIG_FILE}.tmp" "$CONFIG_FILE"
        chmod 600 "$CONFIG_FILE"
        echo "[openclaw-init] Ensured controlUi: dangerouslyAllowHostHeaderOriginFallback=true"
    fi

    # Ensure exec approvals stay at security=full (gateway may regenerate the file)
    EXEC_APPROVALS_FILE="${CONFIG_DIR}/exec-approvals.json"
    if [[ -f "$EXEC_APPROVALS_FILE" ]]; then
        if command -v jq &>/dev/null; then
            CURRENT_SEC=$(jq -r '.defaults.security // empty' "$EXEC_APPROVALS_FILE" 2>/dev/null)
            if [[ "$CURRENT_SEC" != "full" ]]; then
                jq '.defaults.security = "full"' "$EXEC_APPROVALS_FILE" > "${EXEC_APPROVALS_FILE}.tmp" \
                    && mv "${EXEC_APPROVALS_FILE}.tmp" "$EXEC_APPROVALS_FILE"
                chmod 600 "$EXEC_APPROVALS_FILE"
                echo "[openclaw-init] Patched exec approvals: security=full"
            fi
        fi
    else
        cat > "$EXEC_APPROVALS_FILE" << 'EAEOF'
{"version":1,"defaults":{"security":"full"}}
EAEOF
        chmod 600 "$EXEC_APPROVALS_FILE"
        echo "[openclaw-init] Created exec approvals: security=full"
    fi

    # Re-install polis security CLI wrappers (they live in tmpfs, lost on restart)
    POLIS_SCRIPTS_SRC="/usr/local/share/openclaw/scripts"
    POLIS_BIN_DIR="/home/polis/.local/bin"
    mkdir -p "$POLIS_BIN_DIR"
    if [[ -d "$POLIS_SCRIPTS_SRC" ]]; then
        for script in polis-toolbox-call.sh polis-report-block.sh polis-check-status.sh \
                      polis-list-pending.sh polis-security-status.sh polis-security-log.sh; do
            if [[ -f "${POLIS_SCRIPTS_SRC}/${script}" ]]; then
                cp "${POLIS_SCRIPTS_SRC}/${script}" "${POLIS_BIN_DIR}/${script}"
                chmod 755 "${POLIS_BIN_DIR}/${script}"
            fi
        done
        echo "[openclaw-init] Re-installed polis security CLI wrappers"
    fi

    # Re-inject polis security section into workspace SOUL.md (idempotent)
    inject_polis_soul

    # Read existing token from file
    if [[ -f "$TOKEN_FILE" ]]; then
        GATEWAY_TOKEN=$(cat "$TOKEN_FILE")
    else
        # Token file missing but config exists - extract from config
        if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
            GATEWAY_TOKEN=$(jq -r '.gateway.auth.token // empty' "$CONFIG_FILE" 2>/dev/null || echo "")
            if [[ -n "$GATEWAY_TOKEN" ]]; then
                echo "$GATEWAY_TOKEN" > "$TOKEN_FILE"
                chmod 600 "$TOKEN_FILE"
            fi
        fi
    fi
fi

# Create/update environment file for systemd service
# This passes API keys from container env to OpenClaw
# In sysbox containers with systemd, we read from mounted .env file
get_container_env() {
    local var_name="$1"
    local default_val="${2:-}"
    local val="${!var_name:-}"
    
    # First try shell env
    if [[ -n "$val" ]]; then
        echo "$val"
        return
    fi
    
    # Try mounted .env file (docker-compose mounts .env to /run/openclaw-env)
    if [[ -f /run/openclaw-env ]]; then
        val=$(grep "^${var_name}=" /run/openclaw-env 2>/dev/null | cut -d= -f2- | head -1 || echo "")
        if [[ -n "$val" ]]; then
            echo "$val"
            return
        fi
    fi
    
    # Fallback: try /proc/1/environ (works in non-sysbox containers)
    if [[ -f /proc/1/environ ]]; then
        val=$(cat /proc/1/environ 2>/dev/null | tr '\0' '\n' | grep "^${var_name}=" | cut -d= -f2- || echo "")
    fi
    
    echo "${val:-$default_val}"
}

OPENCLAW_PORT=$(get_container_env "OPENCLAW_GATEWAY_PORT" "18789")
ANTHROPIC_KEY=$(get_container_env "ANTHROPIC_API_KEY" "")
OPENAI_KEY=$(get_container_env "OPENAI_API_KEY" "")
OPENROUTER_KEY=$(get_container_env "OPENROUTER_API_KEY" "")
BRAVE_KEY=$(get_container_env "BRAVE_SEARCH_API_KEY" "")

# Debug: show what we found
echo "[openclaw-init] Environment source check:"
if [[ -f /run/openclaw-env ]]; then
    echo "[openclaw-init]   - /run/openclaw-env exists (mounted .env file)"
else
    echo "[openclaw-init]   - /run/openclaw-env NOT found"
fi
if [[ -n "$OPENAI_KEY" ]]; then
    echo "[openclaw-init]   - OPENAI_API_KEY: found (${OPENAI_KEY:0:10}...)"
else
    echo "[openclaw-init]   - OPENAI_API_KEY: NOT found"
fi
if [[ -n "$ANTHROPIC_KEY" ]]; then
    echo "[openclaw-init]   - ANTHROPIC_API_KEY: found (${ANTHROPIC_KEY:0:10}...)"
else
    echo "[openclaw-init]   - ANTHROPIC_API_KEY: NOT found"
fi
if [[ -n "$OPENROUTER_KEY" ]]; then
    echo "[openclaw-init]   - OPENROUTER_API_KEY: found (${OPENROUTER_KEY:0:10}...)"
else
    echo "[openclaw-init]   - OPENROUTER_API_KEY: NOT found"
fi

cat > "$ENV_FILE" << ENVEOF
# OpenClaw Environment Variables (auto-generated by openclaw-init.sh)
# API keys are passed from container environment
OPENCLAW_GATEWAY_PORT=${OPENCLAW_PORT}
ANTHROPIC_API_KEY=${ANTHROPIC_KEY}
OPENAI_API_KEY=${OPENAI_KEY}
OPENROUTER_API_KEY=${OPENROUTER_KEY}
BRAVE_SEARCH_API_KEY=${BRAVE_KEY}
HOME=/home/polis
NODE_ENV=production
ENVEOF
chmod 600 "$ENV_FILE"

# Create auth-profiles.json for OpenClaw agents
# OpenClaw stores API keys separately from environment variables
AUTH_PROFILES_DIR="${CONFIG_DIR}/agents/default/agent"
mkdir -p "$AUTH_PROFILES_DIR"

# Build auth profiles JSON based on available keys
AUTH_JSON="{"
FIRST_KEY=true

if [[ -n "$ANTHROPIC_KEY" ]]; then
    AUTH_JSON="${AUTH_JSON}\"anthropic\":{\"apiKey\":\"${ANTHROPIC_KEY}\"}"
    FIRST_KEY=false
    echo "[openclaw-init] Added Anthropic API key to auth-profiles.json"
fi

if [[ -n "$OPENAI_KEY" ]]; then
    if [[ "$FIRST_KEY" == "false" ]]; then
        AUTH_JSON="${AUTH_JSON},"
    fi
    AUTH_JSON="${AUTH_JSON}\"openai\":{\"apiKey\":\"${OPENAI_KEY}\"}"
    FIRST_KEY=false
    echo "[openclaw-init] Added OpenAI API key to auth-profiles.json"
fi

if [[ -n "$OPENROUTER_KEY" ]]; then
    if [[ "$FIRST_KEY" == "false" ]]; then
        AUTH_JSON="${AUTH_JSON},"
    fi
    AUTH_JSON="${AUTH_JSON}\"openrouter\":{\"apiKey\":\"${OPENROUTER_KEY}\"}"
    echo "[openclaw-init] Added OpenRouter API key to auth-profiles.json"
fi

AUTH_JSON="${AUTH_JSON}}"

echo "[openclaw-init] Writing auth-profiles.json: $AUTH_JSON"
echo "$AUTH_JSON" > "${AUTH_PROFILES_DIR}/auth-profiles.json"
chmod 600 "${AUTH_PROFILES_DIR}/auth-profiles.json"
echo "[openclaw-init] auth-profiles.json written to ${AUTH_PROFILES_DIR}/auth-profiles.json"

# Also create for main agent directory if it exists
MAIN_AUTH_DIR="${CONFIG_DIR}/agents/main/agent"
if [[ -d "${CONFIG_DIR}/agents/main" ]]; then
    mkdir -p "$MAIN_AUTH_DIR"
    echo "$AUTH_JSON" > "${MAIN_AUTH_DIR}/auth-profiles.json"
    chmod 600 "${MAIN_AUTH_DIR}/auth-profiles.json"
fi

# Set ownership (use -R carefully to avoid permission issues)
chown polis:polis "${CONFIG_DIR}"
chown polis:polis "${ENV_FILE}"
chown -R polis:polis "${CONFIG_DIR}/agents" 2>/dev/null || true
chown polis:polis "${CONFIG_DIR}/workspace" 2>/dev/null || true
chown polis:polis "${CONFIG_DIR}/sessions" 2>/dev/null || true
chown polis:polis "${CONFIG_FILE}" 2>/dev/null || true
chown polis:polis "${TOKEN_FILE}" 2>/dev/null || true

echo "[openclaw-init] Initialization complete"
echo "[openclaw-init] Gateway token: ${GATEWAY_TOKEN:0:8}...${GATEWAY_TOKEN: -8}"
echo "[openclaw-init] Full token available at: ${TOKEN_FILE}"
