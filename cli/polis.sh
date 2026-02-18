#!/usr/bin/env bash
# polis.sh — Polis platform lifecycle CLI
# Usage: polis.sh <setup-ca|up|down|status|build>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${PROJECT_ROOT}"

cmd="${1:-}"

case "${cmd}" in
    setup-ca)
        CA_DIR=certs/ca
        CA_KEY="${CA_DIR}/ca.key"
        CA_PEM="${CA_DIR}/ca.pem"
        if [[ -f "${CA_KEY}" && -f "${CA_PEM}" ]]; then
            echo "CA already exists."
            exit 0
        fi
        rm -f "${CA_KEY}" "${CA_PEM}"
        mkdir -p "${CA_DIR}"
        openssl genrsa -out "${CA_KEY}" 4096
        openssl req -new -x509 -days 3650 -key "${CA_KEY}" -out "${CA_PEM}" \
            -subj "/C=US/ST=Local/L=Local/O=Polis/OU=Gateway/CN=Polis CA"
        chmod 644 "${CA_KEY}" "${CA_PEM}"
        ;;
    up)
        docker compose down --remove-orphans 2>/dev/null || true
        sudo systemctl restart sysbox 2>/dev/null || true
        timeout 15 bash -c 'until sudo systemctl is-active sysbox &>/dev/null; do sleep 1; done' || true
        touch .env
        docker compose -f docker-compose.yml --env-file .env --profile test up -d
        docker compose -f docker-compose.yml --env-file .env --profile test ps
        ;;
    down)
        docker compose --profile test down --volumes --remove-orphans
        ;;
    status)
        docker compose -f docker-compose.yml --env-file .env --profile test ps
        ;;
    build)
        touch .env
        docker compose -f docker-compose.yml --env-file .env build
        ;;
    *)
        echo "Usage: polis.sh <setup-ca|up|down|status|build>" >&2
        exit 1
        ;;
esac
