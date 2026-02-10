#!/usr/bin/env bats
# DLP Module Unit Tests
# Tests for srv_molis_dlp c-ICAP service presence and configuration

setup() {
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"
    ICAP_CONTAINER="polis-icap"
}

@test "dlp: module binary exists in container" {
    run docker exec "${ICAP_CONTAINER}" ls /usr/lib/c_icap/srv_molis_dlp.so
    assert_success
}

@test "dlp: module file is not empty" {
    run docker exec "${ICAP_CONTAINER}" sh -c "test -s /usr/lib/c_icap/srv_molis_dlp.so"
    assert_success
}

@test "dlp: config file exists and is mounted read-only" {
    run docker exec "${ICAP_CONTAINER}" test -f /etc/c-icap/molis_dlp.conf
    assert_success

    # Verify read-only mount
    run docker exec "${ICAP_CONTAINER}" sh -c "touch /etc/c-icap/molis_dlp.conf 2>&1"
    assert_output --partial "Read-only file system"
}

@test "dlp: c-icap is configured to load the module" {
    run docker exec "${ICAP_CONTAINER}" grep "Service molis_dlp srv_molis_dlp.so" /etc/c-icap/c-icap.conf
    assert_success
}

@test "dlp: service alias 'credcheck' is configured" {
    run docker exec "${ICAP_CONTAINER}" grep "ServiceAlias credcheck molis_dlp" /etc/c-icap/c-icap.conf
    assert_success
}

@test "dlp: config contains credential patterns" {
    run docker exec "${ICAP_CONTAINER}" grep -c "^pattern\." /etc/c-icap/molis_dlp.conf
    assert_success
    # Should have at least 5 patterns
    [[ "$output" -ge 5 ]]
}

@test "dlp: config contains allow rules" {
    run docker exec "${ICAP_CONTAINER}" grep -c "^allow\." /etc/c-icap/molis_dlp.conf
    assert_success
    [[ "$output" -ge 4 ]]
}

@test "dlp: config contains always-block actions" {
    run docker exec "${ICAP_CONTAINER}" grep -c "^action\." /etc/c-icap/molis_dlp.conf
    assert_success
    [[ "$output" -ge 3 ]]
}

@test "dlp: g3proxy is routing REQMOD to credcheck" {
    GATEWAY_CONTAINER="polis-gateway"
    run docker exec "${GATEWAY_CONTAINER}" grep "icap://icap:1344/credcheck" /etc/g3proxy/g3proxy.yaml
    assert_success
}

# =============================================================================
# Initialization Tests (Requirement 4)
# =============================================================================

@test "dlp: fail-closed if no patterns loaded" {
    # Backup config, create empty one, restart icap, check logs, restore
    run docker exec "${ICAP_CONTAINER}" sh -c "mv /etc/c-icap/molis_dlp.conf /etc/c-icap/molis_dlp.conf.bak && touch /etc/c-icap/molis_dlp.conf"
    assert_success
    
    # Restart c-icap (this will fail to start the module)
    run docker restart "${ICAP_CONTAINER}"
    assert_success
    
    # Wait for restart attempt
    sleep 2
    
    # Check logs for CRITICAL and CWE-636
    run docker logs "${ICAP_CONTAINER}"
    assert_output --partial "CRITICAL: No credential patterns loaded"
    assert_output --partial "fail-closed, CWE-636"
    
    # Restore config
    run docker exec "${ICAP_CONTAINER}" sh -c "mv /etc/c-icap/molis_dlp.conf.bak /etc/c-icap/molis_dlp.conf"
    assert_success
    
    # Restart to healthy state
    run docker restart "${ICAP_CONTAINER}"
    assert_success
}
