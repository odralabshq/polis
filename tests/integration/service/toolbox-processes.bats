#!/usr/bin/env bats
# bats file_tags=integration,service,toolbox
# Integration tests for toolbox service — health endpoint, env vars, TLS certs

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_TOOLBOX"
}

# ── Health (source: docker-compose.yml healthcheck curl localhost:8080/health) ──

@test "toolbox: health endpoint responds" {
    # Check health from host (minimal image has no curl/wget)
    local ip
    ip=$(docker inspect -f '{{(index .NetworkSettings.Networks "polis_internal-bridge").IPAddress}}' "$CTR_TOOLBOX")
    run curl -sf "http://${ip}:8080/health"
    assert_success
}

# ── Environment (source: docker-compose.yml environment) ──────────────────

@test "toolbox: LISTEN_ADDR env set" {
    run docker exec "$CTR_TOOLBOX" printenv polis_AGENT_LISTEN_ADDR
    assert_success
    assert_output --partial "0.0.0.0:8080"
}

@test "toolbox: VALKEY_URL env set" {
    run docker exec "$CTR_TOOLBOX" printenv polis_AGENT_VALKEY_URL
    assert_success
    assert_output --partial "rediss://state:6379"
}

@test "toolbox: VALKEY_USER env set" {
    run docker exec "$CTR_TOOLBOX" printenv polis_AGENT_VALKEY_USER
    assert_success
    assert_output "mcp-agent"
}

# ── TLS certs (source: docker-compose.yml ./certs/valkey:/etc/valkey/tls:ro) ──

@test "toolbox: valkey TLS certs mounted" {
    run docker exec "$CTR_TOOLBOX" test -d /etc/valkey/tls
    assert_success
}
