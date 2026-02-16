# Local build of dhi.io/golang:1-dev
# Go 1.24 with nonroot user (UID 65532)
FROM golang:1.24-bookworm

RUN groupadd -g 65532 nonroot \
    && useradd -u 65532 -g 65532 -d /home/nonroot -s /sbin/nologin -M nonroot
