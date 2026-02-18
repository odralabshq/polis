# DHI ClamAV base image - minimal, hardened
# Image runs as non-root user clamav (UID 65532) by default
# hadolint ignore=DL3006
FROM clamav/clamav:1.5

# Remap clamav user to UID 65532 to match expected nonroot UID
USER root
RUN usermod -u 65532 clamav && groupmod -g 65532 clamav \
    && chown -R 65532:65532 /var/lib/clamav /var/log/clamav /run/clamav 2>/dev/null || true

# Patch the upstream entrypoint: replace the chown that fails when running
# as non-root (UID 65532) with a no-op. Volume ownership is handled by
# the scanner-init container before this service starts.
RUN sed -i '18s/.*/chown -R clamav:clamav \/var\/lib\/clamav 2>\/dev\/null || true/' /init

USER clamav
