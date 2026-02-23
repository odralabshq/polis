#!/usr/bin/env bash
# install-agents.sh — Unpack pre-generated agent artifacts into VM
set -euo pipefail

AGENTS_TAR="/tmp/polis-agents.tar.gz"

echo "==> Installing Polis agents..."

if [[ -f "${AGENTS_TAR}" ]]; then
    cd /opt/polis
    tar -xzf "${AGENTS_TAR}"
    rm -f "${AGENTS_TAR}"
    echo "==> Agents installed:"
    for d in agents/*/; do
        [ -d "$d" ] || continue
        name=$(basename "$d")
        echo "    - ${name}"
    done
else
    echo "==> No agents tarball found at ${AGENTS_TAR}, skipping"
fi
