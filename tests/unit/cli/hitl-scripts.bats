#!/usr/bin/env bats
# bats file_tags=unit,cli
# Unit tests for agents/openclaw/scripts/polis-*.sh
# These run on the host — no containers required.

setup() {
    load "../../lib/test_helper.bash"
    SCRIPTS_DIR="${PROJECT_ROOT}/agents/openclaw/scripts"
}

# ── File existence ────────────────────────────────────────────────────────────

@test "hitl: polis-mcp-call.sh exists and is executable" {
    [[ -x "${SCRIPTS_DIR}/polis-mcp-call.sh" ]]
}

@test "hitl: polis-report-block.sh exists and is executable" {
    [[ -x "${SCRIPTS_DIR}/polis-report-block.sh" ]]
}

@test "hitl: polis-check-status.sh exists and is executable" {
    [[ -x "${SCRIPTS_DIR}/polis-check-status.sh" ]]
}

@test "hitl: polis-list-pending.sh exists and is executable" {
    [[ -x "${SCRIPTS_DIR}/polis-list-pending.sh" ]]
}

@test "hitl: polis-security-status.sh exists and is executable" {
    [[ -x "${SCRIPTS_DIR}/polis-security-status.sh" ]]
}

@test "hitl: polis-security-log.sh exists and is executable" {
    [[ -x "${SCRIPTS_DIR}/polis-security-log.sh" ]]
}

# ── Syntax checks ─────────────────────────────────────────────────────────────

@test "hitl: polis-mcp-call.sh passes bash syntax check" {
    run bash -n "${SCRIPTS_DIR}/polis-mcp-call.sh"
    assert_success
}

@test "hitl: polis-report-block.sh passes bash syntax check" {
    run bash -n "${SCRIPTS_DIR}/polis-report-block.sh"
    assert_success
}

@test "hitl: polis-check-status.sh passes bash syntax check" {
    run bash -n "${SCRIPTS_DIR}/polis-check-status.sh"
    assert_success
}

@test "hitl: polis-list-pending.sh passes bash syntax check" {
    run bash -n "${SCRIPTS_DIR}/polis-list-pending.sh"
    assert_success
}

@test "hitl: polis-security-status.sh passes bash syntax check" {
    run bash -n "${SCRIPTS_DIR}/polis-security-status.sh"
    assert_success
}

@test "hitl: polis-security-log.sh passes bash syntax check" {
    run bash -n "${SCRIPTS_DIR}/polis-security-log.sh"
    assert_success
}

# ── Correctness checks ────────────────────────────────────────────────────────

@test "hitl: polis-mcp-call.sh uses HTTPS by default" {
    run grep 'POLIS_MCP_URL:-https://' "${SCRIPTS_DIR}/polis-mcp-call.sh"
    assert_success
}

@test "hitl: polis-mcp-call.sh targets toolbox:8080/mcp" {
    run grep 'toolbox:8080/mcp' "${SCRIPTS_DIR}/polis-mcp-call.sh"
    assert_success
}

@test "hitl: polis-report-block.sh requires request_id argument" {
    run grep 'request_id' "${SCRIPTS_DIR}/polis-report-block.sh"
    assert_success
}

@test "hitl: polis-check-status.sh requires request_id argument" {
    run grep 'request_id' "${SCRIPTS_DIR}/polis-check-status.sh"
    assert_success
}

@test "hitl: polis-mcp-call.sh outputs JSON error when no args" {
    run bash "${SCRIPTS_DIR}/polis-mcp-call.sh"
    assert_failure
}

@test "hitl: polis-report-block.sh fails without args" {
    run bash "${SCRIPTS_DIR}/polis-report-block.sh"
    assert_failure
}

@test "hitl: polis-check-status.sh fails without args" {
    run bash "${SCRIPTS_DIR}/polis-check-status.sh"
    assert_failure
}

# ── SOUL.md checks ────────────────────────────────────────────────────────────

@test "hitl: SOUL.md exists" {
    [[ -f "${PROJECT_ROOT}/agents/openclaw/config/SOUL.md" ]]
}

@test "hitl: SOUL.md documents shell commands not MCP" {
    run grep 'polis-report-block' "${PROJECT_ROOT}/agents/openclaw/config/SOUL.md"
    assert_success
}

@test "hitl: SOUL.md does not reference mcpServers" {
    run grep -i 'mcpServers\|mcp server\|polis-security.*mcp' \
        "${PROJECT_ROOT}/agents/openclaw/config/SOUL.md"
    assert_failure
}

@test "hitl: openclaw init.sh does not configure mcpServers" {
    run grep 'mcpServers' "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_failure
}
