#!/usr/bin/env bats
# Scanner Configuration Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$CLAMAV_CONTAINER"
}

@test "config: clamd.conf exists" {
    run docker exec "${CLAMAV_CONTAINER}" test -f /etc/clamav/clamd.conf
    assert_success
}

@test "config: clamd listens on 0.0.0.0:3310" {
    run docker exec "${CLAMAV_CONTAINER}" grep "^TCPSocket 3310" /etc/clamav/clamd.conf
    assert_success
}
