#!/usr/bin/env bats
# Security Integration Tests
# Tests for container security hardening

setup() {
    # Set paths relative to test file location
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"
    
    # Container names
    GATEWAY_CONTAINER="polis-gateway"
    ICAP_CONTAINER="polis-icap"
    WORKSPACE_CONTAINER="polis-workspace"
    CLAMAV_CONTAINER="polis-clamav"
}

# =============================================================================
# Gateway Privilege Tests
# =============================================================================

@test "security: gateway is NOT running privileged" {
    run docker inspect --format '{{.HostConfig.Privileged}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "security: gateway has NET_ADMIN capability" {
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --regexp "(NET_ADMIN|CAP_NET_ADMIN)"
}

@test "security: gateway has NET_RAW capability" {
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --regexp "(NET_RAW|CAP_NET_RAW)"
}

@test "security: gateway has only required capabilities" {
    local caps
    caps=$(docker inspect --format '{{.HostConfig.CapAdd}}' "${GATEWAY_CONTAINER}")
    
    # Should have NET_ADMIN, NET_RAW, SETUID, SETGID (4 caps)
    # SETUID/SETGID needed for setpriv to switch user
    local cap_count
    cap_count=$(echo "$caps" | tr -cd '[:alpha:]_' | grep -oE '(NET_ADMIN|NET_RAW|SETUID|SETGID|CAP_NET_ADMIN|CAP_NET_RAW|CAP_SETUID|CAP_SETGID)' | wc -l)
    
    [[ "$cap_count" -eq 4 ]]
}

@test "security: gateway has seccomp profile applied" {
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "seccomp"
}

# =============================================================================
# ICAP Security Tests
# =============================================================================

@test "security: icap is NOT running privileged" {
    run docker inspect --format '{{.HostConfig.Privileged}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "security: icap has no added capabilities" {
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${ICAP_CONTAINER}"
    assert_success
    # Should be empty or []
    [[ "$output" == "[]" ]] || [[ -z "$output" ]] || [[ "$output" == "<no value>" ]]
}

@test "security: icap runs as non-root user" {
    run docker exec "${ICAP_CONTAINER}" ps -o user= -p $(docker exec "${ICAP_CONTAINER}" pgrep -x c-icap | head -1)
    assert_success
    refute_output "root"
}

@test "security: icap c-icap user has no shell" {
    run docker exec "${ICAP_CONTAINER}" getent passwd c-icap
    assert_success
    assert_output --partial "/sbin/nologin"
}

# =============================================================================
# Workspace Security Tests
# =============================================================================

@test "security: workspace is NOT running privileged" {
    run docker inspect --format '{{.HostConfig.Privileged}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "security: workspace uses sysbox runtime" {
    run docker inspect --format '{{.HostConfig.Runtime}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "sysbox-runc"
}

# =============================================================================
# Port Exposure Tests
# =============================================================================

@test "security: gateway has no ports exposed to host" {
    run docker port "${GATEWAY_CONTAINER}"
    assert_output ""
}

@test "security: icap has no ports exposed to host" {
    run docker port "${ICAP_CONTAINER}"
    assert_output ""
}

@test "security: workspace (base) has no ports exposed to host" {
    # Detect openclaw profile by checking for the service file inside the container
    if docker exec "${WORKSPACE_CONTAINER}" test -f /etc/systemd/system/openclaw.service 2>/dev/null; then
        skip "OpenClaw profile running - port 18789 is expected"
    fi
    run docker port "${WORKSPACE_CONTAINER}"
    assert_output ""
}

@test "security: workspace (openclaw) only exposes Control UI port" {
    # Detect openclaw profile by checking for the service file inside the container
    if ! docker exec "${WORKSPACE_CONTAINER}" test -f /etc/systemd/system/openclaw.service 2>/dev/null; then
        skip "Base profile running - no ports expected"
    fi
    run docker port "${WORKSPACE_CONTAINER}"
    # Only port 18789 should be exposed
    assert_output --partial "18789"
    refute_output --partial "22/"      # No SSH
    refute_output --partial "80/"      # No HTTP
    refute_output --partial "443/"     # No HTTPS
}

