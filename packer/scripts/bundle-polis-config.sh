#!/bin/bash
# bundle-polis-config.sh — Create tarball of Polis config for VM image
# Run from polis repo root before packer build
set -euo pipefail

BUNDLE_DIR=$(mktemp -d)
trap 'rm -rf "$BUNDLE_DIR"' EXIT

echo "==> Bundling Polis configuration..."

# Copy docker-compose.yml and .env
# Strip @sha256:... digest suffixes from image references (docker load doesn't preserve digests)
sed 's/@sha256:[a-f0-9]\{64\}//g' docker-compose.yml > "$BUNDLE_DIR/docker-compose.yml"
touch "$BUNDLE_DIR/.env"

# Copy service configs
mkdir -p "$BUNDLE_DIR/services"
for svc in resolver certgen gate sentinel scanner state toolbox workspace; do
    if [[ -d "services/$svc/config" ]]; then
        mkdir -p "$BUNDLE_DIR/services/$svc"
        cp -r "services/$svc/config" "$BUNDLE_DIR/services/$svc/"
    fi
    if [[ -d "services/$svc/scripts" ]]; then
        mkdir -p "$BUNDLE_DIR/services/$svc"
        cp -r "services/$svc/scripts" "$BUNDLE_DIR/services/$svc/"
    fi
done

# Copy setup scripts
mkdir -p "$BUNDLE_DIR/scripts"
cp packer/scripts/setup-certs.sh "$BUNDLE_DIR/scripts/"
chmod +x "$BUNDLE_DIR/scripts/setup-certs.sh"

# Copy config
mkdir -p "$BUNDLE_DIR/config"
cp config/polis.yaml "$BUNDLE_DIR/config/"

# Create placeholder directories
mkdir -p "$BUNDLE_DIR/certs/ca" "$BUNDLE_DIR/certs/valkey" "$BUNDLE_DIR/certs/toolbox"
mkdir -p "$BUNDLE_DIR/secrets"

# Create tarball
OUTPUT=".build/polis-config.tar.gz"
mkdir -p .build
tar -czf "$OUTPUT" -C "$BUNDLE_DIR" .

echo "==> Bundle created: $OUTPUT ($(du -h "$OUTPUT" | cut -f1))"
