#!/usr/bin/env bats
# bats file_tags=integration,service
# Integration tests for workspace service — runtime, systemd, CA cert, init, networking

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    export WORKSPACE_INSPECT="$(docker inspect "$CTR_WORKSPACE" 2>/dev/null || echo '[]')"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_WORKSPACE"
}

# ── Runtime (source: docker-compose.yml runtime: sysbox-runc) ─────────────

@test "workspace: uses sysbox runtime" {
    run jq -r '.[0].HostConfig.Runtime' <<< "$WORKSPACE_INSPECT"
    assert_output "sysbox-runc"
}

# ── Systemd ───────────────────────────────────────────────────────────────

@test "workspace: systemd is PID 1" {
    run docker exec "$CTR_WORKSPACE" ps -p 1 -o comm=
    assert_success
    assert_output --partial "systemd"
}

@test "workspace: polis-init service exists" {
    run docker exec "$CTR_WORKSPACE" systemctl cat polis-init
    assert_success
}

# ── CA certificate (source: docker-compose.yml ca.pem:/usr/local/share/ca-certificates/polis-ca.crt:ro) ──

@test "workspace: CA certificate mounted" {
    run docker exec "$CTR_WORKSPACE" test -f /usr/local/share/ca-certificates/polis-ca.crt
    assert_success
}

@test "workspace: CA certificate valid" {
    run docker exec "$CTR_WORKSPACE" openssl x509 -noout -in /usr/local/share/ca-certificates/polis-ca.crt
    assert_success
}

# ── Init script (source: docker-compose.yml init.sh:/usr/local/bin/polis-init.sh:ro) ──

@test "workspace: init script exists and executable" {
    run docker exec "$CTR_WORKSPACE" test -x /usr/local/bin/polis-init.sh
    assert_success
}

# ── Networking ────────────────────────────────────────────────────────────

@test "workspace: has default route" {
    run docker exec "$CTR_WORKSPACE" ip route
    assert_success
    assert_output --partial "default via"
}

@test "workspace: can resolve gateway hostname" {
    run docker exec "$CTR_WORKSPACE" getent hosts gate
    assert_success
}

# ── Tools & OS ────────────────────────────────────────────────────────────

@test "workspace: curl available" {
    run docker exec "$CTR_WORKSPACE" which curl
    assert_success
}

# Source: services/workspace/Dockerfile FROM debian:trixie
@test "workspace: based on Debian" {
    run docker exec "$CTR_WORKSPACE" cat /etc/os-release
    assert_success
    assert_output --partial "Debian"
}

# ── User (source: services/workspace/Dockerfile useradd -m -u 1000 -s /bin/bash polis) ──

@test "workspace: polis user has /bin/bash" {
    run docker exec "$CTR_WORKSPACE" getent passwd polis
    assert_success
    assert_output --partial "/bin/bash"
}
