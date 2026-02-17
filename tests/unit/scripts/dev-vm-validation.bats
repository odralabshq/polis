#!/usr/bin/env bats
# bats file_tags=unit,scripts
# Unit tests for tools/dev-vm.sh input validation

setup() {
    load "../../lib/test_helper.bash"
}

# ============================================================================
# VM Name Validation
# ============================================================================

@test "dev-vm: rejects VM name with semicolon" {
    run env POLIS_VM_NAME='test;rm' bash -c 'source "$PROJECT_ROOT/tools/dev-vm.sh" 2>&1 || true'
    assert_output --partial "Invalid VM name"
}

@test "dev-vm: rejects VM name with dollar sign" {
    run env POLIS_VM_NAME='test$var' bash -c 'source "$PROJECT_ROOT/tools/dev-vm.sh" 2>&1 || true'
    assert_output --partial "Invalid VM name"
}

@test "dev-vm: rejects VM name with backtick" {
    run env POLIS_VM_NAME='test`cmd`' bash -c 'source "$PROJECT_ROOT/tools/dev-vm.sh" 2>&1 || true'
    assert_output --partial "Invalid VM name"
}

@test "dev-vm: rejects VM name with pipe" {
    run env POLIS_VM_NAME='test|cat' bash -c 'source "$PROJECT_ROOT/tools/dev-vm.sh" 2>&1 || true'
    assert_output --partial "Invalid VM name"
}

@test "dev-vm: rejects VM name over 63 chars" {
    local long_name
    long_name=$(printf 'a%.0s' {1..64})
    run env POLIS_VM_NAME="$long_name" bash -c 'source "$PROJECT_ROOT/tools/dev-vm.sh" 2>&1 || true'
    assert_output --partial "Invalid VM name"
}

@test "dev-vm: accepts valid VM name with hyphens" {
    # This will fail on multipass check, but should pass validation
    run env POLIS_VM_NAME='my-polis-dev' "$PROJECT_ROOT/tools/dev-vm.sh" status 2>&1
    refute_output --partial "Invalid VM name"
}

@test "dev-vm: accepts valid VM name with underscores" {
    run env POLIS_VM_NAME='my_polis_dev' "$PROJECT_ROOT/tools/dev-vm.sh" status 2>&1
    refute_output --partial "Invalid VM name"
}

# ============================================================================
# CPU Validation
# ============================================================================

@test "dev-vm: rejects non-numeric CPU count" {
    run env POLIS_VM_CPUS='abc' "$PROJECT_ROOT/tools/dev-vm.sh" --help 2>&1
    assert_output --partial "Invalid CPU count"
}

@test "dev-vm: rejects CPU count with suffix" {
    run env POLIS_VM_CPUS='4G' "$PROJECT_ROOT/tools/dev-vm.sh" --help 2>&1
    assert_output --partial "Invalid CPU count"
}

# ============================================================================
# Memory Validation
# ============================================================================

@test "dev-vm: rejects memory without unit suffix" {
    run env POLIS_VM_MEMORY='8' "$PROJECT_ROOT/tools/dev-vm.sh" --help 2>&1
    assert_output --partial "Invalid memory"
}

@test "dev-vm: rejects memory with invalid unit" {
    run env POLIS_VM_MEMORY='8T' "$PROJECT_ROOT/tools/dev-vm.sh" --help 2>&1
    assert_output --partial "Invalid memory"
}

@test "dev-vm: accepts memory with G suffix" {
    run env POLIS_VM_MEMORY='16G' "$PROJECT_ROOT/tools/dev-vm.sh" status 2>&1
    refute_output --partial "Invalid memory"
}

# ============================================================================
# Disk Validation
# ============================================================================

@test "dev-vm: rejects disk without unit suffix" {
    run env POLIS_VM_DISK='50' "$PROJECT_ROOT/tools/dev-vm.sh" --help 2>&1
    assert_output --partial "Invalid disk"
}

@test "dev-vm: accepts disk with G suffix" {
    run env POLIS_VM_DISK='100G' "$PROJECT_ROOT/tools/dev-vm.sh" status 2>&1
    refute_output --partial "Invalid disk"
}

# ============================================================================
# Help Command
# ============================================================================

@test "dev-vm: --help works without multipass" {
    run "$PROJECT_ROOT/tools/dev-vm.sh" --help
    assert_success
    assert_output --partial "Polis Development VM"
    assert_output --partial "Commands:"
}