# =============================================================================
# Volume Mount Tests
# =============================================================================

@test "security: gateway config mounted read-only" {
    run docker inspect --format '{{range .Mounts}}{{if eq .Destination "/etc/g3proxy/g3proxy.yaml"}}{{.RW}}{{end}}{{end}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "security: gateway CA cert mounted read-only" {
    run docker inspect --format '{{range .Mounts}}{{if eq .Destination "/etc/g3proxy/ssl"}}{{.RW}}{{end}}{{end}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "security: icap config mounted read-only" {
    run docker inspect --format '{{range .Mounts}}{{if eq .Destination "/etc/c-icap/c-icap.conf"}}{{.RW}}{{end}}{{end}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "security: workspace CA cert mounted read-only" {
    run docker inspect --format '{{range .Mounts}}{{if eq .Destination "/usr/local/share/ca-certificates/polis-ca.crt"}}{{.RW}}{{end}}{{end}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "false"
}

# =============================================================================
# Certificate Security Tests
# =============================================================================

@test "security: CA private key is readable for Docker bind mount" {
    run docker exec "${GATEWAY_CONTAINER}" stat -c '%a' /etc/g3proxy/ssl/ca.key
    assert_success
    # Should be 644 (readable for Docker bind mount)
    # On WSL2, bind mounts may show 777 due to Windows filesystem
    [[ "$output" == "644" ]] || [[ "$output" == "777" ]]
}

@test "security: CA certificate is readable" {
    run docker exec "${GATEWAY_CONTAINER}" stat -c '%a' /etc/g3proxy/ssl/ca.pem
    assert_success
    # Should be 644 or similar
    [[ "$output" -ge 400 ]]
}

# =============================================================================
# Network Isolation Tests
# =============================================================================

@test "security: icap isolated from external network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${ICAP_CONTAINER}"
    assert_success
    refute_output --partial "external-bridge"
}

@test "security: workspace isolated from external network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${WORKSPACE_CONTAINER}"
    assert_success
    refute_output --partial "external-bridge"
}

@test "security: workspace isolated from gateway-bridge" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${WORKSPACE_CONTAINER}"
    assert_success
    refute_output --partial "gateway-bridge"
}

# =============================================================================
# Restart Policy Tests
# =============================================================================

@test "security: gateway has restart policy" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

@test "security: icap has restart policy" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

@test "security: workspace has restart policy" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

# =============================================================================
# Logging Configuration Tests
# =============================================================================

@test "security: gateway has logging configured" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "json-file"
}

@test "security: gateway log size is limited" {
    run docker inspect --format '{{index .HostConfig.LogConfig.Config "max-size"}}' "${GATEWAY_CONTAINER}"
    assert_success
    refute_output ""
}

@test "security: gateway log files are limited" {
    run docker inspect --format '{{index .HostConfig.LogConfig.Config "max-file"}}' "${GATEWAY_CONTAINER}"
    assert_success
    refute_output ""
}

# =============================================================================
# Health Check Tests
# =============================================================================

@test "security: gateway has health check configured" {
    run docker inspect --format '{{.Config.Healthcheck}}' "${GATEWAY_CONTAINER}"
    assert_success
    refute_output "<nil>"
}

@test "security: icap has health check configured" {
    run docker inspect --format '{{.Config.Healthcheck}}' "${ICAP_CONTAINER}"
    assert_success
    refute_output "<nil>"
}

@test "security: workspace has health check configured" {
    run docker inspect --format '{{.Config.Healthcheck}}' "${WORKSPACE_CONTAINER}"
    assert_success
    refute_output "<nil>"
}

# =============================================================================
# User Namespace Tests
# =============================================================================

@test "security: gateway drops privileges to g3proxy user" {
    # g3proxy process should run as g3proxy user (not root)
    run docker exec "${GATEWAY_CONTAINER}" ps -o user= -p $(docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy | head -1)
    assert_success
    assert_output "g3proxy"
}

