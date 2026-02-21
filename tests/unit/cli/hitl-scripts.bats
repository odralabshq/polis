#!/usr/bin/env bats
# bats file_tags=unit,cli
# Unit tests for agents/openclaw/scripts/polis-*.sh
# These run on the host — no containers required.

setup() {
    load "../../lib/test_helper.bash"
    SCRIPTS_DIR="${PROJECT_ROOT}/agents/openclaw/scripts"
}

# ── File existence ────────────────────────────────────────────────────────────

@test "hitl: polis-toolbox-call.sh exists and is executable" {
    [[ -x "${SCRIPTS_DIR}/polis-toolbox-call.sh" ]]
}

@test "hitl: all wrapper scripts exist and are executable" {
    for script in polis-report-block.sh polis-check-status.sh polis-list-pending.sh \
                  polis-security-status.sh polis-security-log.sh; do
        [[ -x "${SCRIPTS_DIR}/${script}" ]] || {
            echo "Missing or not executable: ${script}"
            return 1
        }
    done
}

# ── Syntax checks ─────────────────────────────────────────────────────────────

@test "hitl: polis-toolbox-call.sh passes bash syntax check" {
    run bash -n "${SCRIPTS_DIR}/polis-toolbox-call.sh"
    assert_success
}

# ── Correctness checks ────────────────────────────────────────────────────────

@test "hitl: polis-toolbox-call.sh uses HTTPS by default" {
    run grep 'POLIS_TOOLBOX_URL:-https://' "${SCRIPTS_DIR}/polis-toolbox-call.sh"
    assert_success
}

@test "hitl: polis-toolbox-call.sh targets toolbox:8080/mcp" {
    run grep 'toolbox:8080/mcp' "${SCRIPTS_DIR}/polis-toolbox-call.sh"
    assert_success
}

@test "hitl: polis-toolbox-call.sh outputs JSON error when no args" {
    run bash "${SCRIPTS_DIR}/polis-toolbox-call.sh"
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
