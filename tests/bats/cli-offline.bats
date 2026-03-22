#!/usr/bin/env bats
# CLI offline tests — no VM required. Help, version, flags, error cases.
# Safe to run anywhere polis binary is installed.

load 'bats-support/load'
load 'bats-assert/load'

POLIS_HOME="${POLIS_HOME:-$HOME/.polis}"
POLIS_BIN="${POLIS_HOME}/bin/polis"

setup() {
    [[ -x "${POLIS_BIN}" ]] || skip "polis binary not found at ${POLIS_BIN}"
}

# =============================================================================
# TOP-LEVEL
# =============================================================================

@test "no args: exits non-zero and shows help" {
    run polis
    assert_failure
    assert_output --partial "Usage"
}

@test "--help: exits 0 and lists all subcommands" {
    run polis --help
    assert_success
    assert_output --partial "start"
    assert_output --partial "stop"
    assert_output --partial "delete"
    assert_output --partial "status"
    assert_output --partial "connect"
    assert_output --partial "doctor"
    assert_output --partial "exec"
    assert_output --partial "update"
    assert_output --partial "agent"
    assert_output --partial "security"
    assert_output --partial "version"
}

@test "version: exits 0 and prints version" {
    run polis version
    assert_success
    assert_output --regexp '[0-9]+\.[0-9]+\.[0-9]+'
}

@test "--json version: outputs valid JSON" {
    run polis --json version
    assert_success
    echo "${output}" | jq -e '.version' >/dev/null
}

# =============================================================================
# SUBCOMMAND --help
# =============================================================================

@test "start --help: shows usage" {
    run polis start --help
    assert_success
    assert_output --partial "start"
}

@test "stop --help: shows usage" {
    run polis stop --help
    assert_success
    assert_output --partial "stop"
}

@test "delete --help: shows --all, --no-backup, --yes" {
    run polis delete --help
    assert_success
    assert_output --partial "--all"
    assert_output --partial "--no-backup"
    assert_output --partial "--yes"
}

@test "connect --help: shows --info" {
    run polis connect --help
    assert_success
    assert_output --partial "--info"
}

@test "update --help: shows --check" {
    run polis update --help
    assert_success
    assert_output --partial "--check"
}

@test "doctor --help: shows --verbose and --fix" {
    run polis doctor --help
    assert_success
    assert_output --partial "--verbose"
    assert_output --partial "--fix"
}

@test "exec --help: shows usage" {
    run polis exec --help
    assert_success
    assert_output --partial "exec"
}

@test "agent --help: shows subcommands" {
    run polis agent --help
    assert_success
    assert_output --partial "list"
    assert_output --partial "install"
    assert_output --partial "remove"
    assert_output --partial "activate"
    assert_output --partial "exec"
}

@test "security --help: shows subcommands" {
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

# =============================================================================
# SAFE COMMANDS (no VM mutation)
# =============================================================================

@test "update --check: exits 0" {
    run polis update --check
    assert_success
}

@test "status: exits 0 even without running VM" {
    run polis status
    # exit 0 regardless — graceful degradation
    assert_success
}

@test "--json status: valid JSON without VM" {
    run polis --json --quiet status
    assert_success
    echo "${output}" | jq . >/dev/null
}

@test "doctor: exits 0 or 1, does not crash" {
    run polis doctor
    [[ "${status}" -le 1 ]]
}

@test "agent list: exits 0 or reports not running" {
    run polis agent list
    # exits 0 if VM running, 1 if not — both are valid offline
    [[ "${status}" -le 1 ]]
}

@test "--json agent list: exits 0 or reports not running" {
    run polis --json --quiet agent list
    # exits 0 with JSON if VM running, 1 if not
    [[ "${status}" -le 1 ]]
}

# =============================================================================
# ERROR CASES — missing required args
# =============================================================================

@test "agent install: fails without --path" {
    run polis agent install
    assert_failure
}

@test "agent install --path /nonexistent: fails" {
    run polis agent install --path /nonexistent/path
    assert_failure
}

@test "agent remove: fails for unknown agent" {
    run polis agent remove __nonexistent_agent__
    assert_failure
}

@test "agent exec: fails without args" {
    run polis agent exec
    assert_failure
}

@test "security rule: fails without pattern" {
    run polis security rule
    assert_failure
}

@test "security rule-remove: fails without pattern" {
    run polis security rule-remove
    assert_failure
}

@test "security level: fails without level arg" {
    run polis security level
    assert_failure
}

@test "security level: rejects invalid level" {
    run polis security level invalid
    assert_failure
}

@test "security bypass-remove: fails without domain" {
    run polis security bypass-remove
    assert_failure
}

@test "security credential-remove: fails without args" {
    run polis security credential-remove
    assert_failure
}

@test "security approve: fails without request_id" {
    run polis security approve
    assert_failure
}

@test "security deny: fails without request_id" {
    run polis security deny
    assert_failure
}
