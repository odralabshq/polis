#!/usr/bin/env bats
# bats file_tags=unit,config
# docker-compose.yml configuration validation

setup() {
    load "../../lib/test_helper.bash"
    COMPOSE="$PROJECT_ROOT/docker-compose.yml"
}

@test "compose config: file exists" {
    [ -f "$COMPOSE" ]
}

@test "compose config: all required networks defined" {
    run grep "internal-bridge:" "$COMPOSE"
    assert_success
    run grep "gateway-bridge:" "$COMPOSE"
    assert_success
    run grep "external-bridge:" "$COMPOSE"
    assert_success
    run grep "internet:" "$COMPOSE"
    assert_success
    run grep "host-bridge:" "$COMPOSE"
    assert_success
}

@test "compose config: all networks have IPv6 disabled" {
    local count
    count=$(grep -c "enable_ipv6: false" "$COMPOSE")
    [ "$count" -ge 5 ]
}

@test "compose config: internal-bridge is internal" {
    run grep -A5 "^  internal-bridge:" "$COMPOSE"
    assert_output --partial "internal: true"
}

@test "compose config: correct subnets" {
    run grep "10.10.1.0/24" "$COMPOSE"
    assert_success
    run grep "10.30.1.0/24" "$COMPOSE"
    assert_success
    run grep "10.20.1.0/24" "$COMPOSE"
    assert_success
}

@test "compose config: scanner-db volume named correctly" {
    run grep "name: polis-scanner-db" "$COMPOSE"
    assert_success
}

@test "compose config: state-data volume named correctly" {
    run grep "name: polis-state-data" "$COMPOSE"
    assert_success
}

@test "compose config: no profiles directives on core services" {
    # httpbin is test-only, g3-builder is build-only — exclude both
    run bash -c "sed '/httpbin:/,/^  [^ ]/d; /g3-builder:/,/^  [^ ]/d' '$COMPOSE' | grep 'profiles:'"
    assert_failure
}

@test "compose config: all services have restart policy" {
    local count
    count=$(grep -c "restart: unless-stopped" "$COMPOSE")
    [ "$count" -ge 7 ]
}

@test "compose config: all services have logging config" {
    local count
    count=$(grep -c "driver: json-file" "$COMPOSE")
    [ "$count" -ge 7 ]
}

@test "compose config: workspace uses sysbox runtime" {
    run grep "runtime: sysbox-runc" "$COMPOSE"
    assert_success
}

@test "compose config: gate has required sysctls" {
    run grep "ip_forward=1" "$COMPOSE"
    assert_success
    run grep "ip_nonlocal_bind=1" "$COMPOSE"
    assert_success
}

@test "compose config: workspace has IPv6 disabled sysctls" {
    run grep "disable_ipv6=1" "$COMPOSE"
    assert_success
}

@test "compose config: secrets section defines all 9 secrets" {
    for s in valkey_password valkey_acl valkey_dlp_password valkey_mcp_agent_password \
             valkey_mcp_admin_password valkey_log_writer_password valkey_reqmod_password \
             valkey_respmod_password valkey_cp_server_password; do
        run grep "^  ${s}:" "$COMPOSE"
        assert_success
    done
}

@test "compose config: control-plane binds loopback on 9080" {
    run grep -A30 "^  control-plane:" "$COMPOSE"
    assert_success
    assert_output --partial "127.0.0.1:9080:9080"
    assert_output --partial "host-bridge: {}"
    assert_output --partial "seccomp=./services/control-plane/config/seccomp.json"
}

@test "compose config: control-plane has docker socket integration" {
    run grep -A50 "^  control-plane:" "$COMPOSE"
    assert_success
    assert_output --partial '/var/run/docker.sock:/var/run/docker.sock:ro'
    assert_output --partial 'POLIS_CP_DOCKER_ENABLED=true'
    assert_output --partial 'POLIS_CP_AUTH_ENABLED=false'
    assert_output --partial 'POLIS_CP_ADMIN_TOKEN_FILE=/run/secrets/cp_admin_token'
    assert_output --partial 'POLIS_CP_OPERATOR_TOKEN_FILE=/run/secrets/cp_operator_token'
    assert_output --partial 'POLIS_CP_VIEWER_TOKEN_FILE=/run/secrets/cp_viewer_token'
    assert_output --partial 'POLIS_CP_AGENT_TOKEN_FILE=/run/secrets/cp_agent_token'
    assert_output --partial '${DOCKER_GID:-999}'
    assert_output --partial 'memory: 384M'
}

@test "compose config: workspace has agent metadata labels" {
    run grep -A120 "^  workspace:" "$COMPOSE"
    assert_success
    assert_output --partial 'polis.agent.name: "${POLIS_AGENT_NAME:-}"'
    assert_output --partial 'polis.agent.version: "${POLIS_AGENT_VERSION:-}"'
    assert_output --partial 'polis.agent.display_name: "${POLIS_AGENT_DISPLAY_NAME:-}"'
}

@test "compose config: workspace DNS points to resolver" {
    run grep "10.10.1.2" "$COMPOSE"
    assert_success
}

# ── DHI Supply Chain (Issue 14) ──────────────────────────────────────────

@test "compose config: scanner-init uses polis-init image" {
    run grep -A5 "^  scanner-init:" "$COMPOSE"
    assert_output --partial "polis-init-oss"
}

@test "compose config: state-init uses polis-init image" {
    run grep -A5 "^  state-init:" "$COMPOSE"
    assert_output --partial "polis-init-oss"
}

@test "compose config: state uses DHI valkey" {
    run grep -A2 "^  state:" "$COMPOSE"
    assert_output --partial "dhi.io/valkey"
}
