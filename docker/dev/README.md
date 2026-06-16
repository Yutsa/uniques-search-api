# Dev container image

`Dockerfile` here builds a **development** image of `uniques-http-api`, used by the
[altered-dev-environment](https://github.com/Altered-Community/altered-dev-environment)
Aspire orchestrator (its `uniques` service points at this Dockerfile).

It is intentionally separate from the **prod** `Dockerfile` at the repo root (the Cloud
Run multi-stage build that bakes the binary). This dev image instead expects the
workspace source bind-mounted at `/app` and runs `cargo run` (so source edits are live),
and its entrypoint downloads the prebuilt card index into `/app/build` on first start
(the server loads it from disk via `index.source = "disk"` and doesn't fetch it itself).

The orchestrator builds it with `docker/dev/` as the build context, bind-mounts the repo
at `/app`, keeps `target/` and `build/` in volumes, and exposes the server (default port
`8080`).
