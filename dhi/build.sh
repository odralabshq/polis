#!/usr/bin/env bash
# Build local DHI (Docker Hub Images) base images
# Tags them as dhi.io/... so service Dockerfiles work without the private registry
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

declare -A IMAGES=(
    ["dhi.io/debian-base:trixie"]="debian-base-trixie.Dockerfile"
    ["dhi.io/debian-base:trixie-dev"]="debian-base-trixie-dev.Dockerfile"
    ["dhi.io/rust:1-dev"]="rust-1-dev.Dockerfile"
    ["dhi.io/golang:1-dev"]="golang-1-dev.Dockerfile"
    ["dhi.io/alpine-base:3.23-dev"]="alpine-base-3.23-dev.Dockerfile"
    ["dhi.io/valkey:8.1"]="valkey-8.1.Dockerfile"
    ["dhi.io/clamav:1.5"]="clamav-1.5.Dockerfile"
)

echo "=== Building DHI base images locally ==="

for tag in "${!IMAGES[@]}"; do
    dockerfile="${IMAGES[$tag]}"
    echo ""
    echo "--- Building ${tag} from ${dockerfile} ---"
    docker build -t "${tag}" -f "${SCRIPT_DIR}/${dockerfile}" "${SCRIPT_DIR}" 2>&1
    echo "  ✓ ${tag}"
done

echo ""
echo "=== All DHI base images built ==="
docker images --filter "reference=dhi.io/*" --format "table {{.Repository}}:{{.Tag}}\t{{.Size}}"
