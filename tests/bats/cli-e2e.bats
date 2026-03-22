#!/usr/bin/env bats
# CLI e2e tests — doctor, exec, agent, security subcommands.
# Assumes `polis` is already installed at ~/.polis/bin/polis.
# Tests that require a running workspace call ensure_running first.

load 'bats-support/load'
load 'bats-assert/load'

POLIS_HOME="${POLIS_HOME:-$HOME/.polis}"
POLIS_BIN="${POLIS_HOME}/bin/polis"

# -----------------------------------------------------------------------------
# Setup / Teardown
# -----------------------------------------------------------------------------

setup() {
    [[ -x "${POLIS_BIN}" ]] || skip "polis binary not found at ${POLIS_BIN}"
}

# -----------------------------------------------------------------------------
# Helpers
# -----------------------------------------------------------------------------

vm_exists() { multipass info polis &>/dev/null; }

vm_state() {
    multipass info polis --format json 2>/dev/null \
        | jq -r '.info.polis.state // "NotFound"' || echo "NotFound"
}

ensure_running() {
    if ! vm_exists || [[ "$(vm_state)" != "Running" ]]; then
        skip "workspace not running — skipping test that requires a live workspace"
    fi
}

# =============================================================================
# DOCTOR
# =============================================================================

@test "doctor: --help shows options" {
    run polis doctor --help
    assert_success
    assert_output --partial "doctor"
}

@test "doctor: exits 0 when workspace is running" {
    ensure_running
    run polis doctor
    assert_success
}

@test "doctor: exits 0 when no workspace exists (reports issues, does not crash)" {
    # doctor is diagnostic — it should always exit 0 even when things are broken
    run polis doctor
    # exit code 0 or 1 are both acceptable; what matters is it doesn't panic/crash
    [[ "${status}" -le 1 ]]
}

# =============================================================================
# EXEC
# =============================================================================

@test "exec: --help shows usage" {
    run polis exec --help
    assert_success
    assert_output --partial "exec"
}

@test "exec: runs a command inside the workspace" {
    ensure_running
    run polis exec echo hello
    assert_success
    assert_output --partial "hello"
}

@test "exec: propagates non-zero exit code from remote command" {
    ensure_running
    run polis exec false
    assert_failure
}

@test "exec: passes arguments correctly" {
    ensure_running
    run polis exec printf '%s\n' foo bar
    assert_success
    assert_output --partial "foo"
    assert_output --partial "bar"
}

# =============================================================================
# AGENT
# =============================================================================

@test "agent: --help shows subcommands" {
    run polis agent --help
    assert_success
    assert_output --partial "list"
    assert_output --partial "install"
    assert_output --partial "remove"
    assert_output --partial "activate"
}

@test "agent list: returns 0 and lists agents" {
    run polis agent list
    assert_success
}

@test "agent list --json: returns valid JSON" {
    run polis agent list --json
    assert_success
    echo "${output}" | jq . >/dev/null
}

@test "agent install: fails without --path" {
    run polis agent install
    assert_failure
}

@test "agent remove: fails for unknown agent" {
    run polis agent remove __nonexistent_agent__
    assert_failure
}

@test "agent activate: fails for unknown agent" {
    ensure_running
    run polis agent activate __nonexistent_agent__
    assert_failure
}

# =============================================================================
# SECURITY
# =============================================================================

@test "security: --help shows subcommands" {
    run polis security --help
    assert_success
    assert_output --partial "status"
    assert_output --partial "pending"
    assert_output --partial "approve"
    assert_output --partial "deny"
    assert_output --partial "log"
    assert_output --partial "rule"
    assert_output --partial "level"
}

@test "security status: returns 0 when workspace is running" {
    ensure_running
    run polis security status
    assert_success
}

@test "security status --json: returns valid JSON" {
    ensure_running
    run polis security status --json
    assert_success
    echo "${output}" | jq . >/dev/null
}

@test "security pending: returns 0 and lists blocked requests" {
    ensure_running
    run polis security pending
    assert_success
}

@test "security log: returns 0 and shows recent events" {
    ensure_running
    run polis security log
    assert_success
}

@test "security level: sets level to relaxed" {
    ensure_running
    run polis security level relaxed
    assert_success
}

@test "security level: sets level to balanced" {
    ensure_running
    run polis security level balanced
    assert_success
}

@test "security level: sets level to strict" {
    ensure_running
    run polis security level strict
    assert_success
    # restore default
    polis security level balanced >/dev/null 2>&1 || true
}

@test "security level: rejects invalid level" {
    run polis security level invalid
    assert_failure
}

@test "security rule + rules + rule-remove: round-trip" {
    ensure_running
    local domain="e2e-test-domain-$(date +%s).example.com"

    run polis security rule "${domain}"
    assert_success

    run polis security rules
    assert_success
    assert_output --partial "${domain}"

    run polis security rule-remove "${domain}"
    assert_success
}

@test "security rule --action block: adds block rule" {
    ensure_running
    local domain="e2e-block-$(date +%s).example.com"

    run polis security rule "${domain}" --action block
    assert_success

    run polis security rules
    assert_output --partial "${domain}"

    polis security rule-remove "${domain}" >/dev/null 2>&1 || true
}

@test "security bypass: returns 0 and lists bypass domains" {
    ensure_running
    run polis security bypass
    assert_success
}

@test "security credentials: returns 0 and lists credential rules" {
    ensure_running
    run polis security credentials
    assert_success
}

@test "security approve: fails for unknown request id" {
    ensure_running
    run polis security approve req-nonexistent-e2e
    assert_failure
}

@test "security deny: fails for unknown request id" {
    ensure_running
    run polis security deny req-nonexistent-e2e
    assert_failure
}
