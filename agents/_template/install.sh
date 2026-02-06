#!/bin/bash
# agents/<name>/install.sh
# Build-time install script. Runs inside the container as root.
# Invoked by the generated Dockerfile:
#   COPY agents/<name>/ /tmp/agents/<name>/
#   RUN chmod +x /tmp/agents/<name>/install.sh && /tmp/agents/<name>/install.sh
set -euo pipefail

echo "CHANGEME: implement install steps"
