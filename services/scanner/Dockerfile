# DHI ClamAV base image - minimal, hardened
# Image runs as non-root user clamav (UID 65532) by default
# hadolint ignore=DL3006
FROM dhi.io/clamav:1.5@sha256:242fa8807f3efe2ae7a6cc3dda53a435833490fc02b91664c9fe7fdd881d96d1

# No modifications needed - DHI image is production-ready
# Configs mounted at runtime via docker-compose.yml
# Use numeric UID so metadata matches structure test expectation
USER 65532
