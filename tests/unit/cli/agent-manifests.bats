#!/usr/bin/env bats
# bats file_tags=unit,cli,agents
# Unit tests for agent manifest structure (agent.yaml files)

setup() {
    load "../../lib/test_helper.bash"
    load "${TESTS_DIR}/bats/bats-file/load.bash"
    OPENCLAW_DIR="${PROJECT_ROOT}/agents/openclaw"
    TEMPLATE_DIR="${PROJECT_ROOT}/agents/_template"
}

# ── File existence ─────────────────────────────────────────────────────────

@test "agent-manifests: openclaw agent.yaml exists" {
    assert_file_exist "${OPENCLAW_DIR}/agent.yaml"
}

@test "agent-manifests: template agent.yaml exists" {
    assert_file_exist "${TEMPLATE_DIR}/agent.yaml"
}

# ── Required fields ───────────────────────────────────────────────────────

@test "agent-manifests: openclaw has required fields" {
    local manifest="${OPENCLAW_DIR}/agent.yaml"
    for field in apiVersion kind metadata spec; do
        grep -q "$field" "$manifest" || fail "missing field: $field"
    done
}

@test "agent-manifests: openclaw has runtime command" {
    run grep -q 'command:' "${OPENCLAW_DIR}/agent.yaml"
    assert_success
}

@test "agent-manifests: openclaw has health check" {
    run grep -q 'health:' "${OPENCLAW_DIR}/agent.yaml"
    assert_success
}

# ── Install & init scripts ────────────────────────────────────────────────

@test "agent-manifests: openclaw install.sh exists and executable" {
    assert_file_exist "${OPENCLAW_DIR}/install.sh"
    [[ -x "${OPENCLAW_DIR}/install.sh" ]]
}

@test "agent-manifests: openclaw scripts/init.sh exists" {
    assert_file_exist "${OPENCLAW_DIR}/scripts/init.sh"
}

# ── Old files removed ─────────────────────────────────────────────────────

@test "agent-manifests: old agent.conf removed" {
    run test -f "${OPENCLAW_DIR}/agent.conf"
    assert_failure
}

@test "agent-manifests: old compose.override.yaml removed" {
    run test -f "${OPENCLAW_DIR}/compose.override.yaml"
    assert_failure
}

# ── .gitignore ─────────────────────────────────────────────────────────────

@test "agent-manifests: .gitignore includes .generated/" {
    run grep -q 'agents/\*/.generated/' "${PROJECT_ROOT}/.gitignore"
    assert_success
}
