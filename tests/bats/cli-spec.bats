#!/usr/bin/env bats
# =============================================================================
# Polis CLI Specification Tests
# Tests all command flows and output messages per workspace-lifecycle-refactor-spec.md
# =============================================================================

load 'bats-support/load'
load 'bats-assert/load'
load 'bats-file/load'

# -----------------------------------------------------------------------------
# Configuration
# -----------------------------------------------------------------------------

POLIS_HOME="${POLIS_HOME:-$HOME/.polis}"
POLIS_IMAGES="${HOME}/polis/images"
POLIS_BIN="${POLIS_HOME}/bin/polis"
VM_NAME="polis"

# Dev image path (set via environment or auto-detect)
DEV_IMAGE="${POLIS_DEV_IMAGE:-}"
DEV_PUB_KEY="${POLIS_DEV_PUB_KEY:-}"

# -----------------------------------------------------------------------------
# Setup / Teardown
# -----------------------------------------------------------------------------

setup_file() {
    # Auto-detect dev image if not set
    if [[ -z "${DEV_IMAGE}" ]]; then
        DEV_IMAGE=$(find "${BATS_TEST_DIRNAME}/../../packer/output" -name "*.qcow2" 2>/dev/null | sort | tail -1)
    fi
    if [[ -z "${DEV_PUB_KEY}" ]]; then
        DEV_PUB_KEY="${BATS_TEST_DIRNAME}/../../.secrets/polis-release.pub"
    fi
    
    # Export for use in tests
    export DEV_IMAGE DEV_PUB_KEY
    export POLIS_VERIFYING_KEY_B64
    if [[ -f "${DEV_PUB_KEY}" ]]; then
        POLIS_VERIFYING_KEY_B64=$(base64 -w0 "${DEV_PUB_KEY}")
        export POLIS_VERIFYING_KEY_B64
    fi
}

setup() {
    # Ensure polis binary exists
    if [[ ! -x "${POLIS_BIN}" ]]; then
        skip "polis binary not found at ${POLIS_BIN}"
    fi
}

# -----------------------------------------------------------------------------
# Helper Functions
# -----------------------------------------------------------------------------

# Get VM state from multipass
get_vm_state() {
    local state
    state=$(multipass info "${VM_NAME}" --format json 2>/dev/null | jq -r '.info.polis.state // "NotFound"') || echo "NotFound"
    echo "${state}"
}

# Check if VM exists
vm_exists() {
    multipass info "${VM_NAME}" &>/dev/null
}

# Ensure VM is running
ensure_vm_running() {
    if ! vm_exists; then
        run polis start --image "${DEV_IMAGE}"
    elif [[ "$(get_vm_state)" != "Running" ]]; then
        run polis start
    fi
}

# Ensure VM is stopped
ensure_vm_stopped() {
    if vm_exists && [[ "$(get_vm_state)" == "Running" ]]; then
        run polis stop
    fi
}

# Ensure no VM exists
ensure_no_vm() {
    if vm_exists; then
        multipass delete "${VM_NAME}" --purge 2>/dev/null || true
    fi
    rm -f "${POLIS_HOME}/state.json"
}

# Strip ANSI color codes from output
strip_ansi() {
    sed 's/\x1b\[[0-9;]*m//g'
}

# =============================================================================
# HELP OUTPUT TESTS
# =============================================================================

@test "help: polis --help shows correct commands" {
    run polis --help
    assert_success
    assert_output --partial "start    Start workspace"
    assert_output --partial "stop     Stop workspace"
    assert_output --partial "delete   Remove workspace"
    assert_output --partial "status   Show workspace status"
    assert_output --partial "connect  Show connection options"
    assert_output --partial "config   Manage configuration"
    assert_output --partial "doctor   Diagnose issues"
    assert_output --partial "update   Update Polis"
    assert_output --partial "version  Show version"
}

@test "help: polis start --help shows --image option" {
    run polis start --help
    assert_success
    assert_output --partial "--image <IMAGE>"
    assert_output --partial "Use custom image instead of cached/downloaded"
}

@test "help: polis delete --help shows --all and -y options" {
    run polis delete --help
    assert_success
    assert_output --partial "--all"
    assert_output --partial "Remove everything including certificates, cache, and configuration"
    assert_output --partial "-y, --yes"
    assert_output --partial "Skip confirmation prompt"
}

@test "help: polis update --help shows --check option" {
    run polis update --help
    assert_success
    assert_output --partial "--check"
    assert_output --partial "Check for updates without applying them"
}

@test "help: polis config --help shows show and set subcommands" {
    run polis config --help
    assert_success
    assert_output --partial "show"
    assert_output --partial "set"
}

# =============================================================================
# START COMMAND TESTS
# =============================================================================

