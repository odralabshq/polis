#!/usr/bin/env bash
# Process assertions inside containers.

assert_process_running() {
    local ctr="$1" proc="$2"
    docker exec "$ctr" pgrep -x "$proc" > /dev/null 2>&1 \
        || fail "Expected process '$proc' running in $ctr"
}

assert_process_user() {
    local ctr="$1" proc="$2" expected_user="$3"
    local user
    user=$(docker exec "$ctr" ps -o user= -C "$proc" 2>/dev/null | head -1 | tr -d ' ')
    [[ "$user" == "$expected_user" ]] || fail "Expected $proc running as $expected_user in $ctr, got: $user"
}

assert_pid_valid() {
    local ctr="$1" pidfile="$2"
    local pid
    pid=$(docker exec "$ctr" cat "$pidfile" 2>/dev/null)
    [[ -n "$pid" ]] || fail "PID file $pidfile empty in $ctr"
    docker exec "$ctr" ps -p "$pid" > /dev/null 2>&1 || fail "PID $pid not running in $ctr"
}
