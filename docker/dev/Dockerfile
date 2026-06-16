# syntax=docker/dockerfile:1

# DEV image for the uniques-http-api server, used by the altered-dev-environment
# Aspire orchestrator (its `uniques` service points at this Dockerfile).
#
# It is intentionally SEPARATE from the prod Dockerfile at the repo root (the
# multi-stage Cloud Run build that bakes the binary). This dev image expects the
# workspace source bind-mounted at /app and runs `cargo run`, so source edits are
# picked up without an image rebuild; its entrypoint downloads the prebuilt card
# index into /app/build on first start (the server loads it from disk and won't
# fetch it itself).
#
# The build context is THIS folder (docker/dev/), so it ships no app source — the
# orchestrator bind-mounts the repo at /app and keeps target/ + build/ in volumes.
# rust:1.86 matches the toolchain the root Dockerfile pins (workspace is edition 2024).

FROM rust:1.86-bookworm

# curl + ca-certificates: download the prebuilt index in the entrypoint
# (the bookworm base is minimal on tools).
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --link docker-entrypoint.sh /usr/local/bin/docker-entrypoint
# Windows checkouts often land CRLF; a CRLF shebang breaks exec on Linux. Strip CR and
# set the exec bit. (The repo .gitattributes `*.sh eol=lf` is the primary defense; this
# is belt-and-suspenders so the build is robust regardless of how the file arrived.)
RUN sed -i 's/\r$//' /usr/local/bin/docker-entrypoint \
 && chmod 755 /usr/local/bin/docker-entrypoint

ENTRYPOINT ["docker-entrypoint"]

# `just api` equivalent. cargo checks freshness, so after the first (slow) build the
# bind-mounted target/ volume makes later starts near-instant.
CMD ["cargo", "run", "-p", "uniques-http-api", "--release"]
