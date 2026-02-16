#!/bin/bash
set -euo pipefail

echo "[gateway] Starting initialization..."

# Network setup (runs as root before privilege drop)
if [[ -x /setup-network.sh ]]; then
    echo "[gateway] Running network setup..."
    /setup-network.sh
fi

# Certificate validation (fail-fast) - only ca.pem, ca.key is in certgen sidecar
validate_certificates() {
    local ca_cert="/etc/g3proxy/ssl/ca.pem"
    
    if [[ ! -f "$ca_cert" ]]; then
        echo "[gateway] ERROR: CA certificate not found: $ca_cert"
        exit 1
    fi
    
    # Validate certificate is not expired
    if ! openssl x509 -checkend 86400 -noout -in "$ca_cert" 2>/dev/null; then
        echo "[gateway] ERROR: CA certificate expires within 24 hours"
        exit 1
    fi
    
    echo "[gateway] Certificate validation passed"
}

# Run validation first
validate_certificates

# Wait for certgen sidecar to be ready (UDP port check)
echo "[gateway] Waiting for certgen sidecar..."
for i in {1..30}; do
    if timeout 1 bash -c "echo > /dev/udp/certgen/2999" 2>/dev/null; then
        echo "[gateway] Certgen ready at certgen:2999"
        break
    fi
    sleep 1
done

# Wait for ICAP service to be ready (TCP port check)
echo "[gateway] Waiting for ICAP service..."
for i in {1..30}; do
    if timeout 1 bash -c "echo > /dev/tcp/sentinel/1344" 2>/dev/null; then
        echo "[gateway] ICAP service ready at sentinel:1344"
        break
    fi
    sleep 1
done

# Clean up stale sockets
rm -rf /tmp/g3/*

# Start g3proxy with ambient capabilities (replaces current process)
# g3fcgen now runs in separate certgen container
echo "[gateway] Starting g3proxy..."
exec setpriv --reuid 65532 --regid 65532 --init-groups \
  --inh-caps +net_admin,+net_raw \
  --ambient-caps +net_admin,+net_raw \
  -- g3proxy -c /etc/g3proxy/g3proxy.yaml
