#!/usr/bin/env bats
# bats file_tags=unit,scripts,install
# Tests for scripts/install.sh: resolve_version, run_init, main flow

setup() {
    load "../../lib/test_helper.bash"
    INSTALL_SH="$PROJECT_ROOT/scripts/install.sh"
    TEST_DIR="$(mktemp -d)"
    export POLIS_HOME="$TEST_DIR"
}

teardown() {
    rm -rf "$TEST_DIR"
    unset POLIS_HOME POLIS_VERSION
}

# Source only function definitions (everything except the final `main` invocation).
_source_functions() {
    source <(sed '$d' "$INSTALL_SH")
}

# Run the full script with real-work functions stubbed out.
_run_script() {
    bash -c '
        INSTALL_SH="$1"; shift
        source <(sed "\$d" "$INSTALL_SH")
        check_multipass()     { log_ok "Multipass stub OK"; }
        resolve_version()     { log_info "Installing Polis ${VERSION}"; }
        download_cli()        { mkdir -p "${INSTALL_DIR}/bin"; printf "#!/bin/bash\necho \"polis \$*\"\n" > "${INSTALL_DIR}/bin/polis"; chmod +x "${INSTALL_DIR}/bin/polis"; }
        download_image()      { echo "/tmp/fake.qcow2"; }
        verify_attestation()  { :; }
        create_symlink()      { :; }
        multipass()           { return 1; }  # stub: no existing VM
        main
    ' _ "$INSTALL_SH" "$@"
}

# ── Structural ────────────────────────────────────────────────────────────

@test "install: install.sh exists and is executable" {
    [ -x "$INSTALL_SH" ]
}

@test "install: passes bash syntax check" {
    run bash -n "$INSTALL_SH"
    assert_success
}

@test "install: passes shellcheck" {
    command -v shellcheck >/dev/null 2>&1 || skip "shellcheck not installed"
    run shellcheck "$INSTALL_SH"
    assert_success
}

@test "install: has set -euo pipefail" {
    run grep -q "set -euo pipefail" "$INSTALL_SH"
    assert_success
}

# ── VERSION handling ──────────────────────────────────────────────────────
# VERSION is now hardcoded in install.sh (no resolve_version function).
# These tests verify the VERSION variable is set correctly.

@test "install: VERSION variable is set" {
    run grep -E "^VERSION=" "$INSTALL_SH"
    assert_success
}

# ── main() success message ────────────────────────────────────────────────

@test "install: success message shows Polis installed successfully" {
    export POLIS_VERSION="v0.1.0"
    run _run_script
    assert_success
    assert_output --partial "Polis installed successfully"
}

@test "install: no Get started hints in output" {
    export POLIS_VERSION="v0.1.0"
    run _run_script
    assert_success
    refute_output --partial "Get started"
}
