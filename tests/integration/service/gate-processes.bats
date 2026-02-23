#!/usr/bin/env bats
# bats file_tags=integration,service
# Integration tests for gate service — g3proxy processes, ports, CA certs
# Note: g3fcgen now runs in separate certgen container (Issue #17 - CA Key Isolation)

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

# g3fcgen now runs in certgen container, not gate
@test "gate: g3fcgen NOT running in gate (isolated in certgen)" {
    run docker exec "$CTR_GATE" pgrep -x g3fcgen
    assert_failure
}

# ── Binaries ──────────────────────────────────────────────────────────────

@test "gate: g3proxy binary at /usr/bin/g3proxy" {
    run docker exec "$CTR_GATE" which g3proxy
    assert_success
    assert_output --partial "/usr/bin/g3proxy"
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

# g3fcgen UDP 2999 now in certgen container
@test "gate: g3fcgen NOT listening locally (uses certgen:2999)" {
    run docker exec "$CTR_GATE" ss -uln
    assert_success
    refute_output --partial ":2999"
}

# ── CA certificate (Issue #17: CA key isolated in certgen) ────────────────

@test "gate: CA certificate exists and valid" {
    run docker exec "$CTR_GATE" openssl x509 -noout -in /etc/g3proxy/ssl/ca.pem
    assert_success
}

@test "gate: CA private key is NOT present (isolated in certgen)" {
    run docker exec "$CTR_GATE" test -f /etc/g3proxy/ssl/ca.key
    assert_failure
}

@test "gate: CA cert not expired" {
    run docker exec "$CTR_GATE" openssl x509 -checkend 0 -noout -in /etc/g3proxy/ssl/ca.pem
    assert_success
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
