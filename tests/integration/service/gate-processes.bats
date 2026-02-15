#!/usr/bin/env bats
# bats file_tags=integration,service
# Integration tests for gate service — g3proxy/g3fcgen processes, ports, CA certs

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    load "../../lib/assertions/process.bash"
    require_container "$CTR_GATE"
}

# ── Processes ─────────────────────────────────────────────────────────────

@test "gate: g3proxy process running" {
    run docker exec "$CTR_GATE" pgrep -x g3proxy
    assert_success
}

@test "gate: g3fcgen process running" {
    run docker exec "$CTR_GATE" pgrep -x g3fcgen
    assert_success
}

# ── Binaries ──────────────────────────────────────────────────────────────

@test "gate: g3proxy binary at /usr/bin/g3proxy" {
    run docker exec "$CTR_GATE" which g3proxy
    assert_success
    assert_output --partial "/usr/bin/g3proxy"
}

@test "gate: g3fcgen binary at /usr/bin/g3fcgen" {
    run docker exec "$CTR_GATE" which g3fcgen
    assert_success
    assert_output --partial "/usr/bin/g3fcgen"
}

# Source: services/gate/Dockerfile — compiled from g3proxy v1.12.2
@test "gate: g3proxy version is 1.12.x" {
    run docker exec "$CTR_GATE" g3proxy --version
    assert_success
    assert_output --partial "1.12"
}

# ── Ports (source: services/gate/config/g3proxy.yaml) ────────────────────

@test "gate: g3proxy listening on TCP 18080" {
    run docker exec "$CTR_GATE" ss -tln
    assert_success
    assert_output --partial ":18080"
}

@test "gate: g3fcgen listening on UDP 2999" {
    run docker exec "$CTR_GATE" ss -uln
    assert_success
    assert_output --partial ":2999"
}

# ── CA certificate (source: docker-compose.yml ./certs/ca:/etc/g3proxy/ssl:ro) ──

@test "gate: CA certificate exists and valid" {
    run docker exec "$CTR_GATE" openssl x509 -noout -in /etc/g3proxy/ssl/ca.pem
    assert_success
}

@test "gate: CA key exists" {
    run docker exec "$CTR_GATE" test -f /etc/g3proxy/ssl/ca.key
    assert_success
}

@test "gate: CA cert not expired" {
    run docker exec "$CTR_GATE" openssl x509 -checkend 0 -noout -in /etc/g3proxy/ssl/ca.pem
    assert_success
}

@test "gate: CA key matches cert" {
    local cert_mod key_mod
    cert_mod=$(docker exec "$CTR_GATE" openssl x509 -noout -modulus -in /etc/g3proxy/ssl/ca.pem 2>/dev/null | md5sum)
    key_mod=$(docker exec "$CTR_GATE" openssl rsa -noout -modulus -in /etc/g3proxy/ssl/ca.key 2>/dev/null | md5sum)
    [[ "$cert_mod" == "$key_mod" ]] || fail "CA key does not match cert"
}

# ── Health check ──────────────────────────────────────────────────────────

@test "gate: health check passes" {
    run docker exec "$CTR_GATE" /scripts/health-check.sh
    assert_success
}

# ── CA cert properties ────────────────────────────────────────────────────

@test "gate: CA cert uses SHA-256+ signature" {
    run docker exec "$CTR_GATE" openssl x509 -text -noout -in /etc/g3proxy/ssl/ca.pem
    assert_success
    assert_output --regexp "(sha256|sha384|sha512|ecdsa)"
}

@test "gate: CA cert has CA:TRUE basic constraint" {
    run docker exec "$CTR_GATE" openssl x509 -text -noout -in /etc/g3proxy/ssl/ca.pem
    assert_success
    assert_output --partial "CA:TRUE"
}

@test "gate: CA cert chain is valid" {
    run docker exec "$CTR_GATE" openssl verify -CAfile /etc/g3proxy/ssl/ca.pem /etc/g3proxy/ssl/ca.pem
    assert_success
    assert_output --partial "OK"
}
