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
    # Use docker top (minimal image has no procps)
    run docker top "$CTR_CERTGEN" -o pid,comm
    assert_success
    assert_output --partial "g3fcgen"
}

# ── Ports ─────────────────────────────────────────────────────────────────

@test "certgen: g3fcgen listening on UDP 2999" {
    # Check from host via docker port or netstat
    run docker exec "$CTR_CERTGEN" cat /proc/net/udp
    assert_success
    # 2999 = 0x0BB7, appears in local_address column
    assert_output --partial "0BB7"
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
    # Run openssl from host against mounted certs
    local cert_mod key_mod
    cert_mod=$(openssl x509 -noout -modulus -in "$PROJECT_ROOT/certs/ca/ca.pem" 2>/dev/null | md5sum)
    key_mod=$(openssl rsa -noout -modulus -in "$PROJECT_ROOT/certs/ca/ca.key" 2>/dev/null | md5sum)
    [[ "$cert_mod" == "$key_mod" ]] || fail "CA key does not match cert"
}

# ── Security constraints ──────────────────────────────────────────────────

@test "certgen: runs as nonroot user (65532)" {
    run bash -c "docker top $CTR_CERTGEN | grep g3fcgen | awk '{print \$1}'"
    assert_success
    assert_output "65532"
}

@test "certgen: filesystem is read-only" {
    run docker exec "$CTR_CERTGEN" touch /test-write 2>&1
    assert_failure
    assert_output --partial "Read-only file system"
}
