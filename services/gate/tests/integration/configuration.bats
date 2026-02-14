#!/usr/bin/env bats
# bats file_tags=integration,gate
# Gate Configuration Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$GATEWAY_CONTAINER"
}

# =============================================================================
# g3proxy.yaml Configuration Tests
# =============================================================================

@test "config: g3proxy resolver uses Docker embedded DNS" {
    # g3proxy uses Docker's embedded DNS (127.0.0.11) for service discovery
    # Docker DNS then forwards external queries to CoreDNS resolver
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "127.0.0.11"
}

@test "config: g3proxy ICAP reqmod service configured" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "icap_reqmod_service:"
    assert_output --partial "url: icap://sentinel:1344/credcheck"
}

@test "config: g3proxy ICAP respmod service configured" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "icap_respmod_service:"
    assert_output --partial "url: icap://sentinel:1344/squidclamav"
}

@test "config: g3proxy TLS cert agent on port 2999" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "query_peer_addr: 127.0.0.1:2999"
}

@test "config: g3proxy server listens on 0.0.0.0:18080 (TPROXY requirement)" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "listen: 0.0.0.0:18080"
}

@test "config: g3proxy audit ratio is 1.0 (all traffic)" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "task_audit_ratio: 1.0"
}

@test "config: g3proxy uses direct escaper" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "escaper: direct"
}

# =============================================================================
# seccomp Profile Tests
# =============================================================================

@test "config: seccomp default action is ERRNO (deny by default)" {
    run cat "${PROJECT_ROOT}/services/gate/config/seccomp/gateway.json"
    assert_success
    assert_output --partial '"defaultAction": "SCMP_ACT_ERRNO"'
}

@test "config: seccomp supports x86_64 architecture" {
    run cat "${PROJECT_ROOT}/services/gate/config/seccomp/gateway.json"
    assert_success
    assert_output --partial "SCMP_ARCH_X86_64"
}

@test "config: seccomp supports aarch64 architecture" {
    run cat "${PROJECT_ROOT}/services/gate/config/seccomp/gateway.json"
    assert_success
    assert_output --partial "SCMP_ARCH_AARCH64"
}

@test "config: seccomp allows setsockopt (required for TPROXY)" {
    run cat "${PROJECT_ROOT}/services/gate/config/seccomp/gateway.json"
    assert_success
    assert_output --partial '"setsockopt"'
}

@test "config: seccomp allows socket syscall" {
    run cat "${PROJECT_ROOT}/services/gate/config/seccomp/gateway.json"
    assert_success
    assert_output --partial '"socket"'
}

@test "config: seccomp allows mount syscall (for init)" {
    run cat "${PROJECT_ROOT}/services/gate/config/seccomp/gateway.json"
    assert_success
    assert_output --partial '"mount"'
}

# =============================================================================
# Docker Compose Volume Mount Tests
# =============================================================================

@test "config: gateway g3proxy.yaml mounted read-only" {
    run docker inspect "${GATEWAY_CONTAINER}" --format '{{json .Mounts}}'
    assert_success
    assert_output --partial "g3proxy.yaml"
    assert_output --partial '"RW":false'
}

@test "config: gateway g3fcgen.yaml mounted" {
    run docker inspect "${GATEWAY_CONTAINER}" --format '{{json .Mounts}}'
    assert_success
    assert_output --partial "g3fcgen.yaml"
}

@test "config: gateway init script mounted" {
    run docker inspect "${GATEWAY_CONTAINER}" --format '{{json .Mounts}}'
    assert_success
    assert_output --partial "services/gate/scripts/init.sh"
}
