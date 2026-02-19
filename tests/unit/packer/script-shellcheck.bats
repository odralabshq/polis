#!/usr/bin/env bats
# bats file_tags=unit,packer
# Shellcheck validation for Packer provisioner scripts

setup() {
    load "../../lib/test_helper.bash"
    PACKER_SCRIPTS="$PROJECT_ROOT/packer/scripts"
}

# ── Shellcheck validation ─────────────────────────────────────────────────

@test "shellcheck: install-docker.sh passes" {
    skip_if_no_shellcheck
    run shellcheck -S error "$PACKER_SCRIPTS/install-docker.sh"
    assert_success
}

@test "shellcheck: install-sysbox.sh passes" {
    skip_if_no_shellcheck
    run shellcheck -S error "$PACKER_SCRIPTS/install-sysbox.sh"
    assert_success
}

@test "shellcheck: load-images.sh passes" {
    skip_if_no_shellcheck
    run shellcheck -S error "$PACKER_SCRIPTS/load-images.sh"
    assert_success
}

@test "shellcheck: harden-vm.sh passes" {
    skip_if_no_shellcheck
    run shellcheck -S error "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

@test "shellcheck: install-polis.sh passes" {
    skip_if_no_shellcheck
    run shellcheck -S error "$PACKER_SCRIPTS/install-polis.sh"
    assert_success
}

@test "shellcheck: setup-certs.sh passes" {
    skip_if_no_shellcheck
    run shellcheck -S error "$PACKER_SCRIPTS/setup-certs.sh"
    assert_success
}

@test "shellcheck: bundle-polis-config.sh passes" {
    skip_if_no_shellcheck
    run shellcheck -S error "$PACKER_SCRIPTS/bundle-polis-config.sh"
    assert_success
}

# ── Script structure ──────────────────────────────────────────────────────

@test "scripts: all have set -euo pipefail" {
    for script in "$PACKER_SCRIPTS"/*.sh; do
        run grep -q "set -euo pipefail" "$script"
        assert_success "Missing 'set -euo pipefail' in $(basename "$script")"
    done
}

@test "scripts: all are executable" {
    for script in "$PACKER_SCRIPTS"/*.sh; do
        [ -x "$script" ] || fail "$(basename "$script") is not executable"
    done
}

# ── Helper ────────────────────────────────────────────────────────────────

skip_if_no_shellcheck() {
    command -v shellcheck >/dev/null 2>&1 || skip "shellcheck not installed"
}
