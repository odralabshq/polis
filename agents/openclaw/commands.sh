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
LOCALHOST="localhost"

is_ipv4() {
    local candidate="${1:-}"
    local o1 o2 o3 o4

    [[ "$candidate" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]] || return 1
    IFS=. read -r o1 o2 o3 o4 <<< "$candidate"
    for octet in "$o1" "$o2" "$o3" "$o4"; do
        (( octet >= 0 && octet <= 255 )) || return 1
    done
    return 0
}

case "$SUBCMD" in
    token)
        token=$(docker exec "$CONTAINER" cat /home/polis/.openclaw/gateway-token.txt 2>/dev/null || true)
        if [[ -z "$token" ]]; then
            echo "ERROR: Gateway token not found. OpenClaw may not be initialized yet." >&2
            exit 1
        fi
        vm_ip=$(docker exec "$CONTAINER" printenv POLIS_VM_IP 2>/dev/null || true)
        if [[ -z "$vm_ip" ]] || ! is_ipv4 "$vm_ip"; then
            vm_ip=$(head -n1 /opt/polis/.vm-ip 2>/dev/null || echo "$LOCALHOST")
        fi
        if ! is_ipv4 "$vm_ip"; then
            vm_ip="$LOCALHOST"
        fi
        echo ""
        echo "=== OpenClaw Gateway ==="
        echo ""
        echo "  Token:      $token"
        echo "  Control UI: http://${vm_ip}:18789/overview"
        echo ""
        echo "Paste the token into the Control UI login page to authenticate."
        echo ""
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
        echo ""
        # Restore polis-managed gateway token after onboard (onboard may overwrite config)
        docker exec -u polis "$CONTAINER" bash -c '
            TOKEN_FILE="/home/polis/.openclaw/gateway-token.txt"
            CONFIG_FILE="/home/polis/.openclaw/openclaw.json"
            if [[ -f "$TOKEN_FILE" && -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
                SAVED_TOKEN=$(cat "$TOKEN_FILE")
                CURRENT_TOKEN=$(jq -r ".gateway.auth.token // empty" "$CONFIG_FILE" 2>/dev/null || echo "")
                if [[ -n "$SAVED_TOKEN" && "$CURRENT_TOKEN" != "$SAVED_TOKEN" ]]; then
                    jq --arg token "$SAVED_TOKEN" \
                        ".gateway.auth.mode = \"token\" | .gateway.auth.token = \$token | .gateway.controlUi.enabled = true | .gateway.controlUi.allowInsecureAuth = true | .gateway.controlUi.dangerouslyDisableDeviceAuth = true | .gateway.controlUi.dangerouslyAllowHostHeaderOriginFallback = true" \
                        "$CONFIG_FILE" > "${CONFIG_FILE}.tmp" && mv "${CONFIG_FILE}.tmp" "$CONFIG_FILE"
                    chown polis:polis "$CONFIG_FILE" 2>/dev/null || true
                    chmod 600 "$CONFIG_FILE"
                    echo "[polis] Restored gateway token and Control UI settings after onboard"
                fi
            fi
        ' 2>/dev/null || true
        # Restart the gateway to pick up onboard changes
        log_info "Restarting OpenClaw to apply changes..."
        docker exec "$CONTAINER" bash -c '
            pid=$(systemctl show -p MainPID --value openclaw 2>/dev/null)
            if [[ -n "$pid" && "$pid" != "0" ]]; then
                kill -9 "$pid" 2>/dev/null || true
            fi
            systemctl reset-failed openclaw 2>/dev/null || true
            sleep 2
            systemctl start openclaw
        '
        # Wait for the gateway to become ready (init.sh + gateway startup can
        # take 20-30 seconds). Poll the health endpoint instead of a fixed sleep.
        log_info "Waiting for gateway to become ready..."
        READY=false
        for i in $(seq 1 30); do
            HTTP_CODE=$(docker exec "$CONTAINER" curl -sf -o /dev/null -w '%{http_code}' --connect-timeout 1 http://127.0.0.1:18789/health 2>/dev/null || echo "000")
            if [[ "$HTTP_CODE" == "200" ]]; then
                READY=true
                break
            fi
            sleep 1
        done
        if [[ "$READY" == "true" ]]; then
            log_success "OpenClaw restarted with new configuration"
        else
            log_info "Gateway is still starting — it may take a few more seconds"
        fi
        # Show dashboard URL
        vm_ip=$(docker exec "$CONTAINER" printenv POLIS_VM_IP 2>/dev/null || true)
        if [[ -z "$vm_ip" ]] || ! is_ipv4 "$vm_ip"; then
            vm_ip=$(head -n1 /opt/polis/.vm-ip 2>/dev/null || echo "$LOCALHOST")
        fi
        if ! is_ipv4 "$vm_ip"; then
            vm_ip="$LOCALHOST"
        fi
        token=$(docker exec "$CONTAINER" cat /home/polis/.openclaw/gateway-token.txt 2>/dev/null || true)
        echo ""
        log_success "OpenClaw is ready"
        echo "  Control UI: http://${vm_ip}:18789/overview"
        if [[ -n "$token" ]]; then
            echo "  Token:      ${token}"
        fi
        ;;
    restart)
        log_info "Restarting OpenClaw service..."
        # In sysbox containers, systemctl restart can hang because systemd
        # cannot send SIGTERM to the process.  Instead we kill the gateway
        # process directly and let systemd's Restart=always bring it back.
        docker exec "$CONTAINER" bash -c '
            pid=$(systemctl show -p MainPID --value openclaw 2>/dev/null)
            if [[ -n "$pid" && "$pid" != "0" ]]; then
                kill -9 "$pid" 2>/dev/null || true
            fi
            systemctl reset-failed openclaw 2>/dev/null || true
            sleep 2
            systemctl start openclaw
        '
        # Wait for the gateway to become ready (can take 20-30s)
        log_info "Waiting for gateway..."
        for i in $(seq 1 30); do
            HTTP_CODE=$(docker exec "$CONTAINER" curl -sf -o /dev/null -w '%{http_code}' --connect-timeout 1 http://127.0.0.1:18789/health 2>/dev/null || echo "000")
            if [[ "$HTTP_CODE" == "200" ]]; then
                break
            fi
            sleep 1
        done
        log_success "OpenClaw service restarted"
        ;;
    status)
        docker exec "$CONTAINER" systemctl status openclaw --no-pager || true
        ;;
    cli)
        if [[ $# -eq 0 ]]; then
            echo "Usage: cli <command> [args...]"
            exit 1
        fi
        docker exec -it -u polis -w /app "$CONTAINER" node dist/index.js "$@"
        ;;
    help|*)
        echo "OpenClaw commands: token, devices, onboard, restart, status, cli"
        ;;
esac
