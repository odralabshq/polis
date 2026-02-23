#!/usr/bin/env bash
# Container lifecycle assertions.

assert_container_running() {
    local ctr="$1"
    local state
    state=$(docker inspect --format '{{.State.Status}}' "$ctr" 2>/dev/null)
    [[ "$state" == "running" ]] || fail "Expected $ctr running, got: $state"
}

assert_container_healthy() {
    local ctr="$1"
    local health
    health=$(docker inspect --format '{{.State.Health.Status}}' "$ctr" 2>/dev/null)
    [[ "$health" == "healthy" ]] || fail "Expected $ctr healthy, got: $health"
}

assert_container_exited_ok() {
    local ctr="$1"
    local code
    code=$(docker inspect --format '{{.State.ExitCode}}' "$ctr" 2>/dev/null)
    [[ "$code" == "0" ]] || fail "Expected $ctr exit 0, got: $code"
}

assert_container_image() {
    local ctr="$1" expected="$2"
    local image
    image=$(docker inspect --format '{{.Config.Image}}' "$ctr" 2>/dev/null)
    [[ "$image" == *"$expected"* ]] || fail "Expected $ctr image containing '$expected', got: $image"
}
