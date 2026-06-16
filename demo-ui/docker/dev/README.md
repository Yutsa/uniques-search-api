# Dev container image (demo-ui)

`Dockerfile` here builds a **development** image of this `demo-ui` SPA, used by the
[altered-dev-environment](https://github.com/Altered-Community/altered-dev-environment)
Aspire orchestrator (its `uniques-ui` service points at this Dockerfile).

It runs the Vite **dev server** (HMR): `npm ci` is baked at **build time** (no runtime
install). The orchestrator bind-mounts `demo-ui/` at `/app` for live editing and adds a
`node_modules` volume seeded from the image (so the bind-mount doesn't shadow the baked
deps), then publishes the Vite port. A plain `docker run` (no bind-mount) needs no volume.
The SPA calls the uniques API directly from the browser (the API sets permissive CORS),
so `VITE_API_BASE_URL` points at the browser-reachable API URL — no proxy needed.

(The API's own dev image lives one level up, in [`docker/dev/`](../../../docker/dev).)
