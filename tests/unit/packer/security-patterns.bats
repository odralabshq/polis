#!/usr/bin/env bats
# bats file_tags=unit,packer,security
# Security pattern validation for Packer scripts

setup() {
    load "../../lib/test_helper.bash"
    PACKER_SCRIPTS="$PROJECT_ROOT/packer/scripts"
    PACKER_TEMPLATE="$PROJECT_ROOT/packer/polis-vm.pkr.hcl"
}

# ── Supply chain security ─────────────────────────────────────────────────

@test "security: Docker GPG fingerprint verified" {
    run grep "DOCKER_GPG_FINGERPRINT=" "$PACKER_SCRIPTS/install-docker.sh"
    assert_success
    run grep "gpg.*--with-fingerprint" "$PACKER_SCRIPTS/install-docker.sh"
    assert_success
}

@test "security: Sysbox SHA256 verification" {
    run grep "sha256sum" "$PACKER_SCRIPTS/install-sysbox.sh"
    assert_success
    run grep "SYSBOX_SHA256" "$PACKER_SCRIPTS/install-sysbox.sh"
    assert_success
}

@test "security: template has SHA256 variables for both architectures" {
    run grep "sysbox_sha256_amd64" "$PACKER_TEMPLATE"
    assert_success
    run grep "sysbox_sha256_arm64" "$PACKER_TEMPLATE"
    assert_success
}

# ── Hardening patterns ────────────────────────────────────────────────────

@test "security: ASLR enabled (randomize_va_space=2)" {
    run grep "kernel.randomize_va_space = 2" "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

@test "security: dmesg restricted" {
    run grep "kernel.dmesg_restrict = 1" "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

@test "security: kernel pointer hiding" {
    run grep "kernel.kptr_restrict = 2" "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

@test "security: ptrace scope restricted" {
    run grep "kernel.yama.ptrace_scope = 2" "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

@test "security: core dumps disabled" {
    run grep "fs.suid_dumpable = 0" "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

@test "security: Docker no-new-privileges enabled" {
    run grep '"no-new-privileges": true' "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

@test "security: Docker userland-proxy disabled" {
    run grep '"userland-proxy": false' "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

@test "security: auditd rules for Docker" {
    run grep "/usr/bin/docker" "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
    run grep "/var/lib/docker" "$PACKER_SCRIPTS/harden-vm.sh"
    assert_success
}

# ── Account security ──────────────────────────────────────────────────────

@test "security: ubuntu account locked after build" {
    run grep "passwd -l ubuntu" "$PACKER_TEMPLATE"
    assert_success
}

@test "security: root account locked after build" {
    run grep "passwd -l root" "$PACKER_TEMPLATE"
    assert_success
}

# ── Goss validation ───────────────────────────────────────────────────────

@test "security: goss tests run before cleanup" {
    # Verify goss provisioner comes before the cleanup shell provisioner
    local goss_line cleanup_line
    goss_line=$(grep -n 'provisioner "goss"' "$PACKER_TEMPLATE" | head -1 | cut -d: -f1)
    cleanup_line=$(grep -n 'cloud-init clean' "$PACKER_TEMPLATE" | head -1 | cut -d: -f1)
    [ "$goss_line" -lt "$cleanup_line" ]
}
