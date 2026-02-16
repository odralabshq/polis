FROM golang:1.24-bookworm AS builder

ARG COREDNS_VERSION=1.14.1

# Install curl and gzip for downloading CoreDNS source
RUN apt-get update && apt-get install -y --no-install-recommends curl gzip \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "https://github.com/coredns/coredns/archive/refs/tags/v${COREDNS_VERSION}.tar.gz" \
    | tar -xz -C /tmp && mv "/tmp/coredns-${COREDNS_VERSION}" /src

WORKDIR /src

# Inject blocklist plugin before forward (order matters in CoreDNS)
RUN sed -i '/^forward:forward/i blocklist:github.com/relekang/coredns-blocklist' plugin.cfg \
    && go generate && go get github.com/relekang/coredns-blocklist \
    && CGO_ENABLED=0 go build -o /coredns

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && echo 'resolver:x:200:200:resolver:/home/resolver:/sbin/nologin' >> /etc/passwd \
    && echo 'resolver:x:200:' >> /etc/group

COPY --from=builder /coredns /usr/local/bin/coredns

EXPOSE 53/udp 53/tcp

USER resolver
ENTRYPOINT ["/usr/local/bin/coredns"]
CMD ["-conf", "/etc/coredns/Corefile"]
