#!/usr/bin/env bats
# bats file_tags=unit,scripts
# State generate-certs.sh validation (runs in temp dir)

setup() {
    load "../../lib/test_helper.bash"
    SCRIPT="$PROJECT_ROOT/services/state/scripts/generate-certs.sh"
    TEST_DIR="$(mktemp -d)"
}

teardown() {
    rm -rf "$TEST_DIR"
}

@test "generate-certs: generates certificate files" {
    run bash "$SCRIPT" "$TEST_DIR"
    assert_success
    [ -f "$TEST_DIR/ca.crt" ] || [ -f "$TEST_DIR/ca.pem" ]
}

@test "generate-certs: key files have restrictive permissions" {
    bash "$SCRIPT" "$TEST_DIR"
    for f in "$TEST_DIR"/*.key; do
        [ -f "$f" ] || continue
        local perms
        perms=$(stat -c '%a' "$f")
        [ "$perms" = "600" ] || fail "$(basename "$f") has perms $perms, expected 600"
    done
}

@test "generate-certs: generated certs are valid X.509" {
    bash "$SCRIPT" "$TEST_DIR"
    for f in "$TEST_DIR"/*.crt; do
        [ -f "$f" ] || continue
        run openssl x509 -noout -in "$f"
        assert_success
    done
}

@test "generate-certs: CA key is at least 2048 bits" {
    bash "$SCRIPT" "$TEST_DIR"
    local ca_key=""
    for f in "$TEST_DIR"/ca*.key "$TEST_DIR"/ca.key; do
        [ -f "$f" ] && ca_key="$f" && break
    done
    [ -n "$ca_key" ] || fail "No CA key found"
    local bits
    bits=$(openssl rsa -in "$ca_key" -text -noout 2>/dev/null | grep "Private-Key:" | sed 's/.*(\([0-9]*\) bit.*/\1/')
    [ "$bits" -ge 2048 ] || fail "CA key is $bits bits, expected >= 2048"
}
