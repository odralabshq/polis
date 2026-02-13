#!/usr/bin/env bats
# Hardening Verification Tests
# Ensures that workspace isolation, capabilities, and seccomp are correctly applied

setup() {
    load "../helpers/common.bash"
}

@test "hardening: workspace configuration has CAP_DROP=ALL" {
    # Verify the Docker configuration explicitly drops all capabilities
    run docker inspect --format '{{.HostConfig.CapDrop}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output --partial "ALL"
}

@test "hardening: workspace configuration has Seccomp profile" {
    # Verify that a custom seccomp profile is applied (contains the JSON profile)
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output --partial "seccomp={"
    assert_output --partial "IP_TRANSPARENT"
}

@test "hardening: workspace configuration has no-new-privileges" {
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output --partial "no-new-privileges:true"
}

@test "hardening: process-level verification (Seccomp and NoNewPrivs)" {
    # Seccomp: 2 means filtered
    # NoNewPrivs: 1 means set
    run docker exec "${WORKSPACE_CONTAINER}" grep -E "(Seccomp|NoNewPrivs):" /proc/1/status
    assert_output --regexp "Seccomp:[[:space:]]+2"
    assert_output --regexp "NoNewPrivs:[[:space:]]+1"
}

@test "hardening: internal-bridge is strictly internal" {
    run docker network inspect --format '{{.Internal}}' "${NETWORK_INTERNAL}"
    assert_success
    assert_output "true"
}

@test "hardening: workspace has resource limits applied" {
    run docker inspect "${WORKSPACE_CONTAINER}" --format '{{.HostConfig.Memory}} {{.HostConfig.NanoCpus}}'
    assert_success
    assert_output "4294967296 2000000000"
}

@test "hardening: ipv6 is disabled via persistent sysctls" {
    run docker exec "${WORKSPACE_CONTAINER}" sysctl -n net.ipv6.conf.all.disable_ipv6
    assert_success
    assert_output "1"
    
    run docker exec "${WORKSPACE_CONTAINER}" sysctl -n net.ipv6.conf.default.disable_ipv6
    assert_success
    assert_output "1"
}

@test "hardening: traffic to internet is inspected and forced through proxy" {
    # Verify traffic goes through the gate (Via: ICAP)
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -D - -o /dev/null --connect-timeout 5 http://1.1.1.1
    assert_success
    assert_output --partial "Via: ICAP"
}

@test "hardening: workspace systemd initialization status" {
    run docker exec "${WORKSPACE_CONTAINER}" systemctl is-system-running
    assert_success || [[ "$output" == "degraded" ]]
}
