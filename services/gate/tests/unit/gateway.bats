#!/usr/bin/env bats
# bats file_tags=integration,gate
# Gateway Container Unit Tests
# Tests for polis-gateway container

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$GATEWAY_CONTAINER"
}

# =============================================================================
# Container State Tests
# =============================================================================

@test "gateway: container exists" {
    run docker ps -a --filter "name=^${GATEWAY_CONTAINER}$" --format '{{.Names}}'
    assert_success
    assert_output "${GATEWAY_CONTAINER}"
}

@test "gateway: container is running" {
    run docker ps --filter "name=${GATEWAY_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

@test "gateway: container is healthy" {
    run docker inspect --format '{{.State.Health.Status}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Binary Tests
# =============================================================================

@test "gateway: g3proxy binary exists" {
    run docker exec "${GATEWAY_CONTAINER}" which g3proxy
    assert_success
    assert_output "/usr/bin/g3proxy"
}

@test "gateway: g3proxy binary is executable" {
    run docker exec "${GATEWAY_CONTAINER}" test -x /usr/bin/g3proxy
    assert_success
}

@test "gateway: g3proxy version is accessible" {
    run docker exec "${GATEWAY_CONTAINER}" g3proxy --version
    assert_success
    assert_output --partial "g3proxy"
}

@test "gateway: g3fcgen binary exists" {
    run docker exec "${GATEWAY_CONTAINER}" which g3fcgen
    assert_success
    assert_output "/usr/bin/g3fcgen"
}

@test "gateway: g3fcgen binary is executable" {
    run docker exec "${GATEWAY_CONTAINER}" test -x /usr/bin/g3fcgen
    assert_success
}

@test "gateway: g3fcgen version is accessible" {
    run docker exec "${GATEWAY_CONTAINER}" g3fcgen --version
    assert_success
    assert_output --partial "g3fcgen"
}

# =============================================================================
# Process Tests
# =============================================================================

@test "gateway: g3proxy process is running" {
    run docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy
    assert_success
}

@test "gateway: g3fcgen process is running" {
    run docker exec "${GATEWAY_CONTAINER}" pgrep -x g3fcgen
    assert_success
}

@test "gateway: init script completed successfully" {
    # Check that nftables rules are configured (indicates init completed)
    run docker exec "${GATEWAY_CONTAINER}" nft list table inet polis
    assert_success
}

# =============================================================================
# Configuration Tests
# =============================================================================

@test "gateway: g3proxy config file exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /etc/g3proxy/g3proxy.yaml
    assert_success
}

@test "gateway: g3proxy config is readable" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "server:"
}

@test "gateway: g3fcgen config file exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /etc/g3proxy/g3fcgen.yaml
    assert_success
}

@test "gateway: g3fcgen config is readable" {
    run docker exec "${GATEWAY_CONTAINER}" cat /etc/g3proxy/g3fcgen.yaml
    assert_success
}

# =============================================================================
# Certificate Tests
# =============================================================================

@test "gateway: CA certificate exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /etc/g3proxy/ssl/ca.pem
    assert_success
}

@test "gateway: CA private key exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /etc/g3proxy/ssl/ca.key
    assert_success
}

@test "gateway: CA certificate is valid" {
    run docker exec "${GATEWAY_CONTAINER}" openssl x509 -in /etc/g3proxy/ssl/ca.pem -noout -text
    assert_success
    assert_output --partial "Issuer:"
}

@test "gateway: CA certificate is not expired" {
    run docker exec "${GATEWAY_CONTAINER}" openssl x509 -checkend 86400 -noout -in /etc/g3proxy/ssl/ca.pem
    assert_success
}

@test "gateway: CA key matches certificate" {
    # Get modulus hashes
    local cert_hash key_hash
    cert_hash=$(docker exec "${GATEWAY_CONTAINER}" openssl x509 -noout -modulus -in /etc/g3proxy/ssl/ca.pem | openssl sha256)
    key_hash=$(docker exec "${GATEWAY_CONTAINER}" openssl rsa -noout -modulus -in /etc/g3proxy/ssl/ca.key | openssl sha256)
    
    assert [ "$cert_hash" = "$key_hash" ]
}

# =============================================================================
# Port Tests
# =============================================================================

@test "gateway: g3proxy listening on TPROXY port 18080" {
    run docker exec "${GATEWAY_CONTAINER}" ss -tlnp
    assert_success
    assert_output --partial ":18080"
}

@test "gateway: g3fcgen listening on UDP 2999" {
    run docker exec "${GATEWAY_CONTAINER}" ss -ulnp
    assert_success
    assert_output --partial ":2999"
}

# =============================================================================
# Network Tools Tests
# =============================================================================

@test "gateway: nft is available" {
    run docker exec "${GATEWAY_CONTAINER}" which nft
    assert_success
}

@test "gateway: ip command is available" {
    run docker exec "${GATEWAY_CONTAINER}" which ip
    assert_success
}

@test "gateway: curl is available" {
    run docker exec "${GATEWAY_CONTAINER}" which curl
    assert_success
}

# =============================================================================
# Directory Tests
# =============================================================================

@test "gateway: /etc/g3proxy directory exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -d /etc/g3proxy
    assert_success
}

@test "gateway: /var/log/g3proxy directory exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -d /var/log/g3proxy
    assert_success
}

@test "gateway: /tmp/g3 control directory exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -d /tmp/g3
    assert_success
}

# =============================================================================
# Health Check Script Tests
# =============================================================================

@test "gateway: health check script exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /scripts/health-check.sh
    assert_success
}

@test "gateway: health check script is executable" {
    run docker exec "${GATEWAY_CONTAINER}" test -x /scripts/health-check.sh
    assert_success
}

@test "gateway: health check passes" {
    run docker exec "${GATEWAY_CONTAINER}" /scripts/health-check.sh
    assert_success
    assert_output "OK"
}
