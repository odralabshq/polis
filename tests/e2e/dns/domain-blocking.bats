#!/usr/bin/env bats
# bats file_tags=e2e,dns
# DNS resolution and domain accessibility from the workspace container

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_WORKSPACE" "$CTR_SENTINEL" "$CTR_SCANNER"
}

@test "e2e: workspace resolves external domains" {
    run docker exec "$CTR_WORKSPACE" getent hosts httpbin.org
    assert_success
}

@test "e2e: workspace resolves internal resolver" {
    # Source: docker-compose dns: 10.10.1.2
    run docker exec "$CTR_WORKSPACE" getent hosts resolver
    assert_success
    assert_output --partial "$IP_RESOLVER_INT"
}

@test "e2e: whitelisted repo accessible (Debian)" {
    run_with_network_skip "deb.debian.org" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --max-time 15 --connect-timeout 10 \
        http://deb.debian.org/debian/dists/stable/Release
    assert_success
    assert_output "200"
}

@test "e2e: whitelisted repo accessible (npm)" {
    run_with_network_skip "registry.npmjs.org" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --max-time 15 --connect-timeout 10 \
        https://registry.npmjs.org/
    assert_success
    assert_output --regexp "^(200|301)$"
}
