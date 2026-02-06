#!/usr/bin/env bats
# OpenClaw Integration Tests
# Tests for OpenClaw running in the workspace container
# Requires: ./tools/polis.sh init --profile=openclaw --local

setup() {
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"
    WORKSPACE_CONTAINER="polis-workspace"
    OPENCLAW_PORT="18789"
}

# Helper to skip if openclaw profile not running
skip_if_not_openclaw() {
    if ! docker ps --format '{{.Names}}' | grep -q "${WORKSPACE_CONTAINER}"; then
        skip "Workspace container not running"
    fi
    # Check if this is the openclaw variant
    if ! docker exec "${WORKSPACE_CONTAINER}" test -f /etc/systemd/system/openclaw.service 2>/dev/null; then
        skip "OpenClaw profile not running (use --profile=openclaw)"
    fi
}

# =============================================================================
# Container State Tests
# =============================================================================

@test "openclaw-int: workspace container is running" {
    skip_if_not_openclaw
    run docker ps --filter "name=${WORKSPACE_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

@test "openclaw-int: workspace uses sysbox runtime" {
    skip_if_not_openclaw
    run docker inspect --format '{{.HostConfig.Runtime}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "sysbox-runc"
}

# =============================================================================
# OpenClaw Service Tests
# =============================================================================

@test "openclaw-int: openclaw.service file exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /etc/systemd/system/openclaw.service
    assert_success
}

@test "openclaw-int: openclaw.service is enabled" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" systemctl is-enabled openclaw.service
    assert_output --regexp "^(enabled|enabled-runtime)$"
}

@test "openclaw-int: openclaw.service is active" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" systemctl is-active openclaw.service
    assert_success
    assert_output "active"
}

@test "openclaw-int: openclaw.service is not failed" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" systemctl is-failed openclaw.service
    assert_failure  # is-failed returns 1 if NOT failed
}

# =============================================================================
# OpenClaw Installation Tests
# =============================================================================

@test "openclaw-int: Node.js is installed" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" node --version
    assert_success
    assert_output --partial "v22"
}

@test "openclaw-int: pnpm is installed" {
    skip_if_not_openclaw
    # Only check binary exists - avoid pnpm --version which triggers corepack network fetch
    run docker exec "${WORKSPACE_CONTAINER}" which pnpm
    assert_success
}

@test "openclaw-int: OpenClaw app directory exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -d /app
    assert_success
}

@test "openclaw-int: OpenClaw package.json exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /app/package.json
    assert_success
}

# =============================================================================
# OpenClaw Configuration Tests
# =============================================================================

@test "openclaw-int: .openclaw directory exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -d /home/polis/.openclaw
    assert_success
}

@test "openclaw-int: openclaw.json config exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /home/polis/.openclaw/openclaw.json
    assert_success
}

@test "openclaw-int: gateway-token.txt exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /home/polis/.openclaw/gateway-token.txt
    assert_success
}

@test "openclaw-int: token file has correct permissions (600)" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" stat -c '%a' /home/polis/.openclaw/gateway-token.txt
    assert_success
    assert_output "600"
}

@test "openclaw-int: token is 64 hex characters" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" cat /home/polis/.openclaw/gateway-token.txt
    assert_success
    # Token should be 64 hex chars (32 bytes)
    assert_output --regexp "^[a-f0-9]{64}$"
}

@test "openclaw-int: config has gateway.auth.token set" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" cat /home/polis/.openclaw/openclaw.json
    assert_success
    assert_output --partial '"token":'
}

@test "openclaw-int: config has sandbox mode off" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" cat /home/polis/.openclaw/openclaw.json
    assert_success
    assert_output --partial '"mode": "off"'
}

@test "openclaw-int: config has controlUi enabled" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" cat /home/polis/.openclaw/openclaw.json
    assert_success
    assert_output --partial '"controlUi"'
    assert_output --partial '"enabled": true'
}

@test "openclaw-int: .openclaw owned by polis user" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" stat -c '%U' /home/polis/.openclaw
    assert_success
    assert_output "polis"
}

# =============================================================================
# OpenClaw Gateway Tests
# =============================================================================

@test "openclaw-int: gateway port 18789 is listening" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" ss -tlnp
    assert_success
    assert_output --partial ":${OPENCLAW_PORT}"
}

@test "openclaw-int: gateway responds to health check" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" curl -sf http://127.0.0.1:${OPENCLAW_PORT}/health
    # May return success or specific health response
    assert_success
}

