#!/usr/bin/env bash
# CI helper scripts - sourced by GitHub Actions workflows
set -euo pipefail

install_sysbox() {
    local version="${SYSBOX_VERSION:?SYSBOX_VERSION required}"
    local sha256="${SYSBOX_SHA256:?SYSBOX_SHA256 required}"
    
    docker rm -f "$(docker ps -aq)" 2>/dev/null || true
    wget -q --https-only --max-redirect=5 \
        "https://github.com/nestybox/sysbox/releases/download/v${version}/sysbox-ce_${version}.linux_amd64.deb"
    echo "${sha256}  sysbox-ce_${version}.linux_amd64.deb" | sha256sum -c -
    sudo apt-get install -y jq "./sysbox-ce_${version}.linux_amd64.deb"
    sudo mkdir -p /etc/docker
    echo '{"runtimes":{"sysbox-runc":{"path":"/usr/bin/sysbox-runc"}}}' | sudo tee /etc/docker/daemon.json
    sudo systemctl restart docker
    return 0
}

setup_certs_and_secrets() {
    chmod +x ./services/*/scripts/*.sh ./tests/run-tests.sh
    just setup-ca
    chmod 644 ./certs/ca/ca.pem
    chmod a+r ./certs/ca/ca.key
    mkdir -p ./certs/toolbox
    ./services/toolbox/scripts/generate-certs.sh ./certs/toolbox ./certs/ca
    ./services/state/scripts/generate-certs.sh ./certs/valkey
    chmod 644 ./certs/valkey/*.crt ./certs/toolbox/*.pem
    chmod a+r ./certs/valkey/*.key ./certs/toolbox/*.key
    touch .env
    ./services/state/scripts/generate-secrets.sh ./secrets .
    chmod 644 ./secrets/valkey_users.acl
    return 0
}

wait_for_healthy() {
    local timeout="${1:-60}"
    local containers="polis-gate polis-sentinel polis-state polis-toolbox polis-workspace"
    
    for i in $(seq 1 "$timeout"); do
        local healthy
        healthy=$(docker ps --filter "health=healthy" --format "{{.Names}}" | sort)
        local all_healthy=true
        for c in $containers; do
            if ! echo "$healthy" | grep -q "$c"; then
                all_healthy=false
                break
            fi
        done
        if $all_healthy; then
            echo "All containers healthy"
            return 0
        fi
        echo "Waiting... ($i/$timeout)"
        sleep 5
    done
    echo "Timeout waiting for containers"
    docker ps -a
    return 1
}

show_logs_on_failure() {
    for c in polis-gate polis-sentinel polis-scanner polis-state polis-toolbox polis-workspace; do
        echo "=== $c ==="
        docker logs "$c" 2>&1 | tail -30 || true
    done
    docker ps -a
    return 0
}

# Allow sourcing or direct execution
if [[ "${1:-}" != "" ]]; then
    "$@"
fi
