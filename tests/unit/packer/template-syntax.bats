#!/usr/bin/env bats
# bats file_tags=unit,packer
# Packer template syntax and configuration validation

setup() {
    load "../../lib/test_helper.bash"
    PACKER_DIR="$PROJECT_ROOT/packer"
    PACKER_TEMPLATE="$PACKER_DIR/polis-vm.pkr.hcl"
}

# ── Template syntax ───────────────────────────────────────────────────────

@test "packer: template file exists" {
    [ -f "$PACKER_TEMPLATE" ]
}

@test "packer: validate syntax (syntax-only)" {
    skip_if_no_packer
    cd "$PACKER_DIR"
    run packer validate -syntax-only polis-vm.pkr.hcl
    assert_success
}

@test "packer: fmt check passes" {
    skip_if_no_packer
    cd "$PACKER_DIR"
    run packer fmt -check polis-vm.pkr.hcl
    assert_success
}

# ── Required plugins ──────────────────────────────────────────────────────

@test "packer: qemu plugin declared" {
    run grep -E 'qemu\s*=' "$PACKER_TEMPLATE"
    assert_success
}

@test "packer: goss plugin declared" {
    run grep -E 'goss\s*=' "$PACKER_TEMPLATE"
    assert_success
}

# ── Variables ─────────────────────────────────────────────────────────────

@test "packer: polis_version variable defined" {
    run grep 'variable "polis_version"' "$PACKER_TEMPLATE"
    assert_success
}

@test "packer: sysbox_version variable defined" {
    run grep 'variable "sysbox_version"' "$PACKER_TEMPLATE"
    assert_success
}

@test "packer: sysbox SHA256 variables defined" {
    run grep 'variable "sysbox_sha256_amd64"' "$PACKER_TEMPLATE"
    assert_success
    run grep 'variable "sysbox_sha256_arm64"' "$PACKER_TEMPLATE"
    assert_success
}

# ── Build configuration ───────────────────────────────────────────────────

@test "packer: uses qcow2 format" {
    run grep 'format.*=.*"qcow2"' "$PACKER_TEMPLATE"
    assert_success
}

@test "packer: goss provisioner configured" {
    run grep 'provisioner "goss"' "$PACKER_TEMPLATE"
    assert_success
}

@test "packer: goss tests path configured" {
    run grep 'tests.*=.*\["goss/goss.yaml"\]' "$PACKER_TEMPLATE"
    assert_success
}

# ── Helper ────────────────────────────────────────────────────────────────

skip_if_no_packer() {
    command -v packer >/dev/null 2>&1 || skip "packer not installed"
}
