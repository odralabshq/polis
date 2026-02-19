# Host init container — configures host iptables for TPROXY
# checkov:skip=CKV_DOCKER_2:Init container runs once and exits, HEALTHCHECK not applicable
# checkov:skip=CKV_DOCKER_3:Init container requires root for iptables configuration
FROM dhi.io/alpine-base:3.23-dev@sha256:2b318097756a2d92fb8fc059beaf512f8607224a17767d56e557618ca61296cb
RUN apk add --no-cache docker-cli iptables