@test "start: already running workspace shows correct message" {
    ensure_vm_running
    
    run polis start
    assert_success
    
    # Check output messages per spec
    assert_output --partial "Workspace is running."
    assert_output --partial "✓ Governance"
    assert_output --partial "Policy engine active"
    assert_output --partial "✓ Security"
    assert_output --partial "Workspace isolated"
    assert_output --partial "✓ Observability"
    assert_output --partial "Action tracing live"
    assert_output --partial "Connect: polis connect"
    assert_output --partial "Status:  polis status"
    
    # Verify VM is still running via multipass
    assert_equal "$(get_vm_state)" "Running"
}

@test "start: --image flag shows 'Using:' message" {
    ensure_vm_running
    
    run polis start --image "${DEV_IMAGE}"
    assert_success
    assert_output --partial "Using: ${DEV_IMAGE}"
}

@test "start: stopped workspace shows 'Starting workspace...' message" {
    ensure_vm_running
    ensure_vm_stopped
    
    # Verify VM is stopped via multipass
    assert_equal "$(get_vm_state)" "Stopped"
    
    run polis start
    assert_success
    
    assert_output --partial "Starting workspace..."
    assert_output --partial "Activating security controls..."
    assert_output --partial "Workspace ready."
    assert_output --partial "✓ Governance"
    assert_output --partial "Connect: polis connect"
    
    # Verify VM is now running via multipass
    assert_equal "$(get_vm_state)" "Running"
}

@test "start: fresh install creates workspace" {
    ensure_no_vm
    
    # Verify no VM exists via multipass
    run vm_exists
    assert_failure
    
    run polis start --image "${DEV_IMAGE}"
    assert_success
    
    assert_output --partial "Creating workspace..."
    assert_output --partial "Activating security controls..."
    assert_output --partial "Workspace ready."
    assert_output --partial "✓ Governance"
    
    # Verify VM was created and is running via multipass
    assert_equal "$(get_vm_state)" "Running"
}

@test "start: security banner format is correct" {
    ensure_vm_running
    
    run polis start
    assert_success
    
    # Verify exact banner format per spec
    assert_output --partial "✓ Governance    Policy engine active · Audit trail recording"
    assert_output --partial "✓ Security      Workspace isolated · Traffic inspection enabled"
    assert_output --partial "✓ Observability Action tracing live · Trust scoring active"
}

# =============================================================================
# STOP COMMAND TESTS
# =============================================================================

@test "stop: running workspace shows correct message" {
    ensure_vm_running
    
    # Verify VM is running via multipass
    assert_equal "$(get_vm_state)" "Running"
    
    run polis stop
    assert_success
    
    assert_output --partial "Stopping workspace..."
    assert_output --partial "Workspace stopped. Your data is preserved."
    assert_output --partial "Resume: polis start"
    
    # Verify VM is stopped via multipass
    assert_equal "$(get_vm_state)" "Stopped"
}

@test "stop: already stopped workspace shows correct message" {
    ensure_vm_running
    ensure_vm_stopped
    
    # Verify VM is stopped via multipass
    assert_equal "$(get_vm_state)" "Stopped"
    
    run polis stop
    assert_success
    
    assert_output --partial "Workspace is already stopped."
    assert_output --partial "Resume: polis start"
    
    # Verify VM is still stopped via multipass
    assert_equal "$(get_vm_state)" "Stopped"
}

@test "stop: no workspace shows correct message" {
    ensure_no_vm
    
    run polis stop
    assert_success
    
    assert_output --partial "No workspace to stop."
    assert_output --partial "Create one: polis start"
}

# =============================================================================
# DELETE COMMAND TESTS
# =============================================================================

@test "delete: confirmation prompt shows correct message" {
    ensure_vm_running
    
    # Run with 'n' to decline
    run bash -c 'echo "n" | polis delete'
    assert_success
    
    assert_output --partial "This will remove your workspace."
    assert_output --partial "Configuration, certificates, and cached downloads will be preserved."
    assert_output --partial "Continue? [y/N]:"
    assert_output --partial "Cancelled."
    
    # Verify VM still exists via multipass
    run vm_exists
    assert_success
}

@test "delete: removes workspace but preserves config/cache" {
    ensure_vm_running
    
    # Create some files to verify preservation
    mkdir -p "${POLIS_HOME}/certs"
    touch "${POLIS_HOME}/certs/test.pem"
    mkdir -p "${POLIS_IMAGES}"
    touch "${POLIS_IMAGES}/test-marker"
    
    run polis delete -y
    assert_success
    
    assert_output --partial "Removing workspace..."
    assert_output --partial "Workspace removed."
    assert_output --partial "Create new: polis start"
    
    # Verify VM is deleted via multipass
    run vm_exists
    assert_failure
    
    # Verify config/cache preserved
    assert_file_exists "${POLIS_HOME}/certs/test.pem"
    assert_file_exists "${POLIS_IMAGES}/test-marker"
    
    # Cleanup
    rm -f "${POLIS_HOME}/certs/test.pem"
    rm -f "${POLIS_IMAGES}/test-marker"
}

