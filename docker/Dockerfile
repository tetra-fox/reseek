# the build context holds entrypoint.sh and the static binaries as reseek-<arch>, which
# `just docker` puts there
FROM docker.io/library/alpine:3.24.4

ARG TARGETARCH

# bash and GNU wget are for custom scripts, tzdata for TZ, which log timestamps and the daily
# log file follow. the base's CA bundle serves scripts and the daemon, which also carries its own
# TLS roots
RUN apk add --no-cache tini su-exec tzdata bash wget \
  && addgroup -g 1000 reseek \
  && adduser -u 1000 -G reseek -h /app -H -D -s /sbin/nologin reseek

COPY reseek-${TARGETARCH} /usr/bin/reseek
COPY entrypoint.sh /usr/local/bin/entrypoint.sh

# SHELL is what scripts that name no program run in
ENV RESEEK_APP_DIR=/app \
    RESEEK_HTTP_PORT=5030 \
    RESEEK_HTTPS_PORT=5031 \
    RESEEK_SLSK_LISTEN_PORT=50300 \
    RESEEK_UMASK=0022 \
    SHELL=/bin/bash

VOLUME /app
EXPOSE 5030 5031 50300

HEALTHCHECK --interval=60s --timeout=3s --start-period=60m --retries=3 \
  CMD wget -q -O - "http://localhost:${RESEEK_HTTP_PORT}/health" || exit 1

ENTRYPOINT ["tini", "--", "/usr/local/bin/entrypoint.sh"]
