#!/bin/bash
set -euo pipefail

echo "[gateway] Starting initialization..."

# Network setup (runs as root before privilege drop)
if [[ -x /setup-network.sh ]]; then
    echo "[gateway] Running network setup..."
    /setup-network.sh
fi

# Certificate validation (fail-fast)
validate_certificates() {
    local ca_cert="/etc/g3proxy/ssl/ca.pem"
    local ca_key="/etc/g3proxy/ssl/ca.key"
    
    if [[ ! -f "$ca_cert" ]]; then
        echo "[gateway] ERROR: CA certificate not found: $ca_cert"
        exit 1
    fi
    
    if [[ ! -f "$ca_key" ]]; then
        echo "[gateway] ERROR: CA private key not found: $ca_key"
        exit 1
    fi
    
    # Validate certificate is not expired
    if ! openssl x509 -checkend 86400 -noout -in "$ca_cert" 2>/dev/null; then
        echo "[gateway] ERROR: CA certificate expires within 24 hours"
        exit 1
    fi
    
    # Validate key matches certificate (using SHA-256)
    local cert_modulus=$(openssl x509 -noout -modulus -in "$ca_cert" 2>/dev/null | openssl sha256)
    local key_modulus=$(openssl rsa -noout -modulus -in "$ca_key" 2>/dev/null | openssl sha256)
    
    if [[ "$cert_modulus" != "$key_modulus" ]]; then
        echo "[gateway] ERROR: CA certificate and key do not match"
        exit 1
    fi
    
    echo "[gateway] Certificate validation passed"
}

# Run validation first
validate_certificates

# Wait for ICAP service to be ready (TCP port check)
# Hostname 'sentinel' is resolved via Docker DNS
echo "[gateway] Waiting for ICAP service..."
for i in {1..30}; do
    if timeout 1 bash -c "echo > /dev/tcp/sentinel/1344" 2>/dev/null; then
        echo "[gateway] ICAP service ready at sentinel:1344"
        break
    fi
    sleep 1
done

# Clean up stale sockets
# Directory /tmp/g3 is owned by nonroot (65532) from Dockerfile
rm -rf /tmp/g3/*

# Start g3fcgen as nonroot user (background, no special caps needed)
echo "[gateway] Starting g3fcgen..."
setpriv --reuid 65532 --regid 65532 --init-groups -- g3fcgen -c /etc/g3proxy/g3fcgen.yaml &

# Start g3proxy with ambient capabilities (replaces current process)
# no-new-privileges blocks file caps from setcap, so we use ambient caps instead
echo "[gateway] Starting g3proxy..."
exec setpriv --reuid 65532 --regid 65532 --init-groups \
  --inh-caps +net_admin,+net_raw \
  --ambient-caps +net_admin,+net_raw \
  -- g3proxy -c /etc/g3proxy/g3proxy.yaml
