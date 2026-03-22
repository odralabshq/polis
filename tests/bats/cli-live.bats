#!/usr/bin/env bats
# CLI live tests — require a running VM. Skipped automatically otherwise.

load 'bats-support/load'
load 'bats-assert/load'

POLIS_HOME="${POLIS_HOME:-$HOME/.polis}"
POLIS_BIN="${POLIS_HOME}/bin/polis"
PROJECT_ROOT="$(cd "$(dirname "${BATS_TEST_FILENAME}")/../.." && pwd)"

setup() {
    [[ -x "${POLIS_BIN}" ]] || skip "polis binary not found at ${POLIS_BIN}"
    if ! multipass info polis &>/dev/null 2>&1; then
        skip "no workspace VM found"
    fi
    local state
    state=$(multipass info polis --format json 2>/dev/null \
        | jq -r '.info.polis.state // "Unknown"')
    [[ "${state}" == "Running" ]] || skip "workspace not running (state: ${state})"
}

# =============================================================================
# STATUS
# =============================================================================

@test "status: shows Running" {
    run polis status
    assert_success
    assert_output --partial "Running"
}

@test "--json status: valid JSON with state" {
    run polis --json status
    assert_success
    echo "${output}" | jq -e '.vm.state' >/dev/null
}

# =============================================================================
# CONNECT
# =============================================================================

@test "connect --info: exits 0" {
    run polis connect --info
    assert_success
}

# =============================================================================
# DOCTOR
# =============================================================================

@test "doctor: exits 0 with running VM" {
    run polis doctor
    assert_success
}

@test "doctor --verbose: exits 0" {
    run polis doctor --verbose
    assert_success
}

@test "doctor --fix: exits 0" {
    run polis doctor --fix
    assert_success
}

# =============================================================================
# EXEC
# =============================================================================

@test "exec whoami: returns a username" {
    run polis exec whoami </dev/null
    assert_success
    [[ -n "${output}" ]]
}

@test "exec echo hello: output contains hello" {
    run polis exec echo hello </dev/null
    assert_success
    assert_output --partial "hello"
}

@test "exec -- ls -la /: succeeds with -- separator" {
    run polis exec -- ls -la / </dev/null
    assert_success
}

@test "exec false: propagates non-zero exit code" {
    run polis exec false </dev/null
    assert_failure
}

@test "exec printf: passes multiple arguments" {
    run polis exec printf '%s\n' foo bar </dev/null
    assert_success
    assert_output --partial "foo"
    assert_output --partial "bar"
}

# =============================================================================
# SECURITY — read-only queries
# =============================================================================

@test "security status: exits 0 and contains level" {
    run polis security status
    assert_success
    assert_output --partial -i "level"
}

@test "--json security status: valid JSON" {
    run polis --json security status
    assert_success
    echo "${output}" | jq . >/dev/null
}

@test "security pending: exits 0" {
    run polis security pending
    assert_success
}

@test "security log: exits 0" {
    run polis security log
    assert_success
}

# =============================================================================
# SECURITY — level
# =============================================================================

@test "security level relaxed: exits 0" {
    run polis security level relaxed
    assert_success
}

@test "security level balanced: exits 0" {
    run polis security level balanced
    assert_success
}

@test "security level strict: exits 0" {
    run polis security level strict
    assert_success
    # restore default
    polis security level balanced >/dev/null 2>&1 || true
}

# =============================================================================
# SECURITY — rule lifecycle
# =============================================================================

@test "security rule + rules + rule-remove: round-trip" {
    local domain="live-test-$(date +%s).example.com"

    run polis security rule "${domain}"
    assert_success

    run polis security rules
    assert_success
    assert_output --partial "${domain}"

    run polis security rule-remove "${domain}"
    assert_success
}

@test "security rule --action block: adds block rule" {
    local domain="live-block-$(date +%s).example.com"

    run polis security rule "${domain}" --action block
    assert_success

    run polis security rules
    assert_output --partial "${domain}"

    polis security rule-remove "${domain}" >/dev/null 2>&1 || true
}

@test "security rule --action prompt: adds prompt rule" {
    local domain="live-prompt-$(date +%s).example.com"

    run polis security rule "${domain}" --action prompt
    assert_success

    run polis security rules
    assert_output --partial "${domain}"

    polis security rule-remove "${domain}" >/dev/null 2>&1 || true
}

# =============================================================================
# SECURITY — bypass
# =============================================================================

@test "security bypass: exits 0" {
    run polis security bypass
    assert_success
}

@test "security bypass-remove: fails for unknown domain" {
    run polis security bypass-remove __nonexistent_domain_e2e__
    assert_failure
}

# =============================================================================
# SECURITY — credentials
# =============================================================================

@test "security credentials: exits 0" {
    run polis security credentials
    assert_success
}

@test "security credential-remove: fails for unknown rule" {
    run polis security credential-remove fake_pattern fake_host 0000000000000000
    assert_failure
}

# =============================================================================
# SECURITY — approve / deny
# =============================================================================

@test "security approve: fails for unknown request id" {
    run polis security approve req-nonexist
    assert_failure
}

@test "security deny: fails for unknown request id" {
    run polis security deny req-nonexist
    assert_failure
}

# =============================================================================
# AGENT — error cases (need running VM)
# =============================================================================

@test "agent activate: fails for unknown agent" {
    run polis agent activate __nonexistent_agent__
    assert_failure
}

@test "agent exec: fails for unknown agent" {
    run polis agent exec __nonexistent_agent__ token
    assert_failure
}

# =============================================================================
# AGENT — install / list / remove lifecycle
# =============================================================================

@test "agent install --path: installs template agent" {
    run polis agent install --path "${PROJECT_ROOT}/agents/_template"
    assert_success
}

@test "agent list: shows installed template agent" {
    run polis agent list
    assert_success
    assert_output --partial "_template"
}

@test "agent remove: removes template agent" {
    run polis agent remove _template
    assert_success
}
