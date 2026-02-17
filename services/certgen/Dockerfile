# syntax=docker/dockerfile:1
# Certgen - g3fcgen certificate generator sidecar
# Isolates CA private key from gate container

FROM ghcr.io/odralabshq/g3-builder:latest AS builder

FROM dhi.io/debian-base:trixie@sha256:9ef766670af4743904b0f992a26b525c6c914648b56ea597ec23452adcf1a95d

COPY --from=builder /g3fcgen /usr/local/bin/

USER nonroot
ENTRYPOINT ["g3fcgen", "-c", "/etc/g3fcgen/g3fcgen.yaml"]
