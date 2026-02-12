#!/bin/bash
# agents/<name>/install.sh
# Runtime install script. Runs inside the container as root on first boot
# via systemd ExecStartPre=+/tmp/agents/<name>/install.sh
set -euo pipefail

MARKER="/var/lib/<name>-installed"
if [[ -f "$MARKER" ]]; then
    echo "[<name>-install] Already installed, skipping."
    exit 0
fi

echo "[<name>-install] First boot — installing..."

# CHANGEME: implement install steps

touch "$MARKER"
echo "[<name>-install] Installation complete."