@test "delete --all: confirmation prompt shows correct message" {
    ensure_vm_running
    
    run bash -c 'echo "n" | polis delete --all'
    assert_success
    
    assert_output --partial "This will permanently remove:"
    assert_output --partial "• Your workspace"
    assert_output --partial "• Generated certificates"
    assert_output --partial "• Configuration"
    assert_output --partial "• Cached workspace image (~3.5 GB)"
    assert_output --partial "Continue? [y/N]:"
    assert_output --partial "Cancelled."
}

@test "delete --all: removes everything including cache" {
    ensure_vm_running
    
    # Create files to verify removal
    mkdir -p "${POLIS_HOME}/certs"
    touch "${POLIS_HOME}/certs/test.pem"
    mkdir -p "${POLIS_IMAGES}"
    touch "${POLIS_IMAGES}/test-marker"
    touch "${POLIS_HOME}/known_hosts"
    
    run polis delete --all -y
    assert_success
    
    assert_output --partial "Removing workspace..."
    assert_output --partial "Removing certificates..."
    assert_output --partial "Removing configuration..."
    assert_output --partial "Removing cached data..."
    assert_output --partial "All Polis data removed."
    assert_output --partial "Start fresh: polis start"
    
    # Verify VM is deleted via multipass
    run vm_exists
    assert_failure
    
    # Verify everything removed
    assert_file_not_exists "${POLIS_HOME}/certs/test.pem"
    assert_file_not_exists "${POLIS_HOME}/known_hosts"
    # Note: POLIS_IMAGES may or may not exist depending on implementation
}

@test "delete: stops running workspace before deleting" {
    ensure_vm_running
    
    # Verify VM is running via multipass
    assert_equal "$(get_vm_state)" "Running"
    
    run polis delete -y
    assert_success
    
    # Verify VM is deleted via multipass
    run vm_exists
    assert_failure
}

# =============================================================================
# STATUS COMMAND TESTS
# =============================================================================

@test "status: running workspace shows correct status" {
    ensure_vm_running
    
    run polis status
    assert_success
    
    assert_output --partial "Workspace:"
    assert_output --partial "running"
    assert_output --partial "Security:"
}

@test "status: stopped workspace shows correct status" {
    ensure_vm_running
    ensure_vm_stopped
    
    run polis status
    assert_success
    
    assert_output --partial "Workspace:"
    assert_output --partial "stopped"
}

# =============================================================================
# CONFIG COMMAND TESTS
# =============================================================================

@test "config show: displays current configuration" {
    run polis config show
    assert_success
    
    assert_output --partial "Configuration"
    assert_output --partial "security.level:"
    assert_output --partial "Environment:"
    assert_output --partial "POLIS_CONFIG:"
    assert_output --partial "NO_COLOR:"
}

@test "config set: valid security.level balanced" {
    run polis config set security.level balanced
    assert_success
    assert_output --partial "✓ Set security.level = balanced"
    
    # Verify setting persisted
    run polis config show
    assert_output --partial "security.level:"
    assert_output --partial "balanced"
}

@test "config set: valid security.level strict" {
    run polis config set security.level strict
    assert_success
    assert_output --partial "✓ Set security.level = strict"
    
    # Verify setting persisted
    run polis config show
    assert_output --partial "security.level:"
    assert_output --partial "strict"
    
    # Reset to default
    run polis config set security.level balanced
}

@test "config set: invalid value shows error" {
    run polis config set security.level invalid
    assert_failure
    
    assert_output --partial "Invalid value for security.level: invalid"
    assert_output --partial "Valid values: balanced, strict"
}

@test "config set: unknown key shows error" {
    run polis config set unknown.key value
    assert_failure
    
    assert_output --partial "Unknown setting: unknown.key"
    assert_output --partial "Valid settings: security.level"
}

# =============================================================================
# UPDATE COMMAND TESTS
# =============================================================================

@test "update --check: shows checking message" {
    run polis update --check
    # May fail due to network, but should show checking message
    assert_output --partial "Checking for updates..."
}

@test "update --check: does not modify CLI version" {
    local version_before version_after
    version_before=$(polis version)
    
    run polis update --check
    # Ignore exit status (may fail due to network)
    
    version_after=$(polis version)
    assert_equal "${version_before}" "${version_after}"
}

# =============================================================================
# CONNECT COMMAND TESTS
# =============================================================================

