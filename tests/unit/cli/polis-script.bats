#!/usr/bin/env bats
# bats file_tags=unit,cli
# Unit tests for cli/polis.sh — script structure, commands, functions, security validation

setup() {
    load "../../lib/test_helper.bash"
    POLIS_SCRIPT="${PROJECT_ROOT}/cli/polis.sh"
    WORKSPACE_INIT="${PROJECT_ROOT}/services/workspace/scripts/init.sh"
}

# ── Script basics ──────────────────────────────────────────────────────────

@test "polis.sh: exists and is executable" {
    [[ -x "$POLIS_SCRIPT" ]]
}

@test "polis.sh: passes bash syntax check" {
    run bash -n "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: --help prints usage" {
    run bash "$POLIS_SCRIPT" --help
    assert_success
    assert_output --partial "Usage:"
}

@test "polis.sh: unknown command exits non-zero" {
    run bash "$POLIS_SCRIPT" nonexistent-command
    assert_failure
}

# ── Required commands ──────────────────────────────────────────────────────

@test "polis.sh: has all required commands" {
    local cmds=(init up down start stop status logs build shell test)
    for cmd in "${cmds[@]}"; do
        grep -q "${cmd})" "$POLIS_SCRIPT" || fail "missing command: $cmd"
    done
}

# ── Flag parsing ───────────────────────────────────────────────────────────

@test "polis.sh: has --agent flag parsing" {
    run grep -q '\-\-agent=' "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: has --local flag" {
    run grep -q 'LOCAL_BUILD=' "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: has --no-cache flag" {
    run grep -q 'NO_CACHE=' "$POLIS_SCRIPT"
    assert_success
}

# ── Manifest-driven functions ──────────────────────────────────────────────

@test "polis.sh: has load_agent_yaml function" {
    run grep -q 'load_agent_yaml()' "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: has validate_manifest_security function" {
    run grep -q 'validate_manifest_security()' "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: has generate_compose_override function" {
    run grep -q 'generate_compose_override()' "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: has discover_agents function" {
    run grep -q 'discover_agents()' "$POLIS_SCRIPT"
    assert_success
}

# ── Old references removed ─────────────────────────────────────────────────

@test "polis.sh: old agent.conf references removed" {
    run grep -q 'agent\.conf' "$POLIS_SCRIPT"
    assert_failure
}

# ── Security validation ───────────────────────────────────────────────────

@test "polis.sh: validates metadata.name regex" {
    run grep 'a-z0-9.*a-z0-9' "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: rejects root user" {
    run grep -q '"root"' "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: checks path traversal" {
    run grep -q 'path traversal' "$POLIS_SCRIPT"
    assert_success
}

# ── Generation outputs ─────────────────────────────────────────────────────

@test "polis.sh: generates into .generated/ directory" {
    run grep -q '\.generated/' "$POLIS_SCRIPT"
    assert_success
}

@test "polis.sh: defines reserved platform ports" {
    run grep -q 'RESERVED_PORTS' "$POLIS_SCRIPT"
    assert_success
}

# ── Workspace init.sh integrity ────────────────────────────────────────────

@test "polis.sh: init.sh has SHA-256 integrity check" {
    run grep -q 'sha256sum' "$WORKSPACE_INIT"
    assert_success
}

@test "polis.sh: init.sh has batched daemon-reload" {
    local count
    count=$(grep -c 'systemctl daemon-reload' "$WORKSPACE_INIT")
    [[ "$count" -eq 1 ]]
}
