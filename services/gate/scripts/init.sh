#!/bin/bash
set -euo pipefail

echo "[gateway] Starting initialization (non-root)..."

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
# We use fixed IP 10.30.1.5 to avoid DNS dependency during early boot
echo "[gateway] Waiting for ICAP service at 10.30.1.5..."
for i in {1..30}; do
    if timeout 1 bash -c "echo > /dev/tcp/10.30.1.5/1344" 2>/dev/null; then
        echo "[gateway] ICAP service ready at 10.30.1.5:1344"
        break
    fi
    sleep 1
done

# Clean up stale sockets
# Directory /tmp/g3 is owned by g3proxy from Dockerfile
rm -rf /tmp/g3/*

# Start g3fcgen (background)
echo "[gateway] Starting g3fcgen..."
g3fcgen -c /etc/g3proxy/g3fcgen.yaml &

# Start g3proxy (replaces current process)
echo "[gateway] Starting g3proxy..."
exec g3proxy -c /etc/g3proxy/g3proxy.yaml
