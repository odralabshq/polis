#!/bin/bash
# ============================================================================
# Polis Guest Query Script
# ============================================================================
# This script runs inside the VM to gather system and security status.
# It outputs consolidated MINIFIED JSON to avoid Multipass Windows buffer issues.
# ============================================================================

set -euo pipefail

COMMAND="${1:-status}"
POLIS_ROOT="/opt/polis"
COMPOSE_FILE="${POLIS_ROOT}/docker-compose.yml"

case "${COMMAND}" in
  status)
    # Gather uptime (avoid useless cat — read directly with awk)
    UPTIME=$(awk '{print $1}' /proc/uptime)

    # Gather container info, keeping only the fields the CLI needs.
    CONTAINERS_JSON=$(docker compose -f "${COMPOSE_FILE}" ps --format json \
      | jq -s '[.[] | {Service: .Service, State: .State, Health: .Health}]')

    printf '{"uptime":%s,"containers":%s}\n' "${UPTIME}" "${CONTAINERS_JSON}"
    ;;

  health)
    SERVICE="${2:-gate}"
    # Return the docker compose JSON array for the requested service so the
    # CLI can inspect the State field directly.
    docker compose -f "${COMPOSE_FILE}" ps --format json "${SERVICE}" \
      | jq -s "{\"${SERVICE}\": .}"
    ;;

  malware-db)
    # Return the mtime of the ClamAV daily database so the CLI can compute age.
    MTIME=0
    for f in /var/lib/clamav/daily.cld /var/lib/clamav/daily.cvd; do
      if [[ -f "${f}" ]]; then
        MTIME=$(stat -c %Y "${f}")
        break
      fi
    done
    printf '{"daily_cvd_mtime":%s}\n' "${MTIME}"
    ;;

  cert-expiry)
    CERT_PATH="${POLIS_ROOT}/certs/ca/ca.pem"
    if [[ -f "${CERT_PATH}" ]]; then
      # Return the raw openssl date string — the CLI already parses this format.
      EXPIRY=$(openssl x509 -enddate -noout -in "${CERT_PATH}" | cut -d= -f2)
    else
      EXPIRY=""
    fi
    printf '{"ca_expiry":"%s"}\n' "${EXPIRY}"
    ;;

  *)
    echo "Unknown command: ${COMMAND}" >&2
    exit 1
    ;;
esac
