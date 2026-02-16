#!/sbin/tini /bin/sh
# Custom ClamAV entrypoint for DHI nonroot (UID 65532)
# Skips chown operations — dirs pre-owned by scanner-init and tmpfs mounts
set -eu

# run command if it is not starting with a "-" and is an executable in PATH
if [ "${#}" -gt 0 ] && \
   [ "${1#-}" = "${1}" ] && \
   command -v "${1}" > "/dev/null" 2>&1; then
        CLAMAV_NO_CLAMD="true" exec "${@}"
else
        if [ "${#}" -ge 1 ] && \
           [ "${1#-}" != "${1}" ]; then
                exec clamd "${@}"
        fi

        # Help tiny-init a little
        mkdir -p "/run/lock" 2>/dev/null || true
        ln -f -s "/run/lock" "/var/lock" 2>/dev/null || true

        # Ensure we have some virus data, otherwise clamd refuses to start
        if [ ! -f "/var/lib/clamav/main.cvd" ]; then
                echo "Updating initial database"
                sed -e 's|^\(TestDatabases \)|\#\1|' \
                        -e '$a TestDatabases no' \
                        -e 's|^\(NotifyClamd \)|\#\1|' \
                        /etc/clamav/freshclam.conf > /tmp/freshclam_initial.conf
                freshclam --foreground --stdout --config-file=/tmp/freshclam_initial.conf
                rm /tmp/freshclam_initial.conf
        fi

        if [ "${CLAMAV_NO_FRESHCLAMD:-false}" != "true" ]; then
                echo "Starting Freshclamd"
                freshclam \
                          --checks="${FRESHCLAM_CHECKS:-1}" \
                          --daemon \
                          --foreground \
                          --stdout \
                          &
        fi

        if [ "${CLAMAV_NO_CLAMD:-false}" != "true" ]; then
                echo "Starting ClamAV"
                if [ -S "/run/clamav/clamd.sock" ]; then
                        unlink "/run/clamav/clamd.sock"
                fi
                clamd --foreground &
        fi

        if [ "${CLAMAV_NO_MILTERD:-true}" != "true" ]; then
                echo "Starting ClamAV Milter"
                clamav-milter &
        fi

        # Wait for any process to exit
        wait
fi
