#!/usr/bin/env bats
# bats file_tags=integration,service,state
# Integration tests for state service — Valkey process, TLS port, config, secrets

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

# ── Port (source: services/state/config/valkey.conf tls-port 6379, port 0) ──

@test "state: valkey listening on TLS 6379" {
    run docker exec "$CTR_STATE" cat /proc/net/tcp
    assert_success
    # 6379 decimal = 18EB hex
    assert_output --partial "18EB"
}

# ── Config (source: docker-compose.yml mounts) ───────────────────────────

@test "state: config mounted at /etc/valkey/valkey.conf" {
    run docker exec "$CTR_STATE" test -f /etc/valkey/valkey.conf
    assert_success
}

@test "state: TLS certificates mounted" {
    run docker exec "$CTR_STATE" test -d /etc/valkey/tls
    assert_success
}

# ── Data directory (source: valkey.conf dir /data) ────────────────────────

@test "state: /data directory exists and writable" {
    run docker exec "$CTR_STATE" sh -c 'test -d /data && test -w /data'
    assert_success
}

# ── Secrets (source: docker-compose.yml secrets) ─────────────────────────

@test "state: password secret mounted" {
    run docker exec "$CTR_STATE" test -f /run/secrets/valkey_password
    assert_success
}

@test "state: ACL file mounted" {
    run docker exec "$CTR_STATE" test -f /run/secrets/valkey_acl
    assert_success
}
