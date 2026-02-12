#!/usr/bin/env bats
# Security Fixes Integration Tests
# Tests for: SHA256 verification, privilege dropping, malware scan bypass removal

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER" "$WORKSPACE_CONTAINER"
}

# =============================================================================
# Supply Chain Security - SHA256 Checksum Verification
# =============================================================================

@test "supply-chain: g3proxy Dockerfile has SHA256 verification" {
    run grep -E "sha256sum -c" "${PROJECT_ROOT}/build/g3proxy/Dockerfile"
    assert_success
}

@test "supply-chain: g3proxy Dockerfile pins G3_SHA256 hash" {
    run grep -E "^ENV G3_SHA256=" "${PROJECT_ROOT}/build/g3proxy/Dockerfile"
    assert_success
    assert_output --partial "4aff3f3ea50774b5346859b3ef1f120c5dba70e6cef168fbb9ccdc9168fa0ff5"
}

@test "supply-chain: icap Dockerfile has SHA256 verification for c-icap" {
    run grep -E "CICAP_SHA256.*sha256sum -c" "${PROJECT_ROOT}/build/icap/Dockerfile"
    assert_success
}

@test "supply-chain: icap Dockerfile pins CICAP_SHA256 hash" {
    run grep -E "^ENV CICAP_SHA256=" "${PROJECT_ROOT}/build/icap/Dockerfile"
    assert_success
    assert_output --partial "ecc7789787fe4eceae807f0717f6da18c5b1fbf7d2d26028711222eb154c82fe"
}

@test "supply-chain: icap Dockerfile has SHA256 verification for squidclamav" {
    run grep -E "SQUIDCLAMAV_SHA256.*sha256sum -c" "${PROJECT_ROOT}/build/icap/Dockerfile"
    assert_success
}

@test "supply-chain: icap Dockerfile pins SQUIDCLAMAV_SHA256 hash" {
    run grep -E "^ENV SQUIDCLAMAV_SHA256=" "${PROJECT_ROOT}/build/icap/Dockerfile"
    assert_success
    assert_output --partial "772c058d113a17de0e96b3c8acaad75d78cd4c000f325b865d6c88503c353fc9"
}

# =============================================================================
# Privilege Dropping - Gateway runs as non-root after init
# =============================================================================

@test "privilege-drop: gateway Dockerfile creates g3proxy user" {
    run grep -E "useradd.*g3proxy" "${PROJECT_ROOT}/build/g3proxy/Dockerfile"
    assert_success
}

@test "privilege-drop: gateway Dockerfile installs util-linux for setpriv" {
    run grep -E "util-linux" "${PROJECT_ROOT}/build/g3proxy/Dockerfile"
    assert_success
}

@test "privilege-drop: docker-compose does not set user: root" {
    run grep -E "^\s+user:\s*root" "${PROJECT_ROOT}/docker-compose.yml"
    assert_failure  # Should NOT find user: root
}

@test "privilege-drop: docker-compose drops ALL capabilities" {
    run grep -A1 "cap_drop:" "${PROJECT_ROOT}/docker-compose.yml"
    assert_success
    assert_output --partial "ALL"
}

@test "privilege-drop: init script uses setpriv for privilege drop with ambient caps" {
    run grep -E "exec setpriv" "${PROJECT_ROOT}/scripts/g3proxy-init.sh"
    assert_success
    run grep -E "ambient-caps" "${PROJECT_ROOT}/scripts/g3proxy-init.sh"
    assert_success
}

@test "privilege-drop: g3proxy process runs as g3proxy user" {
    skip_if_containers_not_running
    run docker exec "${GATEWAY_CONTAINER}" ps -o user= -p $(docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy | head -1)
    assert_success
    assert_output "g3proxy"
}

@test "privilege-drop: g3proxy user has no login shell" {
    skip_if_containers_not_running
    run docker exec "${GATEWAY_CONTAINER}" getent passwd g3proxy
    assert_success
    assert_output --partial "/sbin/nologin"
}

@test "privilege-drop: /tmp/g3 owned by g3proxy" {
    skip_if_containers_not_running
    run docker exec "${GATEWAY_CONTAINER}" stat -c '%U' /tmp/g3
    assert_success
    assert_output "g3proxy"
}

