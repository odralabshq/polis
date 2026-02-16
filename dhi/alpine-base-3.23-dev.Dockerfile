# Local build of dhi.io/alpine-base:3.23-dev
# Alpine with nonroot user (UID 65532)
FROM alpine:3.21

RUN addgroup -g 65532 nonroot \
    && adduser -u 65532 -G nonroot -D -h /home/nonroot -s /sbin/nologin nonroot
