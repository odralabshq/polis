#!/usr/bin/env bash
# polis.sh — thin wrapper around just
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/.."
exec just "$@"
