# Certgen - g3fcgen certificate generator sidecar
# Isolates CA private key from gate container
# Reuses gate image which already contains g3fcgen binary

FROM polis-gate-oss:latest

USER root

# Create config directory
RUN mkdir -p /etc/g3fcgen && chown 65532:65532 /etc/g3fcgen

USER nonroot

ENTRYPOINT ["g3fcgen", "-c", "/etc/g3fcgen/g3fcgen.yaml"]
