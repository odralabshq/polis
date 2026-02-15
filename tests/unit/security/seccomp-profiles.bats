#!/usr/bin/env bats
# bats file_tags=unit,security
# Seccomp profile static validation

setup() {
    load "../../lib/test_helper.bash"
    GATE_SECCOMP="$PROJECT_ROOT/services/gate/config/seccomp/gateway.json"
    WORKSPACE_SECCOMP="$PROJECT_ROOT/services/workspace/config/seccomp.json"
}

@test "seccomp: gateway profile exists" {
    [ -f "$GATE_SECCOMP" ]
}

@test "seccomp: workspace profile exists" {
    [ -f "$WORKSPACE_SECCOMP" ]
}

@test "seccomp: gateway default action is ERRNO" {
    run grep "SCMP_ACT_ERRNO" "$GATE_SECCOMP"
    assert_success
}

@test "seccomp: supports x86_64" {
    run grep "SCMP_ARCH_X86_64" "$GATE_SECCOMP"
    assert_success
}

@test "seccomp: supports aarch64" {
    run grep "SCMP_ARCH_AARCH64" "$GATE_SECCOMP"
    assert_success
}

@test "seccomp: gateway allows setsockopt for TPROXY" {
    run grep "setsockopt" "$GATE_SECCOMP"
    assert_success
}