@test "privilege-drop: /var/log/g3proxy owned by g3proxy" {
    skip_if_containers_not_running
    run docker exec "${GATEWAY_CONTAINER}" stat -c '%U' /var/log/g3proxy
    assert_success
    assert_output "g3proxy"
}

# =============================================================================
# Build-time Ownership - No runtime chown needed (no CHOWN capability)
# =============================================================================

@test "build-ownership: gateway does NOT have CHOWN capability" {
    skip_if_containers_not_running
    local caps
    # Check gateway service specifically (not clamav which needs CHOWN)
    caps=$(docker inspect --format '{{.HostConfig.CapAdd}}' "${GATEWAY_CONTAINER}")
    [[ ! "$caps" =~ "CHOWN" ]]
}

@test "build-ownership: Dockerfile sets ownership at build time" {
    run grep -E "chown.*g3proxy.*(/var/log/g3proxy|/var/lib/g3proxy|/tmp/g3)" "${PROJECT_ROOT}/build/g3proxy/Dockerfile"
    assert_success
}

@test "build-ownership: init script does NOT chown directories" {
    run grep -E "chown.*(g3proxy|/var/log|/var/lib|/tmp/g3)" "${PROJECT_ROOT}/scripts/g3proxy-init.sh"
    assert_failure  # Should NOT find chown in init script
}

@test "build-ownership: g3proxy runs with minimal capabilities" {
    skip_if_containers_not_running
    # g3proxy should only have CAP_NET_ADMIN (0x1000) in effective set
    local pid
    pid=$(docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy | head -1)
    local cap_eff
    cap_eff=$(docker exec "${GATEWAY_CONTAINER}" cat /proc/${pid}/status | grep "CapEff:" | awk '{print $2}')
    # Should be exactly 0x1000 (CAP_NET_ADMIN only)
    [[ "$cap_eff" == "0000000000001000" ]]
}

# =============================================================================
# Malware Scan Bypass Removal - No extension/MIME bypass
# =============================================================================

@test "scan-bypass: squidclamav.conf has no abort directives" {
    run grep -E "^abort\s" "${PROJECT_ROOT}/config/squidclamav.conf"
    assert_failure  # Should NOT find any abort directives
}

@test "scan-bypass: squidclamav.conf has no abortcontent directives" {
    run grep -E "^abortcontent\s" "${PROJECT_ROOT}/config/squidclamav.conf"
    assert_failure  # Should NOT find any abortcontent directives
}

@test "scan-bypass: squidclamav.conf documents security fix" {
    run grep -i "abort" "${PROJECT_ROOT}/config/squidclamav.conf"
    assert_success
    assert_output --partial "No abort"
}

@test "scan-bypass: running config has no abort directives" {
    skip_if_containers_not_running
    run docker exec "${ICAP_CONTAINER}" grep -E "^abort\s" /etc/squidclamav.conf
    assert_failure
}

@test "scan-bypass: running config has no abortcontent directives" {
    skip_if_containers_not_running
    run docker exec "${ICAP_CONTAINER}" grep -E "^abortcontent\s" /etc/squidclamav.conf
    assert_failure
}

@test "scan-bypass: ClamAV detects EICAR with .png extension" {
    skip_if_containers_not_running
    # EICAR test string
    local EICAR='X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*'
    run docker exec "${CLAMAV_CONTAINER}" sh -c "echo '${EICAR}' > /tmp/fake.png && clamdscan /tmp/fake.png 2>&1; rm -f /tmp/fake.png"
    assert_output --partial "FOUND"
}

@test "scan-bypass: ClamAV detects EICAR with .jpg extension" {
    skip_if_containers_not_running
    local EICAR='X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*'
    run docker exec "${CLAMAV_CONTAINER}" sh -c "echo '${EICAR}' > /tmp/fake.jpg && clamdscan /tmp/fake.jpg 2>&1; rm -f /tmp/fake.jpg"
    assert_output --partial "FOUND"
}

@test "scan-bypass: ClamAV detects EICAR with .gif extension" {
    skip_if_containers_not_running
    local EICAR='X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*'
    run docker exec "${CLAMAV_CONTAINER}" sh -c "echo '${EICAR}' > /tmp/fake.gif && clamdscan /tmp/fake.gif 2>&1; rm -f /tmp/fake.gif"
    assert_output --partial "FOUND"
}
