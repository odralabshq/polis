#!/bin/bash
# setup-certs.sh — Generate TLS certificates and secrets for Polis services
# Runs at first boot via systemd ExecStartPre
set -euo pipefail

cd /opt/polis

# Skip if already generated
if [[ -f certs/ca/ca.key && -f secrets/valkey_users.acl ]]; then
    echo "==> Certificates and secrets already exist, skipping"
    exit 0
fi

echo "==> Generating certificates and secrets..."

mkdir -p certs/ca certs/valkey certs/toolbox secrets

# 1. Generate CA
echo "==> Generating CA..."
openssl genrsa -out certs/ca/ca.key 4096
openssl req -new -x509 -days 3650 -key certs/ca/ca.key -out certs/ca/ca.pem \
    -subj "/C=US/ST=Local/L=Local/O=Polis/OU=Gateway/CN=Polis CA"
chmod 644 certs/ca/ca.key certs/ca/ca.pem

# 2. Generate Valkey certs and secrets
echo "==> Generating Valkey certs..."
./services/state/scripts/generate-certs.sh ./certs/valkey
./services/state/scripts/generate-secrets.sh ./secrets .

# 3. Generate Toolbox certs
echo "==> Generating Toolbox certs..."
./services/toolbox/scripts/generate-certs.sh ./certs/toolbox ./certs/ca

# 4. Set ownership for containers (uid 65532)
chown 65532:65532 certs/ca/ca.key certs/ca/ca.pem
chown 65532:65532 certs/valkey/server.key certs/valkey/client.key
chown 65532:65532 certs/toolbox/toolbox.key

# 5. Create .env
touch .env

echo "==> Setup complete"
