#!/usr/bin/env bats
# bats file_tags=integration,service
# Integration tests for sentinel service — c-icap processes, ports, modules

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_SENTINEL"
}

# ── Processes ─────────────────────────────────────────────────────────────

@test "sentinel: c-icap process running" {
    # Use docker top instead of pgrep (minimal image has no procps)
    run docker top "$CTR_SENTINEL" -o pid,comm
    assert_success
    assert_output --partial "c-icap"
}

# Source: c-icap.conf StartServers 3 → master + workers
@test "sentinel: multiple c-icap worker processes" {
    # Use docker top instead of pgrep -c (minimal image has no procps)
    run bash -c "docker top $CTR_SENTINEL | grep -c c-icap"
    assert_success
    [[ "$output" -ge 2 ]] || fail "Expected ≥2 c-icap processes, got $output"
}

# ── Port (source: services/sentinel/config/c-icap.conf Port 0.0.0.0:1344) ──

@test "sentinel: listening on TCP 1344" {
    run docker exec "$CTR_SENTINEL" cat /proc/net/tcp
    assert_success
    # 1344 decimal = 0540 hex
    assert_output --partial "0540"
}

@test "sentinel: port bound to all interfaces" {
    run docker exec "$CTR_SENTINEL" cat /proc/net/tcp
    assert_success
    assert_output --partial "00000000:0540"
}

# ── PID file (source: c-icap.conf PidFile /var/run/c-icap/c-icap.pid) ────

@test "sentinel: PID file exists and valid" {
    local pid
    pid=$(docker exec "$CTR_SENTINEL" cat /var/run/c-icap/c-icap.pid 2>/dev/null)
    [[ -n "$pid" ]] || fail "PID file empty"
    # Use /proc instead of ps (minimal image has no procps)
    run docker exec "$CTR_SENTINEL" test -d "/proc/$pid"
    assert_success
}

# ── Entrypoint ────────────────────────────────────────────────────────────

@test "sentinel: entrypoint script exists and executable" {
    run docker exec "$CTR_SENTINEL" test -x /entrypoint.sh
    if [[ "$status" -ne 0 ]]; then
        # Some images use c-icap directly as entrypoint
        run docker exec "$CTR_SENTINEL" test -x /usr/bin/c-icap
        assert_success
    fi
}

# ── Modules (source: c-icap.conf Service directives) ─────────────────────

@test "sentinel: echo service module exists" {
    run docker exec "$CTR_SENTINEL" test -f /usr/local/lib/c_icap/srv_echo.so
    assert_success
}

@test "sentinel: squidclamav module exists" {
    run docker exec "$CTR_SENTINEL" test -f /usr/local/lib/c_icap/squidclamav.so
    assert_success
}

@test "sentinel: DLP module exists" {
    run docker exec "$CTR_SENTINEL" test -f /usr/local/lib/c_icap/srv_polis_dlp.so
    assert_success
}

@test "sentinel: approval modules exist" {
    run docker exec "$CTR_SENTINEL" test -f /usr/local/lib/c_icap/srv_polis_approval.so
    assert_success
}

# ── Logging (source: c-icap.conf ServerLog /var/log/c-icap/server.log) ───

@test "sentinel: server log exists" {
    run docker exec "$CTR_SENTINEL" test -f /var/log/c-icap/server.log
    assert_success
}

@test "sentinel: server log writable" {
    run docker exec "$CTR_SENTINEL" test -w /var/log/c-icap/server.log
    assert_success
}
