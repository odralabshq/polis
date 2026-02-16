#!/usr/bin/env bats
# bats file_tags=integration,service
# Integration tests for certgen service — g3fcgen certificate generator sidecar
# Issue #17: CA Key Isolation - certgen holds CA key, gate only has ca.pem

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    load "../../lib/assertions/process.bash"
    require_container "$CTR_CERTGEN"
}

# ── Processes ─────────────────────────────────────────────────────────────

@test "certgen: g3fcgen process running" {
    run docker exec "$CTR_CERTGEN" pgrep -x g3fcgen
    assert_success
}

# ── Ports ─────────────────────────────────────────────────────────────────

@test "certgen: g3fcgen listening on UDP 2999" {
    run docker exec "$CTR_CERTGEN" ss -uln
    assert_success
    assert_output --partial ":2999"
}

# ── CA certificate and key (certgen holds both) ───────────────────────────

@test "certgen: CA certificate exists" {
    run docker exec "$CTR_CERTGEN" test -f /etc/g3fcgen/ca.pem
    assert_success
}

@test "certgen: CA private key exists" {
    run docker exec "$CTR_CERTGEN" test -f /etc/g3fcgen/ca.key
    assert_success
}

@test "certgen: CA key matches cert" {
    local cert_mod key_mod
    cert_mod=$(docker exec "$CTR_CERTGEN" openssl x509 -noout -modulus -in /etc/g3fcgen/ca.pem 2>/dev/null | md5sum)
    key_mod=$(docker exec "$CTR_CERTGEN" openssl rsa -noout -modulus -in /etc/g3fcgen/ca.key 2>/dev/null | md5sum)
    [[ "$cert_mod" == "$key_mod" ]] || fail "CA key does not match cert"
}

# ── Security constraints ──────────────────────────────────────────────────

@test "certgen: runs as nonroot user (65532)" {
    run docker exec "$CTR_CERTGEN" id -u
    assert_success
    assert_output "65532"
}

@test "certgen: filesystem is read-only" {
    run docker exec "$CTR_CERTGEN" touch /test-write 2>&1
    assert_failure
    assert_output --partial "Read-only file system"
}