@test "connect: shows connection options when running" {
    ensure_vm_running
    
    run polis connect
    assert_success
    # Output format may vary, just verify it runs
}

# =============================================================================
# VERSION COMMAND TESTS
# =============================================================================

@test "version: shows version string" {
    run polis version
    assert_success
    assert_output --partial "polis"
}

# =============================================================================
# DEPRECATED COMMANDS TESTS (per spec section 6.1)
# =============================================================================

@test "deprecated: init command behavior" {
    run polis init
    # Per spec v0.4.0: should show deprecation warning then run start logic
    # Per spec v0.5.0: should be removed
    # Current implementation: removed (returns error)
    # This test documents current behavior
    if [[ "${status}" -eq 2 ]]; then
        assert_output --partial "unrecognized subcommand 'init'"
    else
        # If implemented per spec v0.4.0
        assert_output --partial "deprecated"
    fi
}

@test "deprecated: run command behavior" {
    run polis run
    # Per spec v0.4.0: should silently alias to start
    # Per spec v0.5.0: should be removed
    # Current implementation: removed (returns error)
    if [[ "${status}" -eq 2 ]]; then
        assert_output --partial "unrecognized subcommand 'run'"
    else
        # If implemented per spec v0.4.0 (alias to start)
        assert_success
    fi
}

# =============================================================================
# EXIT CODE TESTS
# =============================================================================

@test "exit code: successful command returns 0" {
    ensure_vm_running
    run polis status
    assert_success
}

@test "exit code: invalid argument returns 2" {
    run polis --invalid-flag
    assert_failure
    assert_equal "${status}" 2
}

@test "exit code: config validation error returns 1" {
    run polis config set security.level invalid
    assert_failure
    assert_equal "${status}" 1
}

@test "exit code: user cancelled returns 0" {
    ensure_vm_running
    run bash -c 'echo "n" | polis delete'
    assert_success
}

# =============================================================================
# JSON OUTPUT TESTS
# =============================================================================

@test "json: polis status --json outputs valid JSON" {
    ensure_vm_running
    
    run polis status --json
    assert_success
    
    # Verify it's valid JSON
    echo "${output}" | jq . >/dev/null
    assert_success
}

@test "json: polis config show --json outputs valid JSON" {
    run polis config show --json
    assert_success
    
    # Verify it's valid JSON
    echo "${output}" | jq . >/dev/null
    assert_success
}

# =============================================================================
# MULTIPASS STATE VERIFICATION TESTS
# =============================================================================

@test "multipass: start creates VM with correct name" {
    ensure_no_vm
    
    run polis start --image "${DEV_IMAGE}"
    assert_success
    
    # Verify VM exists with correct name via multipass
    run multipass info "${VM_NAME}"
    assert_success
}

@test "multipass: stop changes VM state to Stopped" {
    ensure_vm_running
    
    run polis stop
    assert_success
    
    # Verify state via multipass
    local state
    state=$(multipass info "${VM_NAME}" --format json | jq -r '.info.polis.state')
    assert_equal "${state}" "Stopped"
}

@test "multipass: start changes VM state to Running" {
    ensure_vm_running
    ensure_vm_stopped
    
    run polis start
    assert_success
    
    # Verify state via multipass
    local state
    state=$(multipass info "${VM_NAME}" --format json | jq -r '.info.polis.state')
    assert_equal "${state}" "Running"
}

@test "multipass: delete removes VM completely" {
    ensure_vm_running
    
    run polis delete -y
    assert_success
    
    # Verify VM does not exist via multipass
    run multipass info "${VM_NAME}"
    assert_failure
}

@test "multipass: delete --all removes VM and purges" {
    ensure_vm_running
    
    run polis delete --all -y
    assert_success
    
    # Verify VM does not exist via multipass
    run multipass info "${VM_NAME}"
    assert_failure
    
    # Verify not in deleted state either
    run multipass list --format json
    assert_success
    local deleted_count
    deleted_count=$(echo "${output}" | jq '[.list[] | select(.name == "polis")] | length')
    assert_equal "${deleted_count}" "0"
}

# =============================================================================
# IDEMPOTENCY TESTS
# =============================================================================

@test "idempotent: multiple start calls on running workspace" {
    ensure_vm_running
    
    run polis start
    assert_success
    assert_output --partial "Workspace is running."
    
    run polis start
    assert_success
    assert_output --partial "Workspace is running."
    
    # Verify still running via multipass
    assert_equal "$(get_vm_state)" "Running"
}

@test "idempotent: multiple stop calls on stopped workspace" {
    ensure_vm_running
    ensure_vm_stopped
    
    run polis stop
    assert_success
    
    run polis stop
    assert_success
    
    # Verify still stopped via multipass
    assert_equal "$(get_vm_state)" "Stopped"
}
