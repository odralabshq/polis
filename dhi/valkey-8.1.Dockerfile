# Local build of dhi.io/valkey:8.1
# Valkey 8 with nonroot user (UID 65532)
FROM valkey/valkey:8-alpine

# valkey image runs as UID 999 by default; add nonroot user for DHI compat
RUN addgroup -g 65532 nonroot \
    && adduser -u 65532 -G nonroot -D -h /home/nonroot -s /sbin/nologin nonroot \
    && chown -R 65532:65532 /data
