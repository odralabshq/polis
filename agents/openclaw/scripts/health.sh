#!/bin/bash
# OpenClaw health check — used by systemd watchdog
# Returns 0 if the gateway is responding, 1 otherwise
exec curl -sf http://127.0.0.1:18789/health
