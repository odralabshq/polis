#!/usr/bin/env bash
# Single source of truth — all container names, IPs, subnets, ports.
# Matches docker-compose.yml exactly.

# Container names
export CTR_RESOLVER="polis-resolver"
export CTR_GATE="polis-gate"
export CTR_CERTGEN="polis-certgen"
export CTR_SENTINEL="polis-sentinel"
export CTR_SCANNER="polis-scanner"
export CTR_STATE="polis-state"
export CTR_TOOLBOX="polis-toolbox"
export CTR_WORKSPACE="polis-workspace"

# Init containers
export CTR_SCANNER_INIT="polis-scanner-init"
export CTR_STATE_INIT="polis-state-init"

# Network names
export NET_INTERNAL="polis_internal-bridge"
export NET_GATEWAY="polis_gateway-bridge"
export NET_EXTERNAL="polis_external-bridge"
export NET_INTERNET="polis_internet"

# Subnets
export SUBNET_INTERNAL="10.10.1.0/24"
export SUBNET_GATEWAY="10.30.1.0/24"
export SUBNET_EXTERNAL="10.20.1.0/24"

# Static IPs
export IP_RESOLVER_GW="10.30.1.10"
export IP_RESOLVER_INT="10.10.1.2"
export IP_GATE_INT="10.10.1.10"
export IP_GATE_GW="10.30.1.6"
export IP_GATE_EXT="10.20.1.3"
export IP_CERTGEN="10.30.1.7"
export IP_SENTINEL="10.30.1.5"
export IP_TOOLBOX_INT="10.10.1.20"
export IP_TOOLBOX_GW="10.30.1.20"

# Ports
export PORT_TPROXY=18080
export PORT_ICAP=1344
export PORT_CLAMAV=3310
export PORT_VALKEY=6379
export PORT_MCP=8080
export PORT_DNS=53
export PORT_G3FCGEN=2999

# All long-running containers (for iteration)
export ALL_CONTAINERS=("$CTR_RESOLVER" "$CTR_GATE" "$CTR_CERTGEN" "$CTR_SENTINEL" "$CTR_SCANNER" "$CTR_STATE" "$CTR_TOOLBOX" "$CTR_WORKSPACE")
export ALL_INIT_CONTAINERS=("$CTR_SCANNER_INIT" "$CTR_STATE_INIT")

# Test profile containers
export CTR_HTTPBIN="polis-httpbin"
export HTTPBIN_HOST="10.20.1.100:8080"
# Gate HTTP proxy — used to route workspace traffic through g3proxy→ICAP to httpbin
export HTTP_PROXY_VIA_GATE="http://${IP_GATE_INT}:8080"
