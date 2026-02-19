FROM dhi.io/golang:1-dev@sha256:d241d31631f6bcfdef4f42f70083bb06d51e6401d9ce172a05b9ba37f27c2940 AS builder

ARG COREDNS_VERSION=1.14.1

# Install curl and gzip for downloading CoreDNS source
RUN apt-get update && apt-get install -y --no-install-recommends curl gzip \
    && rm -rf /var/lib/apt/lists/*

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
RUN curl -fsSL "https://github.com/coredns/coredns/archive/refs/tags/v${COREDNS_VERSION}.tar.gz" \
    | tar -xz -C /tmp && mv "/tmp/coredns-${COREDNS_VERSION}" /src

WORKDIR /src

# Inject blocklist plugin before forward (order matters in CoreDNS)
RUN sed -i '/^forward:forward/i blocklist:github.com/relekang/coredns-blocklist' plugin.cfg \
    && go generate && go get github.com/relekang/coredns-blocklist \
    && CGO_ENABLED=0 go build -o /coredns

FROM dhi.io/debian-base:trixie@sha256:9ef766670af4743904b0f992a26b525c6c914648b56ea597ec23452adcf1a95d

# DHI debian-base includes ca-certificates; no apt-get available in minimal image

COPY --from=builder /coredns /usr/local/bin/coredns

EXPOSE 53/udp 53/tcp

# DHI images run as nonroot (UID 65532) by default
USER nonroot
ENTRYPOINT ["/usr/local/bin/coredns"]
CMD ["-conf", "/etc/coredns/Corefile"]
