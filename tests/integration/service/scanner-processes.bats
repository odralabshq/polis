#!/usr/bin/env bats
# bats file_tags=integration,service
# Integration tests for scanner service — ClamAV process, port, signatures

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_SCANNER"
}

# ── Port (source: services/scanner/config/clamd.conf TCPSocket 3310) ─────

@test "scanner: ClamAV listening on 3310" {
    run docker exec "$CTR_SCANNER" cat /proc/net/tcp
    assert_success
    # 3310 decimal = 0CEE hex
    assert_output --partial "0CEE"
}

@test "scanner: responds to PING" {
    # DHI ClamAV image doesn't have nc; use clamdscan --ping instead
    run docker exec "$CTR_SCANNER" clamdscan --ping 3
    assert_success
}

@test "scanner: returns version info" {
    # DHI ClamAV image doesn't have nc; use clamdscan --version instead
    run docker exec "$CTR_SCANNER" clamdscan --version
    assert_success
    assert_output --partial "ClamAV"
}

# ── Signature database (source: /var/lib/clamav volume) ───────────────────

@test "scanner: main signature database loaded" {
    run docker exec "$CTR_SCANNER" sh -c 'test -f /var/lib/clamav/main.cvd || test -f /var/lib/clamav/main.cld'
    assert_success
}

@test "scanner: daily signatures loaded" {
    run docker exec "$CTR_SCANNER" sh -c 'test -f /var/lib/clamav/daily.cvd || test -f /var/lib/clamav/daily.cld'
    assert_success
}

# ── Config (source: docker-compose.yml mounts) ───────────────────────────

@test "scanner: freshclam.conf mounted" {
    run docker exec "$CTR_SCANNER" test -f /etc/clamav/freshclam.conf
    assert_success
}

@test "scanner: freshclam configured for updates" {
    run docker exec "$CTR_SCANNER" grep DatabaseMirror /etc/clamav/freshclam.conf
    assert_success
}

# ── Volume (source: docker-compose.yml scanner-db:/var/lib/clamav) ────────

@test "scanner: database volume mounted" {
    run docker inspect --format '{{range .Mounts}}{{if eq .Destination "/var/lib/clamav"}}{{.Name}}{{end}}{{end}}' "$CTR_SCANNER"
    assert_success
    assert_output --partial "polis-scanner-db"
}
