# The reseek image: the static binary for the platform being built, run by tini through an
# entrypoint that readies /app and drops to the reseek user when PUID and PGID are given.
#
# The build context holds the entrypoint and the static binaries as reseek-<arch>
# (`just docker` puts them there):
#   docker build -f docker/Dockerfile <context>
FROM docker.io/library/debian:trixie-slim

ARG TARGETARCH

RUN apt-get update \
  && apt-get install -y --no-install-recommends tini gosu wget ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && groupadd -g 1000 reseek \
  && useradd -u 1000 -g reseek -d /app -M -s /usr/sbin/nologin reseek

COPY reseek-${TARGETARCH} /usr/bin/reseek
COPY entrypoint.sh /usr/local/bin/entrypoint.sh

ENV RESEEK_APP_DIR=/app \
    RESEEK_HTTP_PORT=5030 \
    RESEEK_HTTPS_PORT=5031 \
    RESEEK_SLSK_LISTEN_PORT=50300 \
    RESEEK_UMASK=0022 \
    SHELL=/usr/bin/bash

VOLUME /app
EXPOSE 5030 5031 50300

HEALTHCHECK --interval=60s --timeout=3s --start-period=60m --retries=3 \
  CMD wget -q -O - "http://localhost:${RESEEK_HTTP_PORT}/health" || exit 1

ENTRYPOINT ["tini", "--", "/usr/local/bin/entrypoint.sh"]
