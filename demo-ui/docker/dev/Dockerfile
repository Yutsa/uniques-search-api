# syntax=docker/dockerfile:1

# DEV image for this demo-ui (a Vite 6 + React 19 SPA), used by the
# altered-dev-environment Aspire orchestrator (its `uniques-ui` service points at this
# Dockerfile). Build context is the demo-ui/ root.
#
# Deps are installed at BUILD time (npm ci, layer-cached — only re-runs when the
# lockfile changes), so there's no runtime install and no entrypoint. The source is
# also copied in, so a plain `docker run` (no bind-mount) works standalone.
#
# For live editing / HMR, the orchestrator bind-mounts the demo-ui/ source over /app
# and adds a node_modules volume: the bind-mount would otherwise shadow the baked
# node_modules, so the volume (seeded from this image's node_modules) keeps the deps
# visible. A `docker run` without a source mount needs no volume.
#
# The SPA calls the uniques API straight from the browser (the API sets permissive
# CORS), so VITE_API_BASE_URL points at the browser-reachable API URL — no proxy / CORS
# work needed. Vite reads VITE_-prefixed vars from the environment.

FROM node:22-bookworm-slim

WORKDIR /app

# Deps first (own layer) so npm ci is only re-run when package*.json changes.
COPY package.json package-lock.json ./
RUN npm ci

# App source (overlaid by the bind-mount when run under the orchestrator).
COPY . .

# Run Vite on the SAME port we publish (8004) so the HMR websocket — which the browser
# opens to the page's own host:port — lines up. --host binds 0.0.0.0; --strictPort
# fails loudly rather than drifting to another port.
CMD ["npm", "run", "dev", "--", "--host", "0.0.0.0", "--port", "8004", "--strictPort"]
