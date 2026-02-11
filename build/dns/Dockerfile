FROM debian:bookworm-slim

ARG COREDNS_BLOCKLIST_VERSION=1.13.3

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl unzip dnsutils \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "https://github.com/relekang/coredns-blocklist/releases/download/v${COREDNS_BLOCKLIST_VERSION}/coredns-with-blocklist-linux-amd64.zip" \
    -o /tmp/coredns.zip && unzip /tmp/coredns.zip -d /tmp && mv /tmp/coredns /usr/local/bin/coredns \
    && chmod +x /usr/local/bin/coredns && rm /tmp/coredns.zip

EXPOSE 53/udp 53/tcp

ENTRYPOINT ["/usr/local/bin/coredns"]
CMD ["-conf", "/etc/coredns/Corefile"]
