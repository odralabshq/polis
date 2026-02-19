# syntax=docker/dockerfile:1
# Polis Gateway with g3proxy + g3fcgen

# hadolint ignore=DL3007
FROM ghcr.io/odralabshq/g3-builder:latest AS builder

# Install runtime packages into trixie-dev (has apt), copy results into trixie runtime
FROM dhi.io/debian-base:trixie-dev@sha256:2166e2eaef0651c9ad21de6ab5a34fda12541d89bccf7bcb0a94afceb1b1541b AS deps
RUN apt-get update && apt-get install -y --no-install-recommends \
    iproute2 nftables openssl ca-certificates curl libssl3t64 liblua5.4-0 \
    libc-ares2 libcap2 python3 libpython3.13 libunwind8 libnuma1 \
    procps netcat-openbsd util-linux \
    && rm -rf /var/lib/apt/lists/*

FROM dhi.io/debian-base:trixie@sha256:9ef766670af4743904b0f992a26b525c6c914648b56ea597ec23452adcf1a95d

COPY --from=deps /usr /usr
COPY --from=deps /lib /lib
COPY --from=deps /etc/ssl /etc/ssl
COPY --from=deps /etc/alternatives /etc/alternatives

USER 0
RUN (grep -q '^root:' /etc/passwd || echo 'root:x:0:0:root:/root:/bin/sh' >> /etc/passwd) \
    && (grep -q '^root:' /etc/group || echo 'root:x:0:' >> /etc/group) \
    && (grep -q '^nonroot:' /etc/passwd || echo 'nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin' >> /etc/passwd) \
    && (grep -q '^nonroot:' /etc/group || echo 'nonroot:x:65532:' >> /etc/group) \
    && mkdir -p /etc/g3proxy /var/lib/g3proxy/ssl /tmp/g3 /var/log/g3proxy \
    && chown -R 65532:65532 /var/log/g3proxy /var/lib/g3proxy /tmp/g3

COPY --from=builder /g3proxy /usr/bin/g3proxy
COPY --from=builder /g3fcgen /usr/bin/g3fcgen

COPY --chmod=755 services/gate/scripts/init.sh /usr/local/bin/init.sh
COPY --chmod=755 services/gate/scripts/setup-network.sh /usr/local/bin/setup-network.sh

USER 65532
ENTRYPOINT ["/usr/local/bin/init.sh"]