@test "openclaw-int: gateway port exposed to host" {
    skip_if_not_openclaw
    run docker port "${WORKSPACE_CONTAINER}" "${OPENCLAW_PORT}"
    assert_success
    assert_output --partial "${OPENCLAW_PORT}"
}

# =============================================================================
# Init Script Tests
# =============================================================================

@test "openclaw-int: openclaw-init.sh exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /usr/local/bin/openclaw-init.sh
    assert_success
}

@test "openclaw-int: openclaw-init.sh is executable" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -x /usr/local/bin/openclaw-init.sh
    assert_success
}

@test "openclaw-int: openclaw-health.sh exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /usr/local/bin/openclaw-health.sh
    assert_success
}

@test "openclaw-int: .initialized marker exists" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /home/polis/.openclaw/.initialized
    assert_success
}

# =============================================================================
# Network Tests
# =============================================================================

@test "openclaw-int: can reach gateway container" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts gateway
    assert_success
}

@test "openclaw-int: has default route via gateway" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" ip route show default
    assert_success
    assert_output --partial "via"
}

@test "openclaw-int: CA certificate is trusted" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /usr/local/share/ca-certificates/polis-ca.crt
    assert_success
}

# =============================================================================
# Process Tests
# =============================================================================

@test "openclaw-int: openclaw-gateway process is running" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" pgrep -f "openclaw-gateway"
    assert_success
}

# =============================================================================
# Environment Tests
# =============================================================================

@test "openclaw-int: .env file created by init" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /home/polis/.openclaw/.env
    assert_success
}

@test "openclaw-int: .env has OPENCLAW_GATEWAY_PORT" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" grep "OPENCLAW_GATEWAY_PORT" /home/polis/.openclaw/.env
    assert_success
}

# =============================================================================
# API Key Propagation Tests (verifies /proc/1/environ fix)
# =============================================================================

@test "openclaw-int: API keys in container env are readable from /proc/1/environ" {
    skip_if_not_openclaw
    # Check that at least one API key is set in container environment
    run docker exec "${WORKSPACE_CONTAINER}" bash -c 'cat /proc/1/environ | tr "\0" "\n" | grep -E "^(ANTHROPIC|OPENAI|OPENROUTER)_API_KEY="'
    # This may fail if no API key is configured - that's expected in some test environments
    if [[ "$status" -ne 0 ]]; then
        skip "No API keys configured in container environment"
    fi
    assert_success
}

@test "openclaw-int: .env file contains API key from container env" {
    skip_if_not_openclaw
    # Get API key from container's PID 1 environment
    local container_key
    container_key=$(docker exec "${WORKSPACE_CONTAINER}" bash -c 'cat /proc/1/environ 2>/dev/null | tr "\0" "\n" | grep -E "^(ANTHROPIC|OPENAI|OPENROUTER)_API_KEY=" | head -1 | cut -d= -f2-')
    
    if [[ -z "$container_key" ]]; then
        skip "No API keys configured in container environment"
    fi
    
    # Verify the key was propagated to .env file
    run docker exec "${WORKSPACE_CONTAINER}" grep -F "$container_key" /home/polis/.openclaw/.env
    assert_success
}

@test "openclaw-int: auth-profiles.json exists for default agent" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" test -f /home/polis/.openclaw/agents/default/agent/auth-profiles.json
    assert_success
}

@test "openclaw-int: auth-profiles.json contains API key" {
    skip_if_not_openclaw
    # Check if any API key is configured
    local has_key
    has_key=$(docker exec "${WORKSPACE_CONTAINER}" bash -c 'cat /proc/1/environ 2>/dev/null | tr "\0" "\n" | grep -E "^(ANTHROPIC|OPENAI|OPENROUTER)_API_KEY=.+"' || echo "")
    
    if [[ -z "$has_key" ]]; then
        skip "No API keys configured in container environment"
    fi
    
    # Verify auth-profiles.json has apiKey field
    run docker exec "${WORKSPACE_CONTAINER}" grep -q "apiKey" /home/polis/.openclaw/agents/default/agent/auth-profiles.json
    assert_success
}

@test "openclaw-int: auth-profiles.json has correct permissions (600)" {
    skip_if_not_openclaw
    run docker exec "${WORKSPACE_CONTAINER}" stat -c '%a' /home/polis/.openclaw/agents/default/agent/auth-profiles.json
    assert_success
    assert_output "600"
}
