# Task runner for rust-cards-api (https://github.com/casey/just)
#
# List recipes:  just
# Recipe help:    just --list
#

set dotenv-load := true
set dotenv-path := ".env.local"

# Just uses `sh` by default, but on windows we want to use PowerShell as it is more common and easier to use.
# We will need to write custom recipes for Windows if going beyond simple passthrough commands.
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    @just --list


# Build and run the HTTP API (release; loads uniques-http-api/.env.local).
[group('1-run')]
api:
    cargo run -p uniques-http-api --release

# Run the demo UI dev server (Vite; expects API at uniques-http-api/.env.local).
[group('1-run'), unix]
demo-ui:
    cd demo-ui && npm run dev
# Run the demo UI dev server (Vite; expects API at uniques-http-api/.env.local).
[group('1-run'), windows]
demo-ui:
    Set-Location demo-ui; npm run dev

# Passthrough to cli-indexer (release). Example: just cli build --help
[group('1-run')]
cli *ARGS:
    cargo run -p cli-indexer --release -- {{ARGS}}

# Passthrough to cli-indexer (debug, faster iteration). Example: just cli-dev build --help
[group('1-run')]
cli-dev *ARGS:
    cargo run -p cli-indexer -- {{ARGS}}


# Fast compile check for the whole workspace.
[group('2-dev')]
check:
    cargo check --workspace

# Run clippy (linter for code quality)
[group('2-dev')]
clippy:
    cargo clippy --workspace

# Run all workspace tests.
[group('2-dev')]
test:
    cargo test --workspace

# Build release binaries for every workspace crate.
[group('2-dev')]
build:
    cargo build --release --workspace

# Quick idGd query smoke test against the merged index.
[group('3-manual-test')]
query id="24":
    cargo run -p cli-indexer --release -- query --index-dir build/full_index --set ALL_SETS --list 3 --show-effect --id-gd {{id}}

# Quick query by reference ID
[group('3-manual-test')]
get-refid id="ALT_COREKS_B_AX_04_U_10":
    cargo run -p cli-indexer --release -- query --index-dir build/full_index --set ALL_SETS --refid {{id}}

# Try pushing a collection to the API
[group('3-manual-test'), windows]
push-collection id="coll-test-1k" collection="uniques-http-api/tests/fixtures/collection/random-1000.txt":
    curl.exe -v -X POST --data-binary "@{{collection}}" http://localhost:8234/api/v2/collection/{{id}}

# Try pushing a collection to the API
[group('3-manual-test'), unix]
push-collection id="coll-test-1k" collection="uniques-http-api/tests/fixtures/collection/random-1000.txt":
    curl -v -X POST --data-binary "@{{collection}}" http://localhost:8234/api/v2/collection/{{id}}

# Build the index for a single set
[group('4-production')]
create-index set="COREKS" root_dir="../equinox-cards":
    cargo run -p cli-indexer --release -- build --root {{root_dir}}/cards-unique-{{set}} --set {{set}} --out build/sets_index

# Build the index for all sets
[group('4-production')]
create-index-all root_dir="../equinox-cards":
    cargo run -p cli-indexer --release -- build --root {{root_dir}}/cards-unique-COREKS --set COREKS --out build/sets_index
    cargo run -p cli-indexer --release -- build --root {{root_dir}}/cards-unique-CORE --set CORE --out build/sets_index
    cargo run -p cli-indexer --release -- build --root {{root_dir}}/cards-unique-ALIZE --set ALIZE --out build/sets_index
    cargo run -p cli-indexer --release -- build --root {{root_dir}}/cards-unique-BISE --set BISE --out build/sets_index
    cargo run -p cli-indexer --release -- build --root {{root_dir}}/cards-unique-CYCLONE --set CYCLONE --out build/sets_index
    cargo run -p cli-indexer --release -- build --root {{root_dir}}/cards-unique-DUSTER --set DUSTER --out build/sets_index
    cargo run -p cli-indexer --release -- build --root {{root_dir}}/cards-unique-EOLE --set EOLE --out build/sets_index
    cargo run -p cli-indexer --release -- merge --index-dir build/sets_index --sets COREKS,CORE,ALIZE,BISE,CYCLONE,DUSTER,EOLE --out build/full_index/ALL_SETS

# Merge per-set indexes under build/sets_index/ into build/full_index/ALL_SETS.
[group('4-production')]
index-merge sets="COREKS,CORE,ALIZE,BISE,CYCLONE,DUSTER,EOLE":
    cargo run -p cli-indexer --release -- merge --index-dir build/sets_index --sets {{sets}} --out build/full_index/ALL_SETS

# Compress the full index into a single .tar.zst file.
[group('4-production'), unix]
compress-index:
    tar -C build/full_index/ALL_SETS -I "zstd -19" --transform 's,^\./,,' -cvf build/full_index.tar.zst .

# Publish merged index to GCS (requires gcloud ADC + GCS_INDEX_BUCKET in .env.local).
[group('4-production'), unix]
publish-index prefix="index":
    #!/usr/bin/env bash
    set -euo pipefail
    : "${GCS_INDEX_BUCKET:?Set GCS_INDEX_BUCKET in .env.local}"
    if [[ ! -f build/full_index.tar.zst ]]; then
      echo "build/full_index.tar.zst not found; run: just compress-index" >&2
      exit 1
    fi
    archive_object="{{prefix}}/full_index.tar.zst"
    gsutil cp build/full_index.tar.zst "gs://${GCS_INDEX_BUCKET}/${archive_object}"
    version="$(python -c "import json; m=json.load(open('build/full_index/ALL_SETS/manifest.json')); print(json.dumps({'version': m['built_at_secs'], 'archive_object': '${archive_object}'}))")"
    printf '%s\n' "$version" > build/version.json
    gsutil cp build/version.json "gs://${GCS_INDEX_BUCKET}/{{prefix}}/version.json"

# Publish merged index to GCS (requires gcloud ADC + GCS_INDEX_BUCKET in .env.local).
[group('4-production'), windows]
publish-index prefix="index":
    if (-not $env:GCS_INDEX_BUCKET) { throw "Set GCS_INDEX_BUCKET in .env.local" }
    if (-not (Test-Path build/full_index.tar.zst)) { throw "build/full_index.tar.zst not found; run: just compress-index" }
    gsutil cp build/full_index.tar.zst "gs://$($env:GCS_INDEX_BUCKET)/{{prefix}}/full_index.tar.zst"
    python -c "import json; ao='{{prefix}}/full_index.tar.zst'; m=json.load(open('build/full_index/ALL_SETS/manifest.json')); json.dump({'version': m['built_at_secs'], 'archive_object': ao}, open('build/version.json','w'))"
    gsutil cp build/version.json "gs://$($env:GCS_INDEX_BUCKET)/{{prefix}}/version.json"

# Build the Cloud Run Docker image (requires build/full_index/ALL_SETS on disk).
[group('4-production')]
docker:
    docker build -t uniques-http-api .

# Build and push the Cloud Run image to Artifact Registry. Example: just docker-push 0.0.8
[group('4-production'), windows]
docker-push version:
    docker build -t $($env:DOCKER_REGISTRY):v{{version}} -f Dockerfile .
    docker push $($env:DOCKER_REGISTRY):v{{version}}

