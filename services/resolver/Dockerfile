FROM dhi.io/golang:1-dev@sha256:d241d31631f6bcfdef4f42f70083bb06d51e6401d9ce172a05b9ba37f27c2940 AS builder
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

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

# Static distroless image: CA certs + tzdata only, no glibc/shell/package manager
# Eliminates ~95% of CVEs from debian-base while CoreDNS is CGO_ENABLED=0 static binary
FROM dhi.io/static@sha256:22b2b8a083432d7a8ccc097a65b433595e0be7c00b66b71baedb50bd3b9a24c0

COPY --from=builder /coredns /usr/bin/coredns

EXPOSE 53/udp 53/tcp

# DHI static runs as nonroot (UID 65532) by default
USER 65532
ENTRYPOINT ["/usr/bin/coredns"]
CMD ["-conf", "/etc/coredns/Corefile"]
