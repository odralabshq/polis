#!/usr/bin/env bats
# bats file_tags=integration,state,security
# Integration tests for Valkey TLS enforcement
# Source: services/state/config/valkey.conf (tls-port 6379, port 0, tls-auth-clients yes)

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_STATE"
}

@test "valkey-tls: non-TLS connection rejected" {
    # valkey.conf: port 0 (non-TLS disabled), tls-port 6379
    run docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_password) \
        valkey-cli --user healthcheck --no-auth-warning PING 2>&1"
    # Should fail — no TLS means connection refused or error
    refute_output "PONG"
}

@test "valkey-tls: TLS connection with valid cert succeeds" {
    run docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user healthcheck --no-auth-warning PING"
    assert_success
    assert_output "PONG"
}

@test "valkey-tls: TLS connection with wrong CA rejected" {
    # Use a non-existent CA file — should fail TLS handshake
    run docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/server.crt \
            --user healthcheck --no-auth-warning PING 2>&1"
    refute_output "PONG"
}
