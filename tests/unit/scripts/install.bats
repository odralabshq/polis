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
        download_cli()        { :; }
        download_image()      { echo "/tmp/fake.qcow2"; }
        verify_attestation()  { :; }
        create_symlink()      { :; }
        run_init()            { :; }
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

# ── run_init() ────────────────────────────────────────────────────────────

@test "run_init: calls polis start --image with given path" {
    _source_functions
    mkdir -p "$TEST_DIR/bin"
    printf '#!/bin/bash\necho "polis $*"\n' > "$TEST_DIR/bin/polis"
    chmod +x "$TEST_DIR/bin/polis"
    multipass() { return 1; }
    run run_init "/tmp/test.qcow2"
    assert_success
    assert_output --partial "polis start --image /tmp/test.qcow2"
}

@test "run_init: non-fatal when polis start exits non-zero" {
    _source_functions
    mkdir -p "$TEST_DIR/bin"
    printf '#!/bin/bash\nexit 1\n' > "$TEST_DIR/bin/polis"
    chmod +x "$TEST_DIR/bin/polis"
    multipass() { return 1; }
    run run_init "/tmp/test.qcow2"
    assert_success
    assert_output --partial "polis start failed"
}

@test "run_init: non-fatal when polis binary does not exist" {
    _source_functions
    multipass() { return 1; }
    run run_init "/tmp/test.qcow2"
    assert_success
    assert_output --partial "polis start failed"
}

# ── resolve_version() ─────────────────────────────────────────────────────

@test "resolve_version: HTTP 403 exits 1 with rate limit message" {
    _source_functions
    curl() { printf '{"message":"rate limited"}\n403'; }
    VERSION="latest"
    run resolve_version
    assert_failure
    assert_output --partial "GitHub API rate limit exceeded"
    assert_output --partial "POLIS_VERSION"
}

@test "resolve_version: empty tag_name exits 1" {
    _source_functions
    curl() { printf '{"tag_name":""}\n200'; }
    VERSION="latest"
    run resolve_version
    assert_failure
    assert_output --partial "Failed to resolve latest version"
}

@test "resolve_version: null tag_name exits 1" {
    _source_functions
    curl() { printf '{"tag_name":null}\n200'; }
    VERSION="latest"
    run resolve_version
    assert_failure
    assert_output --partial "Failed to resolve latest version"
}

@test "resolve_version: valid response resolves version with jq" {
    command -v jq >/dev/null 2>&1 || skip "jq not installed"
    _source_functions
    curl() { printf '{"tag_name":"v1.2.3"}\n200'; }
    VERSION="latest"
    run resolve_version
    assert_success
    assert_output --partial "v1.2.3"
}

@test "resolve_version: pinned version skips API call and logs version" {
    _source_functions
    curl() { fail "curl must not be called for a pinned version"; }
    VERSION="v0.3.0"
    run resolve_version
    assert_success
    assert_output --partial "v0.3.0"
}

# ── main() success message ────────────────────────────────────────────────

@test "install: success message shows polis start" {
    export POLIS_VERSION="v0.1.0"
    run _run_script
    assert_success
    assert_output --partial "polis start"
}

@test "install: success message shows polis start claude" {
    export POLIS_VERSION="v0.1.0"
    run _run_script
    assert_success
    assert_output --partial "polis start claude"
}
