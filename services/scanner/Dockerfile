FROM clamav/clamav:1.5.1

USER root

# Rename clamav user/group to scanner (keeping UID/GID 100:101)
# we use sed because usermod might not be available or restrictive in alpine/debian depending on base
# clamav image is based on alpine:3.18 (usually)
RUN sed -i 's/^clamav:/scanner:/' /etc/passwd && \
    sed -i 's/^clamav:/scanner:/' /etc/group && \
    sed -i 's/^clamav:/scanner:/' /etc/shadow 2>/dev/null || true

# Update clamd.conf and freshclam.conf to use scanner user
RUN sed -i 's/^User clamav/User scanner/' /etc/clamav/clamd.conf && \
    sed -i 's/^DatabaseOwner clamav/DatabaseOwner scanner/' /etc/clamav/freshclam.conf

# Copy modified init script
COPY services/scanner/scripts/init.sh /init
RUN chmod +x /init

# Update permissions for new user name (UID is same, so files on disk are fine, but verify)
RUN mkdir -p /var/lib/clamav /var/log/clamav /run/clamav && \
    chown -R scanner:scanner /var/lib/clamav /var/log/clamav /run/clamav

USER scanner
