FROM dhi.io/golang:1-dev AS builder

ARG COREDNS_VERSION=1.14.1

# Install curl and gzip for downloading CoreDNS source
RUN apt-get update && apt-get install -y --no-install-recommends curl gzip \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "https://github.com/coredns/coredns/archive/refs/tags/v${COREDNS_VERSION}.tar.gz" \
    | tar -xz -C /tmp && mv "/tmp/coredns-${COREDNS_VERSION}" /src

WORKDIR /src

# Inject blocklist plugin before forward (order matters in CoreDNS)
RUN sed -i '/^forward:forward/i blocklist:github.com/relekang/coredns-blocklist' plugin.cfg \
    && go generate && go get github.com/relekang/coredns-blocklist

# Patch otel SDK to v1.40.0+ (fixes SNYK-GOLANG-GOOPENTELEMETRYIOOTELSDKRESOURCE Untrusted Search Path)
RUN go get go.opentelemetry.io/otel/sdk@v1.40.0 \
    && go get go.opentelemetry.io/otel/sdk/resource@v1.40.0 \
    && go mod tidy

RUN CGO_ENABLED=0 go build -o /coredns

FROM dhi.io/debian-base:trixie

# DHI debian-base includes ca-certificates; no apt-get available in minimal image

COPY --from=builder /coredns /usr/local/bin/coredns

EXPOSE 53/udp 53/tcp

# DHI images run as nonroot (UID 65532) by default
USER nonroot
ENTRYPOINT ["/usr/local/bin/coredns"]
CMD ["-conf", "/etc/coredns/Corefile"]
