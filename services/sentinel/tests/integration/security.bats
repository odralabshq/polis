#!/usr/bin/env bats
# Sentinel Security Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$ICAP_CONTAINER"
}

@test "security: icap is NOT running privileged" {
    run docker inspect --format '{{.HostConfig.Privileged}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "security: icap has minimal added capabilities" {
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output --partial "CHOWN"
    # SETUID and SETGID removed — sentinel runs as fixed user
    # assert_output --partial "SETUID"
    # assert_output --partial "SETGID"
    refute_output --partial "SYS_ADMIN"
}

@test "security: icap runs as sentinel user" {
    run docker exec "${ICAP_CONTAINER}" ps -o user= -p $(docker exec "${ICAP_CONTAINER}" pgrep -x c-icap | head -1)
    assert_success
    assert_output "sentinel"
}

@test "supply-chain: icap Dockerfile has SHA256 verification" {
    run grep -E "sha256sum -c" "${PROJECT_ROOT}/services/sentinel/Dockerfile"
    assert_success
}
