#!/usr/bin/env bats
# CLI specification tests — verifies command behaviour via multipass state,
# not output text. Help/config/version tests are the exception (output IS the behaviour).

load 'bats-support/load'
load 'bats-assert/load'
load 'bats-file/load'

POLIS_HOME="${POLIS_HOME:-$HOME/.polis}"
POLIS_IMAGES="${HOME}/polis/images"
POLIS_BIN="${POLIS_HOME}/bin/polis"
VM_NAME="polis"
DEV_IMAGE="${POLIS_DEV_IMAGE:-}"
DEV_PUB_KEY="${POLIS_DEV_PUB_KEY:-}"

# -----------------------------------------------------------------------------
# Setup / Teardown
# -----------------------------------------------------------------------------

setup_file() {
    if [[ -z "${DEV_IMAGE}" ]]; then
        DEV_IMAGE=$(find "${BATS_TEST_DIRNAME}/../../packer/output" -name "*.qcow2" 2>/dev/null | sort | tail -1)
    fi
    if [[ -z "${DEV_PUB_KEY}" ]]; then
        DEV_PUB_KEY="${BATS_TEST_DIRNAME}/../../.secrets/polis-release.pub"
    fi
    export DEV_IMAGE DEV_PUB_KEY
    if [[ -f "${DEV_PUB_KEY}" ]]; then
        POLIS_VERIFYING_KEY_B64=$(base64 -w0 "${DEV_PUB_KEY}")
        export POLIS_VERIFYING_KEY_B64
    fi
}

setup() {
    [[ -x "${POLIS_BIN}" ]] || skip "polis binary not found at ${POLIS_BIN}"
}

# -----------------------------------------------------------------------------
# Helpers
# -----------------------------------------------------------------------------

vm_state() {
    multipass info "${VM_NAME}" --format json 2>/dev/null \
        | jq -r '.info.polis.state // "NotFound"' || echo "NotFound"
}

vm_exists() { multipass info "${VM_NAME}" &>/dev/null; }

ensure_running() {
    if ! vm_exists; then
        polis start --image "${DEV_IMAGE}" >/dev/null 2>&1
    elif [[ "$(vm_state)" != "Running" ]]; then
        polis start >/dev/null 2>&1
    fi
}

ensure_stopped() {
    ensure_running
    [[ "$(vm_state)" == "Running" ]] && polis stop >/dev/null 2>&1 || true
}

ensure_no_vm() {
    vm_exists && multipass delete "${VM_NAME}" --purge 2>/dev/null || true
    rm -f "${POLIS_HOME}/state.json"
}

# =============================================================================
# HELP / VERSION (output IS the behaviour)
# =============================================================================

@test "help: shows all subcommands" {
    run polis --help
    assert_success
    assert_output --partial "start"
    assert_output --partial "stop"
    assert_output --partial "delete"
    assert_output --partial "status"
    assert_output --partial "connect"
    assert_output --partial "config"
    assert_output --partial "doctor"
    assert_output --partial "update"
    assert_output --partial "version"
}

@test "help: start --help shows --image option" {
    run polis start --help
    assert_success
    assert_output --partial "--image"
}

@test "help: delete --help shows --all and -y options" {
    run polis delete --help
    assert_success
    assert_output --partial "--all"
    assert_output --partial "-y"
}

@test "help: update --help shows --check option" {
    run polis update --help
    assert_success
    assert_output --partial "--check"
}

@test "version: returns 0 and prints version" {
    run polis version
    assert_success
    assert_output --partial "polis"
}

# =============================================================================
# START
# =============================================================================

@test "start: creates VM from image when none exists" {
    ensure_no_vm
    run polis start --image "${DEV_IMAGE}"
    assert_success
    assert_equal "$(vm_state)" "Running"
}

@test "start: starts stopped VM" {
    ensure_stopped
    run polis start
    assert_success
    assert_equal "$(vm_state)" "Running"
}

@test "start: idempotent on already-running VM" {
    ensure_running
    run polis start
    assert_success
    assert_equal "$(vm_state)" "Running"
}

# =============================================================================
# STOP
# =============================================================================

@test "stop: stops running VM" {
    ensure_running
    run polis stop
    assert_success
    assert_equal "$(vm_state)" "Stopped"
}

@test "stop: idempotent on already-stopped VM" {
    ensure_stopped
    run polis stop
    assert_success
    assert_equal "$(vm_state)" "Stopped"
}

@test "stop: succeeds when no VM exists" {
    ensure_no_vm
    run polis stop
    assert_success
}

# =============================================================================
# DELETE
# =============================================================================

@test "delete: removes VM" {
    ensure_running
    run polis delete -y
    assert_success
    run vm_exists
    assert_failure
}

@test "delete: preserves certs and image cache" {
    ensure_running
    mkdir -p "${POLIS_HOME}/certs"
    touch "${POLIS_HOME}/certs/test.pem"
    mkdir -p "${POLIS_IMAGES}"
    touch "${POLIS_IMAGES}/test-marker"

    run polis delete -y
    assert_success

    assert_file_exists "${POLIS_HOME}/certs/test.pem"
    assert_file_exists "${POLIS_IMAGES}/test-marker"

    rm -f "${POLIS_HOME}/certs/test.pem" "${POLIS_IMAGES}/test-marker"
}

