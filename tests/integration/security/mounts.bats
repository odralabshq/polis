#!/usr/bin/env bats
# bats file_tags=integration,security
# Integration tests for volume mounts, tmpfs, and named volumes

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    for ctr in "$CTR_GATE" "$CTR_SENTINEL" "$CTR_SCANNER" "$CTR_STATE" "$CTR_WORKSPACE" "$CTR_TOOLBOX"; do
        local var="${ctr//-/_}_INSPECT"
        export "$var"="$(docker inspect "$ctr" 2>/dev/null || echo '[]')"
    done
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

_inspect() { local var="${1//-/_}_INSPECT"; echo "${!var}"; }

# ── Read-only bind mounts ─────────────────────────────────────────────────

@test "gate: config mounted read-only" {
    require_container "$CTR_GATE"
    # Source: docker-compose.yml ./services/gate/config/g3proxy.yaml:/etc/g3proxy/g3proxy.yaml:ro
    run jq -r '.[0].Mounts[] | select(.Destination=="/etc/g3proxy/g3proxy.yaml") | .RW' <<< "$(_inspect "$CTR_GATE")"
    assert_output "false"
}

@test "sentinel: config mounted read-only" {
    require_container "$CTR_SENTINEL"
    # Source: docker-compose.yml ./services/sentinel/config/c-icap.conf:/etc/c-icap/c-icap.conf:ro
    run jq -r '.[0].Mounts[] | select(.Destination=="/etc/c-icap/c-icap.conf") | .RW' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output "false"
}

@test "sentinel: ClamAV DB mounted read-only" {
    require_container "$CTR_SENTINEL"
    # Source: docker-compose.yml scanner-db:/var/lib/clamav:ro
    run jq -r '.[0].Mounts[] | select(.Destination=="/var/lib/clamav") | .RW' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output "false"
}

@test "scanner: ClamAV DB mounted read-write" {
    require_container "$CTR_SCANNER"
    # Source: docker-compose.yml scanner-db:/var/lib/clamav (no :ro)
    run jq -r '.[0].Mounts[] | select(.Destination=="/var/lib/clamav") | .RW' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output "true"
}

# ── Tmpfs mounts ──────────────────────────────────────────────────────────

@test "gate: has /tmp tmpfs with 50M size" {
    require_container "$CTR_GATE"
    # Source: docker-compose.yml tmpfs: /tmp:size=50M,mode=1777,uid=999,gid=999
    run jq -r '.[0].HostConfig.Tmpfs["/tmp"] // empty' <<< "$(_inspect "$CTR_GATE")"
    assert_success
    assert_output --partial "size=50"
}

@test "gate: has /var/log/g3proxy tmpfs" {
    require_container "$CTR_GATE"
    # Source: docker-compose.yml tmpfs: /var/log/g3proxy:size=50M,mode=755,uid=999,gid=999
    run jq -r '.[0].HostConfig.Tmpfs["/var/log/g3proxy"] // empty' <<< "$(_inspect "$CTR_GATE")"
    assert_success
    assert_output --partial "size=50"
}

@test "gate: has /var/lib/g3proxy tmpfs" {
    require_container "$CTR_GATE"
    # Source: docker-compose.yml tmpfs: /var/lib/g3proxy:size=10M,mode=755,uid=999,gid=999
    run jq -r '.[0].HostConfig.Tmpfs["/var/lib/g3proxy"] // empty' <<< "$(_inspect "$CTR_GATE")"
    assert_success
    assert_output --partial "size=10"
}

@test "sentinel: has /tmp tmpfs with 2G size" {
    require_container "$CTR_SENTINEL"
    # Source: docker-compose.yml tmpfs: /tmp:size=2G,mode=1777
    run jq -r '.[0].HostConfig.Tmpfs["/tmp"] // empty' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_success
    assert_output --partial "size=2"
}

@test "sentinel: has /var/log tmpfs with 100M size" {
    require_container "$CTR_SENTINEL"
    # Source: docker-compose.yml tmpfs: /var/log:size=100M
    run jq -r '.[0].HostConfig.Tmpfs["/var/log"] // empty' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_success
    assert_output --partial "size=100"
}

@test "scanner: has /tmp tmpfs" {
    require_container "$CTR_SCANNER"
    # Source: docker-compose.yml tmpfs: /tmp:size=100M,mode=1777
    run jq -r '.[0].HostConfig.Tmpfs["/tmp"] // empty' <<< "$(_inspect "$CTR_SCANNER")"
    assert_success
    assert_output --partial "size="
}

@test "state: has /tmp tmpfs" {
    require_container "$CTR_STATE"
    # Source: docker-compose.yml tmpfs: /tmp:size=10M,mode=1777
    run jq -r '.[0].HostConfig.Tmpfs["/tmp"] // empty' <<< "$(_inspect "$CTR_STATE")"
    assert_success
    assert_output --partial "size="
}

@test "toolbox: has /tmp tmpfs" {
    require_container "$CTR_TOOLBOX"
    # Source: docker-compose.yml tmpfs: /tmp:size=50M,mode=1777
    run jq -r '.[0].HostConfig.Tmpfs["/tmp"] // empty' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_success
    assert_output --partial "size=50"
}

# ── Named volumes ─────────────────────────────────────────────────────────

@test "scanner-db: named volume exists" {
    # Source: docker-compose.yml volumes: scanner-db: name: polis-scanner-db
    run docker volume inspect polis-scanner-db
    assert_success
}

@test "state-data: named volume exists" {
    # Source: docker-compose.yml volumes: state-data: name: polis-state-data
    run docker volume inspect polis-state-data
    assert_success
}

# ── Workspace sensitive paths ─────────────────────────────────────────────

@test "workspace: sensitive paths are tmpfs" {
    require_container "$CTR_WORKSPACE"
    # Source: docker-compose.yml type: tmpfs targets: /root/.ssh, /root/.aws, /root/.gnupg
    local mounts
    mounts=$(jq -r '.[0].Mounts[] | select(.Type=="tmpfs") | .Destination' <<< "$(_inspect "$CTR_WORKSPACE")")
    echo "$mounts" | grep -q "/root/.ssh" || fail "/root/.ssh not tmpfs"
    echo "$mounts" | grep -q "/root/.aws" || fail "/root/.aws not tmpfs"
    echo "$mounts" | grep -q "/root/.gnupg" || fail "/root/.gnupg not tmpfs"
}

@test "workspace: CA cert mounted read-only" {
    require_container "$CTR_WORKSPACE"
    # Source: docker-compose.yml ./certs/ca/ca.pem:/usr/local/share/ca-certificates/polis-ca.crt:ro
    run jq -r '.[0].Mounts[] | select(.Destination=="/usr/local/share/ca-certificates/polis-ca.crt") | .RW' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_output "false"
}
