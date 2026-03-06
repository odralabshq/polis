#!/bin/bash
# agents/openclaw/commands.sh
# Agent-specific commands delegated by dispatch_agent_command.
# Called as: bash commands.sh <container> <subcommand> [args...]
set -euo pipefail

CONTAINER="${1:?container name required}"
SUBCMD="${2:-help}"
shift 2 || true

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'
log_info() { echo -e "${CYAN}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[OK]${NC} $*"; }
log_step() { echo -e "${CYAN}[STEP]${NC} $*"; }
is_ipv4() {
    local candidate="${1:-}"
    local o1 o2 o3 o4
    local IFS=.

    [[ "$candidate" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]] || return 1
    read -r o1 o2 o3 o4 <<< "$candidate"
    for octet in "$o1" "$o2" "$o3" "$o4"; do
        (( octet >= 0 && octet <= 255 )) || return 1
    done
}

case "$SUBCMD" in
    token)
        token=$(docker exec "$CONTAINER" cat /home/polis/.openclaw/gateway-token.txt 2>/dev/null || true)
        if [[ -z "$token" ]]; then
            echo "ERROR: Gateway token not found. OpenClaw may not be initialized yet." >&2
            exit 1
        fi
        echo "=== OpenClaw Gateway Token ==="
        echo ""
        echo "Token: $token"
        echo ""
        # Use VM IP if available (set by polis CLI during start)
        vm_ip=$(docker exec "$CONTAINER" printenv POLIS_VM_IP 2>/dev/null || head -n1 /opt/polis/.vm-ip 2>/dev/null || echo "localhost")
        if ! is_ipv4 "$vm_ip"; then
            vm_ip="localhost"
        fi
        echo "Control UI: http://${vm_ip}:18789/overview"
        ;;
    devices)
        action="${1:-list}"
        shift || true
        case "$action" in
            list)
                echo "=== OpenClaw Devices ==="
                docker exec -u polis -w /app "$CONTAINER" node dist/index.js devices list
                ;;
            approve)
                request_id="${1:-}"
                if [[ -z "$request_id" ]]; then
                    pending=$(docker exec -u polis -w /app "$CONTAINER" node dist/index.js devices list 2>/dev/null \
                        | grep -A100 "^Pending" | grep "│" | awk -F'│' '{print $2}' | tr -d ' ' | grep -v "^$" | grep -v "Request")
                    if [[ -z "$pending" ]]; then
                        echo "No pending device requests."
                        exit 0
                    fi
                    for req_id in $pending; do
                        [[ -n "$req_id" && "$req_id" != "Request" ]] || continue
                        echo "Approving: $req_id"
                        docker exec -u polis -w /app "$CONTAINER" node dist/index.js devices approve "$req_id" 2>/dev/null || true
                    done
                else
                    docker exec -u polis -w /app "$CONTAINER" node dist/index.js devices approve "$request_id"
                fi
                ;;
            *)
                echo "Usage: devices [list|approve [request_id]]"
                exit 1
                ;;
        esac
        ;;
    onboard)
        docker exec -it -u polis -w /app "$CONTAINER" node dist/index.js onboard
        ;;
    cli)
        if [[ $# -eq 0 ]]; then
            echo "Usage: cli <command> [args...]"
            exit 1
        fi
        docker exec -it -u polis -w /app "$CONTAINER" node dist/index.js "$@"
        ;;
    help|*)
        echo "OpenClaw commands: token, devices, onboard, cli"
        ;;
esac
