#!/usr/bin/env bats
# OpenClaw Agent Unit Tests
# Tests for OpenClaw agent plugin structure and configuration

setup() {
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"
}

# =============================================================================
# Install Script Tests (replaces Dockerfile tests)
# =============================================================================

@test "openclaw: install.sh exists" {
    test -f "${PROJECT_ROOT}/agents/openclaw/install.sh"
}

@test "openclaw: install.sh is executable" {
    test -x "${PROJECT_ROOT}/agents/openclaw/install.sh"
}

@test "openclaw: install.sh uses strict mode" {
    run grep -q "set -euo pipefail" "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

@test "openclaw: install.sh installs Node.js 22" {
    run grep -q "node_22.x" "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

@test "openclaw: install.sh installs pnpm via corepack" {
    run grep -q "corepack enable" "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

@test "openclaw: install.sh installs Bun" {
    run grep -q "bun.sh/install" "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

@test "openclaw: install.sh clones OpenClaw from GitHub" {
    run grep -q "github.com/openclaw/openclaw" "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

@test "openclaw: install.sh sets NODE_ENV=production" {
    run grep -q "NODE_ENV=production" "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

@test "openclaw: install.sh creates .openclaw directories" {
    run grep -q "/home/polis/.openclaw" "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

# =============================================================================
# Agent Config Tests
# =============================================================================

@test "openclaw: agent.conf exists" {
    test -f "${PROJECT_ROOT}/agents/openclaw/agent.conf"
}

@test "openclaw: agent.conf has AGENT_NAME" {
    run grep -q '^AGENT_NAME=openclaw' "${PROJECT_ROOT}/agents/openclaw/agent.conf"
    assert_success
}

@test "openclaw: agent.conf has AGENT_SERVICE_NAME" {
    run grep -q '^AGENT_SERVICE_NAME=' "${PROJECT_ROOT}/agents/openclaw/agent.conf"
    assert_success
}

@test "openclaw: agent.conf has AGENT_CONTAINER_PORT" {
    run grep -q '^AGENT_CONTAINER_PORT=18789' "${PROJECT_ROOT}/agents/openclaw/agent.conf"
    assert_success
}

# =============================================================================
# Init Script Tests
# =============================================================================

@test "openclaw: init script exists" {
    test -f "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
}

@test "openclaw: init script generates token with openssl" {
    run grep -q "openssl rand -hex 32" "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script saves token to file" {
    run grep -q "gateway-token.txt" "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script creates openclaw.json config" {
    run grep -q "openclaw.json" "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script sets token file permissions to 600" {
    run grep -q 'chmod 600 "$TOKEN_FILE"' "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script detects ANTHROPIC_API_KEY" {
    run grep -q 'ANTHROPIC_API_KEY' "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script detects OPENAI_API_KEY" {
    run grep -q 'OPENAI_API_KEY' "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script detects OPENROUTER_API_KEY" {
    run grep -q 'OPENROUTER_API_KEY' "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script uses openai/gpt-4o for OpenAI key" {
    run grep -q "openai/gpt-4o" "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script uses anthropic model for Anthropic key" {
    run grep -q "anthropic/claude-sonnet" "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

@test "openclaw: init script disables sandbox mode" {
    run grep -q '"mode": "off"' "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_success
}

# =============================================================================
# Service File Tests
# =============================================================================

@test "openclaw: systemd service file exists" {
    test -f "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
}

@test "openclaw: service file has Unit section" {
    run grep -q '\[Unit\]' "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}

@test "openclaw: service file has Service section" {
    run grep -q '\[Service\]' "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}

@test "openclaw: service file has Install section" {
    run grep -q '\[Install\]' "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}

@test "openclaw: service runs as polis user" {
    run grep -q "User=polis" "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}

@test "openclaw: service runs openclaw-init.sh before start" {
    run grep -q "openclaw-init.sh" "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}

@test "openclaw: service depends on network-online.target" {
    run grep -q "network-online.target" "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}

@test "openclaw: systemd service requires polis-init" {
    run grep -q "Requires=polis-init.service" "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}

@test "openclaw: service restarts on failure" {
    run grep -q "Restart=on-failure" "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}

# =============================================================================
# Environment Example Tests
# =============================================================================

@test "openclaw: env example file exists" {
    test -f "${PROJECT_ROOT}/agents/openclaw/config/env.example"
}

@test "openclaw: env example has ANTHROPIC_API_KEY" {
    run grep -q "ANTHROPIC_API_KEY" "${PROJECT_ROOT}/agents/openclaw/config/env.example"
    assert_success
}

@test "openclaw: env example has OPENAI_API_KEY" {
    run grep -q "OPENAI_API_KEY" "${PROJECT_ROOT}/agents/openclaw/config/env.example"
    assert_success
}

@test "openclaw: env example has OPENROUTER_API_KEY" {
    run grep -q "OPENROUTER_API_KEY" "${PROJECT_ROOT}/agents/openclaw/config/env.example"
    assert_success
}

@test "openclaw: env example has BRAVE_SEARCH_API_KEY" {
    run grep -q "BRAVE_SEARCH_API_KEY" "${PROJECT_ROOT}/agents/openclaw/config/env.example"
    assert_success
}

@test "openclaw: env example documents auto-detection" {
    run grep -q "auto-detect" "${PROJECT_ROOT}/agents/openclaw/config/env.example"
    assert_success
}

@test "openclaw: env example does NOT have OPENCLAW_GATEWAY_TOKEN" {
    run grep "^OPENCLAW_GATEWAY_TOKEN=" "${PROJECT_ROOT}/agents/openclaw/config/env.example"
    assert_failure
}

# =============================================================================
# Compose Override Tests
# =============================================================================

@test "openclaw: compose.override.yaml exists" {
    test -f "${PROJECT_ROOT}/agents/openclaw/compose.override.yaml"
}

@test "openclaw: compose override maps port 18789" {
    run grep -q "18789" "${PROJECT_ROOT}/agents/openclaw/compose.override.yaml"
    assert_success
}

@test "openclaw: compose override uses env_file" {
    run grep -q "env_file" "${PROJECT_ROOT}/agents/openclaw/compose.override.yaml"
    assert_success
}

@test "openclaw: compose override mounts openclaw.service" {
    run grep -q "openclaw.service" "${PROJECT_ROOT}/agents/openclaw/compose.override.yaml"
    assert_success
}

@test "openclaw: docker-compose workspace depends on gateway" {
    run grep -A 5 'workspace:' "${PROJECT_ROOT}/deploy/docker-compose.yml"
    assert_success
    assert_output --partial "gateway"
}
