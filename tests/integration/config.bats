#!/usr/bin/env bats
# Configuration Integration Tests
# Verifies config files are correctly applied in running containers

setup() {
    # Set paths relative to test file location
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"
    GATEWAY_CONTAINER="polis-gateway"
    ICAP_CONTAINER="polis-icap"
    WORKSPACE_CONTAINER="polis-workspace"
    CLAMAV_CONTAINER="polis-clamav"
    
    # Set PROJECT_ROOT relative to this test file
    PROJECT_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
}

# =============================================================================
# g3proxy.yaml Configuration Tests
# =============================================================================

@test "config: g3proxy resolver uses Google DNS 8.8.8.8" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "8.8.8.8"
}

@test "config: g3proxy resolver uses Google DNS 8.8.4.4" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "8.8.4.4"
}

@test "config: g3proxy ICAP reqmod service configured" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "icap_reqmod_service: icap://icap:1344"
}

@test "config: g3proxy ICAP respmod service configured" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "icap_respmod_service:"
    assert_output --partial "url: icap://icap:1344/squidclamav"
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
# g3fcgen.yaml Configuration Tests
# =============================================================================

@test "config: g3fcgen CA certificate path correct" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3fcgen.yaml
    assert_success
    assert_output --partial "ca_certificate: /etc/g3proxy/ssl/ca.pem"
}

@test "config: g3fcgen CA private key path correct" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3fcgen.yaml
    assert_success
    assert_output --partial "ca_private_key: /etc/g3proxy/ssl/ca.key"
}

# =============================================================================
# c-icap.conf Configuration Tests
# =============================================================================

@test "config: c-icap StartServers is 3" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "StartServers 3"
}

@test "config: c-icap MaxServers is 10" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "MaxServers 10"
}

@test "config: c-icap Timeout is 300" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "Timeout 300"
}

@test "config: c-icap listens on 0.0.0.0:1344" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "Port 0.0.0.0:1344"
}

@test "config: c-icap echo service configured" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "Service echo srv_echo.so"
}

@test "config: c-icap PID file path configured" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "PidFile /var/run/c-icap/c-icap.pid"
}

# =============================================================================
# seccomp Profile Tests
# =============================================================================

@test "config: seccomp default action is ERRNO (deny by default)" {
    run cat "${PROJECT_ROOT}/config/seccomp/gateway.json"
    assert_success
    assert_output --partial '"defaultAction": "SCMP_ACT_ERRNO"'
}

@test "config: seccomp supports x86_64 architecture" {
    run cat "${PROJECT_ROOT}/config/seccomp/gateway.json"
    assert_success
    assert_output --partial "SCMP_ARCH_X86_64"
}

@test "config: seccomp supports aarch64 architecture" {
    run cat "${PROJECT_ROOT}/config/seccomp/gateway.json"
    assert_success
    assert_output --partial "SCMP_ARCH_AARCH64"
}

@test "config: seccomp allows setsockopt (required for TPROXY)" {
    run cat "${PROJECT_ROOT}/config/seccomp/gateway.json"
    assert_success
    assert_output --partial '"setsockopt"'
}

@test "config: seccomp allows socket syscall" {
    run cat "${PROJECT_ROOT}/config/seccomp/gateway.json"
    assert_success
    assert_output --partial '"socket"'
}

@test "config: seccomp allows mount syscall (for init)" {
    run cat "${PROJECT_ROOT}/config/seccomp/gateway.json"
    assert_success
    assert_output --partial '"mount"'
}

# =============================================================================
# polis-init.service Tests
# =============================================================================

@test "config: polis-init.service type is oneshot" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /etc/systemd/system/polis-init.service
    assert_success
    assert_output --partial "Type=oneshot"
}

@test "config: polis-init.service RemainAfterExit is yes" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /etc/systemd/system/polis-init.service
    assert_success
    assert_output --partial "RemainAfterExit=yes"
}

@test "config: polis-init.service runs after network-online.target" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /etc/systemd/system/polis-init.service
    assert_success
    assert_output --partial "After=network-online.target"
}

@test "config: polis-init.service ExecStart points to init script" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /etc/systemd/system/polis-init.service
    assert_success
    assert_output --partial "ExecStart=/usr/local/bin/polis-init.sh"
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
    assert_output --partial "g3proxy-init.sh"
}

@test "config: icap config mounted read-only" {
    run docker inspect "${ICAP_CONTAINER}" --format '{{json .Mounts}}'
    assert_success
    assert_output --partial "c-icap.conf"
    assert_output --partial '"RW":false'
}

@test "config: workspace init script mounted" {
    run docker inspect "${WORKSPACE_CONTAINER}" --format '{{json .Mounts}}'
    assert_success
    assert_output --partial "workspace-init.sh"
}

@test "config: openclaw.service file exists" {
    # openclaw.service is agent-specific, located in agents/openclaw/config/
    test -f "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
}

@test "config: openclaw.service is valid systemd unit" {
    run grep -q '\[Unit\]' "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
    run grep -q '\[Service\]' "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_success
}