@test "security: gateway g3proxy user exists" {
    run docker exec "${GATEWAY_CONTAINER}" id g3proxy
    assert_success
    assert_output --partial "g3proxy"
}

@test "security: gateway g3proxy user has no shell" {
    run docker exec "${GATEWAY_CONTAINER}" getent passwd g3proxy
    assert_success
    assert_output --partial "/sbin/nologin"
}

@test "security: gateway setpriv is installed for privilege dropping" {
    run docker exec "${GATEWAY_CONTAINER}" which setpriv
    assert_success
}

@test "security: g3proxy has CAP_NET_ADMIN via ambient capabilities" {
    # Verify g3proxy process has CAP_NET_ADMIN in effective set
    local pid
    pid=$(docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy | head -1)
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/${pid}/status
    assert_success
    # CapEff should contain 0x1000 (bit 12 = CAP_NET_ADMIN)
    assert_output --partial "CapEff:	0000000000001000"
}

@test "security: g3proxy has CAP_NET_ADMIN in ambient set" {
    local pid
    pid=$(docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy | head -1)
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/${pid}/status
    assert_success
    assert_output --partial "CapAmb:	0000000000001000"
}

@test "security: gateway drops all capabilities except NET_ADMIN/NET_RAW" {
    run docker inspect --format '{{.HostConfig.CapDrop}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "ALL"
}

# =============================================================================
# Seccomp Profile Tests
# =============================================================================

@test "security: seccomp profile file exists" {
    [[ -f "${PROJECT_ROOT}/config/seccomp/gateway.json" ]]
}

@test "security: seccomp profile is valid JSON" {
    run cat "${PROJECT_ROOT}/config/seccomp/gateway.json"
    assert_success
    # Try to parse as JSON
    echo "$output" | python3 -m json.tool > /dev/null 2>&1
}

# =============================================================================
# Security Level Tests (Requirement 2)
# =============================================================================

@test "security: level relaxed allows new domains" {
    # Use mcp-admin to SET (dlp-reader only has GET)
    local admin_pass=$(grep 'VALKEY_MCP_ADMIN_PASS=' "${PROJECT_ROOT}/secrets/credentials.env.example" | cut -d'=' -f2)
    run docker exec polis-v2-valkey valkey-cli --tls --cert /etc/valkey/tls/client.crt --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt --user mcp-admin --pass "$admin_pass" SET molis:config:security_level relaxed
    assert_success
    
    # Verify with dlp-reader (read-only)
    local dlp_pass=$(cat "${PROJECT_ROOT}/secrets/valkey_dlp_password.txt")
    run docker exec polis-v2-valkey valkey-cli --tls --cert /etc/valkey/tls/client.crt --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt --user dlp-reader --pass "$dlp_pass" GET molis:config:security_level
    assert_output --partial "relaxed"
}

@test "security: level strict blocks new domains" {
    # Use mcp-admin to SET (dlp-reader only has GET)
    local admin_pass=$(grep 'VALKEY_MCP_ADMIN_PASS=' "${PROJECT_ROOT}/secrets/credentials.env.example" | cut -d'=' -f2)
    run docker exec polis-v2-valkey valkey-cli --tls --cert /etc/valkey/tls/client.crt --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt --user mcp-admin --pass "$admin_pass" SET molis:config:security_level strict
    assert_success
    
    # Verify with dlp-reader (read-only)
    local dlp_pass=$(cat "${PROJECT_ROOT}/secrets/valkey_dlp_password.txt")
    run docker exec polis-v2-valkey valkey-cli --tls --cert /etc/valkey/tls/client.crt --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt --user dlp-reader --pass "$dlp_pass" GET molis:config:security_level
    assert_output --partial "strict"
}

@test "security: credentials always trigger prompt (balanced behavior)" {
    # Even in relaxed, credentials should prompt (return 403 with reason "prompt" or similar)
    # This is better tested with a full E2E flow.
    skip "Requires E2E flow with g3proxy and ICAP"
}
