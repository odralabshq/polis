#!/usr/bin/env bats
# Sentinel Resilience Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$ICAP_CONTAINER"
}

@test "resilience: icap has healthcheck configured" {
    run docker inspect --format '{{.Config.Healthcheck.Test}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output --partial "c-icap"
}

@test "resilience: icap uses json-file logging driver" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "json-file"
}

@test "resilience: icap restart policy is unless-stopped" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}
