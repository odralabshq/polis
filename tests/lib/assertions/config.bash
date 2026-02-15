#!/usr/bin/env bash
# Configuration file assertions.

assert_yaml_key() {
    local file="$1" key="$2"
    grep -q "$key" "$file" || fail "Expected key '$key' in $file"
}

assert_conf_value() {
    local file="$1" pattern="$2"
    grep -q "$pattern" "$file" || fail "Expected pattern '$pattern' in $file"
}

assert_file_mounted_ro() {
    local ctr="$1" mount_path="$2"
    local mounts
    mounts=$(docker inspect --format '{{json .Mounts}}' "$ctr" 2>/dev/null)
    echo "$mounts" | jq -e ".[] | select(.Destination == \"$mount_path\") | select(.RW == false)" > /dev/null 2>&1 \
        || fail "Expected $mount_path mounted read-only in $ctr"
}
