#!/usr/bin/env bats
# bats file_tags=integration,sentinel
# Sentinel Configuration Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$ICAP_CONTAINER"
}

@test "config: c-icap StartServers is 3" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "StartServers 3"
}

@test "config: c-icap listens on 0.0.0.0:1344" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "Port 0.0.0.0:1344"
}

@test "config: icap config mounted read-only" {
    run docker inspect "${ICAP_CONTAINER}" --format '{{json .Mounts}}'
    assert_success
    assert_output --partial "c-icap.conf"
    assert_output --partial '"RW":false'
}
