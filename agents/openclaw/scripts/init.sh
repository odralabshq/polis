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
      "dangerouslyDisableDeviceAuth": true
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
    }
  }
}
CONFIGEOF
    chmod 600 "$CONFIG_FILE"
    echo "[openclaw-init] Created openclaw.json with model: ${DEFAULT_MODEL}"
    
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

    # Install polis security CLI wrappers (bridge to MCP toolbox server)
    POLIS_SCRIPTS_SRC="/usr/local/share/openclaw/scripts"
    POLIS_BIN_DIR="/home/polis/.local/bin"
    mkdir -p "$POLIS_BIN_DIR"
    if [[ -d "$POLIS_SCRIPTS_SRC" ]]; then
        for script in polis-mcp-call.sh polis-report-block.sh polis-check-status.sh \
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

    # Re-install polis security CLI wrappers (they live in tmpfs, lost on restart)
    POLIS_SCRIPTS_SRC="/usr/local/share/openclaw/scripts"
    POLIS_BIN_DIR="/home/polis/.local/bin"
    mkdir -p "$POLIS_BIN_DIR"
    if [[ -d "$POLIS_SCRIPTS_SRC" ]]; then
        for script in polis-mcp-call.sh polis-report-block.sh polis-check-status.sh \
                      polis-list-pending.sh polis-security-status.sh polis-security-log.sh; do
            if [[ -f "${POLIS_SCRIPTS_SRC}/${script}" ]]; then
                cp "${POLIS_SCRIPTS_SRC}/${script}" "${POLIS_BIN_DIR}/${script}"
                chmod 755 "${POLIS_BIN_DIR}/${script}"
            fi
        done
        echo "[openclaw-init] Re-installed polis security CLI wrappers"
    fi

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