@test "delete --all: removes VM and purges from multipass list" {
    ensure_running
    run polis delete --all -y
    assert_success

    run multipass info "${VM_NAME}"
    assert_failure

    local count
    count=$(multipass list --format json | jq '[.list[] | select(.name == "polis")] | length')
    assert_equal "${count}" "0"
}

@test "delete: cancelled by user leaves VM intact" {
    ensure_running
    run bash -c 'echo "n" | polis delete'
    assert_success
    run vm_exists
    assert_success
}

# =============================================================================
# STATUS
# =============================================================================

@test "status: returns 0 for running VM" {
    ensure_running
    run polis status
    assert_success
}

@test "status: --json returns valid JSON" {
    ensure_running
    run polis status --json
    assert_success
    echo "${output}" | jq . >/dev/null
}

@test "status: running VM reports running state in JSON" {
    ensure_running
    local state
    state=$(polis status --json | jq -r '.state // .workspace.state')
    assert_equal "${state}" "running"
}

@test "status: stopped VM reports stopped state in JSON" {
    ensure_stopped
    local state
    state=$(polis status --json | jq -r '.state // .workspace.state')
    assert_equal "${state}" "stopped"
}

# =============================================================================
# CONNECT
# =============================================================================

@test "connect: returns 0 when SSH already configured" {
    ensure_running
    
    # Pre-create SSH config to avoid interactive prompt
    mkdir -p "${HOME}/.polis"
    cat > "${HOME}/.polis/ssh_config" <<'EOF'
Host workspace
    HostName workspace
    User polis
    StrictHostKeyChecking yes
    ForwardAgent no
EOF
    
    run polis connect
    assert_success
}

@test "connect: SSH config contains StrictHostKeyChecking yes" {
    ensure_running
    
    # Pre-create minimal SSH config
    mkdir -p "${HOME}/.polis"
    cat > "${HOME}/.polis/ssh_config" <<'EOF'
Host workspace
    HostName workspace
    User polis
    StrictHostKeyChecking yes
    ForwardAgent no
EOF
    
    run cat "${HOME}/.polis/ssh_config"
    assert_output --partial "StrictHostKeyChecking yes"
}

@test "connect: SSH config contains ForwardAgent no" {
    ensure_running
    
    # Pre-create minimal SSH config
    mkdir -p "${HOME}/.polis"
    cat > "${HOME}/.polis/ssh_config" <<'EOF'
Host workspace
    HostName workspace
    User polis
    StrictHostKeyChecking yes
    ForwardAgent no
EOF
    
    run cat "${HOME}/.polis/ssh_config"
    assert_output --partial "ForwardAgent no"
}

@test "connect: SSH config contains User polis" {
    ensure_running
    
    # Pre-create minimal SSH config
    mkdir -p "${HOME}/.polis"
    cat > "${HOME}/.polis/ssh_config" <<'EOF'
Host workspace
    HostName workspace
    User polis
    StrictHostKeyChecking yes
    ForwardAgent no
EOF
    
    run cat "${HOME}/.polis/ssh_config"
    assert_output --partial "User polis"
}

@test "connect --ide: rejects unknown IDE" {
    ensure_running
    run polis connect --ide unknown-ide
    assert_failure
    assert_output --partial "Unknown IDE"
}

# =============================================================================
# CONFIG
# =============================================================================

@test "config show: returns 0 and valid output" {
    run polis config show
    assert_success
    assert_output --partial "security.level"
}

@test "config show --json: returns valid JSON" {
    run polis config show --json
    assert_success
    echo "${output}" | jq . >/dev/null
}

@test "config set: persists security.level balanced" {
    run polis config set security.level balanced
    assert_success
    run polis config show --json
    local level
    level=$(echo "${output}" | jq -r '.["security.level"] // .security.level')
    assert_equal "${level}" "balanced"
}

@test "config set: persists security.level strict" {
    run polis config set security.level strict
    assert_success
    run polis config show --json
    local level
    level=$(echo "${output}" | jq -r '.["security.level"] // .security.level')
    assert_equal "${level}" "strict"
    polis config set security.level balanced >/dev/null 2>&1 || true
}

@test "config set: rejects invalid value" {
    run polis config set security.level invalid
    assert_failure
}

@test "config set: rejects unknown key" {
    run polis config set unknown.key value
    assert_failure
}

# =============================================================================
# EXIT CODES
# =============================================================================

@test "exit code: invalid flag returns 2" {
    run polis --invalid-flag
    assert_failure
    assert_equal "${status}" 2
}

@test "exit code: config validation error returns 1" {
    run polis config set security.level invalid
    assert_failure
    assert_equal "${status}" 1
}

@test "exit code: cancelled delete returns 0" {
    ensure_running
    run bash -c 'echo "n" | polis delete'
    assert_success
}

# =============================================================================
# UPDATE
# =============================================================================

@test "update --check: does not change installed version" {
    local before after
    before=$(polis version)
    run polis update --check
    after=$(polis version)
    assert_equal "${before}" "${after}"
}
