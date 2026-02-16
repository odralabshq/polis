# Local build of dhi.io/clamav:1.5
# ClamAV with nonroot user (UID 65532)
# Runs as nonroot — ClamAV dirs pre-owned by 65532
FROM clamav/clamav:1.4

USER root

# Create nonroot user and fix ownership so ClamAV runs as UID 65532
RUN addgroup -g 65532 nonroot \
    && adduser -u 65532 -G nonroot -D -h /home/nonroot -s /sbin/nologin nonroot \
    && mkdir -p /var/lib/clamav /var/log/clamav /run/clamav /tmp \
    && chown -R 65532:65532 /var/lib/clamav /var/log/clamav /run/clamav /tmp

# Patch ClamAV configs to use nonroot UID
RUN sed -i 's/^User .*/User nonroot/' /etc/clamav/clamd.conf 2>/dev/null || true \
    && sed -i 's/^DatabaseOwner .*/DatabaseOwner nonroot/' /etc/clamav/freshclam.conf 2>/dev/null || true

# Custom entrypoint that skips chown (dirs pre-owned by scanner-init + tmpfs)
COPY clamav-entrypoint.sh /init-nonroot
RUN chmod +x /init-nonroot

USER nonroot
ENTRYPOINT ["/init-nonroot"]
