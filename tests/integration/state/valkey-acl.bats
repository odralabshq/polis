#!/usr/bin/env bats
# bats file_tags=integration,state,security
# Integration tests for Valkey ACL enforcement — verifies each user role is properly restricted
# Source: /run/secrets/valkey_acl (mounted from secrets/valkey_users.acl)

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

# Helper: run valkey-cli as a specific user
valkey_cmd() {
    local user="$1" password_file="$2" cmd="$3"
    docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/$password_file) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user $user --no-auth-warning $cmd"
}

# ── Dangerous commands blocked ────────────────────────────────────────────

@test "valkey-acl: FLUSHALL blocked for mcp-admin" {
    # mcp-admin has -FLUSHALL in ACL
    run valkey_cmd mcp-admin valkey_mcp_admin_password "FLUSHALL"
    assert_output --partial "NOPERM"
}

@test "valkey-acl: CONFIG blocked for mcp-admin" {
    # mcp-admin has -CONFIG in ACL
    run valkey_cmd mcp-admin valkey_mcp_admin_password "CONFIG GET maxmemory"
    assert_output --partial "NOPERM"
}

# ── mcp-agent restrictions ────────────────────────────────────────────────

@test "valkey-acl: mcp-agent denied unauthorized keys" {
    # mcp-agent only has ~polis:blocked:* ~polis:approved:* (+ selectors)
    run valkey_cmd mcp-agent valkey_mcp_agent_password "GET unauthorized:key"
    assert_output --partial "NOPERM"
}

@test "valkey-acl: mcp-agent denied DEL command" {
    # mcp-agent does not have +DEL
    run valkey_cmd mcp-agent valkey_mcp_agent_password "DEL polis:blocked:test"
    assert_output --partial "NOPERM"
}

@test "valkey-acl: mcp-agent cannot SET security_level" {
    # Selector: (~polis:config:security_level -@all +GET +PING) — no SET
    run valkey_cmd mcp-agent valkey_mcp_agent_password "SET polis:config:security_level test"
    assert_output --partial "NOPERM"
}

@test "valkey-acl: mcp-admin denied FLUSHALL" {
    run valkey_cmd mcp-admin valkey_mcp_admin_password "FLUSHALL"
    assert_output --partial "NOPERM"
}

# ── log-writer restrictions ───────────────────────────────────────────────

@test "valkey-acl: log-writer denied SET command" {
    # log-writer only has ZADD, ZRANGEBYSCORE, ZCARD, PING
    run valkey_cmd log-writer valkey_log_writer_password "SET polis:log:events test"
    assert_output --partial "NOPERM"
}

@test "valkey-acl: log-writer denied non-allowed keys" {
    # log-writer only has ~polis:log:events
    run valkey_cmd log-writer valkey_log_writer_password "ZADD polis:other:key 1 test"
    assert_output --partial "NOPERM"
}

# ── healthcheck restrictions ──────────────────────────────────────────────

@test "valkey-acl: healthcheck denied SET command" {
    # healthcheck only has PING, INFO
    run valkey_cmd healthcheck valkey_password "SET test value"
    assert_output --partial "NOPERM"
}

@test "valkey-acl: healthcheck denied key access" {
    # healthcheck has no key patterns
    run valkey_cmd healthcheck valkey_password "GET polis:config:security_level"
    assert_output --partial "NOPERM"
}

# ── dlp-reader restrictions ──────────────────────────────────────────────

@test "valkey-acl: dlp-reader can GET security_level" {
    # dlp-reader has ~polis:config:security_level +GET +PING
    run valkey_cmd dlp-reader valkey_dlp_password "GET polis:config:security_level"
    # Key may not exist, but command should be allowed (returns nil, not NOPERM)
    refute_output --partial "NOPERM"
}

@test "valkey-acl: dlp-reader cannot SET security_level" {
    run valkey_cmd dlp-reader valkey_dlp_password "SET polis:config:security_level test"
    assert_output --partial "NOPERM"
}
