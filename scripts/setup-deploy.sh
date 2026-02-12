#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
ln -sf ../.env deploy/.env
echo "✓ deploy/.env symlink created"
