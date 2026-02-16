# Local build of dhi.io/debian-base:trixie-dev
# Full Debian trixie with build tools and nonroot user (UID 65532)
FROM debian:trixie

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 build-essential curl wget git \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 65532 nonroot \
    && useradd -u 65532 -g 65532 -d /home/nonroot -s /sbin/nologin -M nonroot
