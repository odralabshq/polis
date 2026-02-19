# syntax=docker/dockerfile:1
# Polis Gateway with g3proxy + g3fcgen

# hadolint ignore=DL3007
FROM ghcr.io/odralabshq/g3-builder:latest AS builder

FROM dhi.io/debian-base:trixie-dev@sha256:2166e2eaef0651c9ad21de6ab5a34fda12541d89bccf7bcb0a94afceb1b1541b

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    iproute2 nftables openssl ca-certificates curl libssl3t64 liblua5.4-0 \
    libc-ares2 libcap2 python3 libpython3.13 libunwind8 libnuma1 \
    procps netcat-openbsd util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && (grep -q '^nonroot:' /etc/passwd || echo 'nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin' >> /etc/passwd) \
    && (grep -q '^nonroot:' /etc/group || echo 'nonroot:x:65532:' >> /etc/group)

COPY --from=builder /g3proxy /usr/bin/g3proxy
COPY --from=builder /g3fcgen /usr/bin/g3fcgen

RUN mkdir -p /etc/g3proxy /var/lib/g3proxy/ssl /tmp/g3 /var/log/g3proxy && \
    chown -R 65532:65532 /var/log/g3proxy /var/lib/g3proxy /tmp/g3

RUN g3proxy --version && g3fcgen --version

COPY services/gate/scripts/init.sh /usr/local/bin/init.sh
COPY services/gate/scripts/setup-network.sh /usr/local/bin/setup-network.sh
RUN chmod 755 /usr/local/bin/init.sh /usr/local/bin/setup-network.sh

USER 65532
ENTRYPOINT ["/usr/local/bin/init.sh"]
