#!/usr/bin/env bats
# bats file_tags=unit,scripts,install
# Tests for scripts/install.sh (issue 05): flag parsing, init_image, resolve_version

setup() {
    load "../../lib/test_helper.bash"
    INSTALL_SH="$PROJECT_ROOT/scripts/install.sh"
    TEST_DIR="$(mktemp -d)"
    export POLIS_HOME="$TEST_DIR"  # picked up by INSTALL_DIR="${POLIS_HOME:-...}" in install.sh
}

teardown() {
    rm -rf "$TEST_DIR"
    unset POLIS_HOME
}

# Source only the function definitions (stop before the flag-parsing execution block).
_source_functions() {
    source <(awk '/^# Parse flags/{exit} {print}' "$INSTALL_SH")
}

# Run the full script with real-work functions stubbed out.
# Two-phase source: load functions, override stubs, then run the execution block.
# Extra args are forwarded as flags to install.sh.
_run_script() {
    bash -c '
        INSTALL_SH="$1"; shift
        source <(awk "/^# Parse flags/{exit} {print}" "$INSTALL_SH")
        download_and_verify() { :; }
        verify_attestation()  { :; }
        create_symlink()      { :; }
        init_image()          { :; }
        curl()                { printf '"'"'{"tag_name":"v0.1.0"}\n200'"'"'; }
        multipass()           { return 0; }
        source <(awk "/^# Parse flags/,0" "$INSTALL_SH")
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

# ── Flag parsing: --image ─────────────────────────────────────────────────

@test "install: --image flag is accepted" {
    run _run_script --version v0.1.0 --image https://example.com/img.qcow2
    assert_success
}

@test "install: --image= (equals form) is accepted" {
    run _run_script --version v0.1.0 --image=https://example.com/img.qcow2
    assert_success
}

@test "install: --image without value exits 1 with error" {
    run _run_script --image
    assert_failure
    assert_output --partial "--image requires a value"
}

# ── Flag parsing: --version ───────────────────────────────────────────────

@test "install: --version flag pins version in output" {
    run _run_script --version v0.3.0
    assert_success
    assert_output --partial "v0.3.0"
}

@test "install: --version= (equals form) pins version in output" {
    run _run_script --version=v0.3.0
    assert_success
    assert_output --partial "v0.3.0"
}

@test "install: --version without value exits 1 with error" {
    run _run_script --version
    assert_failure
    assert_output --partial "--version requires a value"
}

# ── Flag parsing: unknown flag ────────────────────────────────────────────

@test "install: unknown flag exits 1 with error message" {
    run _run_script --unknown
    assert_failure
    assert_output --partial "Unknown flag: --unknown"
}

# ── init_image() ──────────────────────────────────────────────────────────

@test "init_image: calls polis init without --image when IMAGE_URL is empty" {
    _source_functions
    mkdir -p "$TEST_DIR/bin"
    printf '#!/bin/bash\necho "polis $*"\n' > "$TEST_DIR/bin/polis"
    chmod +x "$TEST_DIR/bin/polis"
    IMAGE_URL=""
    run init_image
    assert_success
    assert_output --partial "polis init"
    refute_output --partial "--image"
}

@test "init_image: calls polis init --image <url> when IMAGE_URL is set" {
    _source_functions
    mkdir -p "$TEST_DIR/bin"
    printf '#!/bin/bash\necho "polis $*"\n' > "$TEST_DIR/bin/polis"
    chmod +x "$TEST_DIR/bin/polis"
    IMAGE_URL="https://example.com/img.qcow2"
    run init_image
    assert_success
    assert_output --partial "init --image https://example.com/img.qcow2"
}

@test "init_image: non-fatal when polis init exits non-zero" {
    _source_functions
    mkdir -p "$TEST_DIR/bin"
    printf '#!/bin/bash\nexit 1\n' > "$TEST_DIR/bin/polis"
    chmod +x "$TEST_DIR/bin/polis"
    IMAGE_URL=""
    run init_image
    assert_success
    assert_output --partial "Image download failed. Run 'polis init' to retry."
}

@test "init_image: non-fatal when polis binary does not exist" {
    _source_functions
    # No binary at INSTALL_DIR/bin/polis
    IMAGE_URL=""
    run init_image
    assert_success
    assert_output --partial "Image download failed. Run 'polis init' to retry."
}

# ── resolve_version() ─────────────────────────────────────────────────────

@test "resolve_version: HTTP 403 exits 1 with rate limit message" {
    _source_functions
    curl() { printf '{"message":"rate limited"}\n403'; }
    VERSION="latest"
    run resolve_version
    assert_failure
    assert_output --partial "GitHub API rate limit exceeded"
    assert_output --partial "GITHUB_TOKEN"
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

@test "install: success message shows polis run" {
    run _run_script --version v0.1.0
    assert_success
    assert_output --partial "polis run"
}

@test "install: success message shows polis run claude" {
    run _run_script --version v0.1.0
    assert_success
    assert_output --partial "polis run claude"
}
