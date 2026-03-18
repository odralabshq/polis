#!/usr/bin/env bats
# bats file_tags=integration,service,control-plane
# Integration tests for control-plane service — health endpoint, env vars, and mounted secrets

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_CONTROL_PLANE"
}

wait_for_control_plane() {
    local path="$1"
    local url="http://127.0.0.1:${PORT_CONTROL_PLANE}${path}"
    local attempt

    for attempt in $(seq 1 20); do
        if curl -sf "$url"; then
            return 0
        fi
        sleep 1
    done

    return 1
}

@test "control-plane: health endpoint responds on loopback" {
    wait_for_control_plane "/health" >/dev/null
}

@test "control-plane: root page serves dashboard HTML" {
    output="$(wait_for_control_plane "/")"
    [[ "$output" == *"Polis Control Plane"* ]]
}

@test "control-plane: LISTEN_ADDR env set" {
    run docker exec "$CTR_CONTROL_PLANE" printenv POLIS_CP_LISTEN_ADDR
    assert_success
    assert_output "0.0.0.0:9080"
}

@test "control-plane: VALKEY_URL env set" {
    run docker exec "$CTR_CONTROL_PLANE" printenv POLIS_CP_VALKEY_URL
    assert_success
    assert_output "rediss://valkey:6379"
}

@test "control-plane: VALKEY_USER env set" {
    run docker exec "$CTR_CONTROL_PLANE" printenv POLIS_CP_VALKEY_USER
    assert_success
    assert_output "cp-server"
}

@test "control-plane: valkey TLS certs mounted" {
    run docker exec "$CTR_CONTROL_PLANE" test -d /etc/valkey/tls
    assert_success
}

@test "control-plane: password secret mounted" {
    run docker exec "$CTR_CONTROL_PLANE" test -f /run/secrets/valkey_cp_server_password
    assert_success
}
