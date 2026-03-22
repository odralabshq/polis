#!/usr/bin/env bats
# CLI lifecycle tests — DESTRUCTIVE. Stops, deletes, and recreates the VM.
# Run these LAST, in a dedicated QA environment.
# Tests are ordered and each depends on the state left by the previous one.

load 'bats-support/load'
load 'bats-assert/load'

POLIS_HOME="${POLIS_HOME:-$HOME/.polis}"
POLIS_BIN="${POLIS_HOME}/bin/polis"

# Longer timeouts for start/delete operations
export POLIS_VM_START_TIMEOUT="${POLIS_VM_START_TIMEOUT:-600}"

setup() {
    [[ -x "${POLIS_BIN}" ]] || skip "polis binary not found at ${POLIS_BIN}"
}

# Helper: get current VM state (Running, Stopped, NotFound)
vm_state() {
    multipass info polis --format json 2>/dev/null \
        | jq -r '.info.polis.state // "NotFound"' 2>/dev/null || echo "NotFound"
}

require_state() {
    local expected="$1"
    local actual
    actual=$(vm_state)
    [[ "${actual}" == "${expected}" ]] || skip "need VM in ${expected} state (currently: ${actual})"
}

# =============================================================================
# PHASE 1 — stop a running VM
# =============================================================================

@test "lifecycle: stop exits 0" {
    require_state "Running"
    run polis stop
    assert_success
}

@test "lifecycle: status shows Stopped after stop" {
    require_state "Stopped"
    run polis status
    assert_success
    assert_output --partial "Stopped"
}

@test "lifecycle: stop is idempotent on stopped VM" {
    require_state "Stopped"
    run polis stop
    assert_success
}

@test "lifecycle: exec fails when VM is stopped" {
    require_state "Stopped"
    run polis exec echo hi </dev/null
    assert_failure
}

@test "lifecycle: connect --info fails when VM is stopped" {
    require_state "Stopped"
    run polis connect --info
    assert_failure
}

# =============================================================================
# PHASE 2 — restart from stopped
# =============================================================================

@test "lifecycle: start after stop exits 0" {
    require_state "Stopped"
    run polis -y start
    assert_success
}

@test "lifecycle: status shows Running after restart" {
    require_state "Running"
    run polis status
    assert_success
    assert_output --partial "Running"
}

@test "lifecycle: exec works after restart" {
    require_state "Running"
    run polis exec echo alive </dev/null
    assert_success
    assert_output --partial "alive"
}

# =============================================================================
# PHASE 3 — delete
# =============================================================================

@test "lifecycle: delete -y --no-backup exits 0" {
    require_state "Running"
    run polis delete -y --no-backup
    assert_success
}

@test "lifecycle: status shows no workspace after delete" {
    run polis status
    assert_success
    refute_output --partial "Running"
}

# =============================================================================
# PHASE 4 — fresh start after delete
# =============================================================================

@test "lifecycle: start after delete exits 0 (fresh create)" {
    require_state "NotFound"
    run polis -y start
    assert_success
}

@test "lifecycle: status shows Running after fresh start" {
    require_state "Running"
    run polis status
    assert_success
    assert_output --partial "Running"
}

# =============================================================================
# PHASE 5 — delete --all and recover
# =============================================================================

@test "lifecycle: delete --all -y --no-backup exits 0" {
    require_state "Running"
    run polis delete --all -y --no-backup
    assert_success
}

@test "lifecycle: start after delete --all exits 0 (full reinstall)" {
    require_state "NotFound"
    run polis -y start
    assert_success
}

# =============================================================================
# PHASE 6 — update
# =============================================================================

@test "lifecycle: update exits 0 (applies or reports up-to-date)" {
    require_state "Running"
    run polis -y update
    assert_success
}
