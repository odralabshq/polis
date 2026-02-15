#!/bin/bash
set -e

# Create directories in tmpfs mounts with correct ownership
# /var/log and /var/run/c-icap are owned by c-icap (101) via docker-compose tmpfs
mkdir -p /var/log/c-icap /var/run/c-icap

# Clean up stale PID files
rm -f /var/run/c-icap/c-icap.pid /var/run/c-icap/c-icap.ctl

echo "[icap] Starting c-icap with SquidClamav on 0.0.0.0:1344..."
exec /usr/bin/c-icap -N -D -f /etc/c-icap/c-icap.conf
