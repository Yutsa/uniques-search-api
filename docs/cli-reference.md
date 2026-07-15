# CLI reference

This page lists every command-line interface exposed by this repository.

| Tool | How to run | Interface |
|------|------------|-----------|
| **`cli-indexer`** | `cargo run -p cli-indexer -- …` (from repo root) | Subcommands + flags (clap) |
| **`uniques-http-api`** | `cargo run -p uniques-http-api` (from repo root) | Environment variables only (no subcommands) |

The repository root [`Cargo.toml`](../Cargo.toml) is a Cargo workspace; all members share one `Cargo.lock` and build into `target/` at the root.

For narrative context, see [Architecture overview](architecture.md).

---

## `cli-indexer`

Binary crate in [`cli-indexer/`](../cli-indexer/). Indexes Equinox-style Unique card JSON into Roaring bitmaps, `cards.bin`, and related metadata.

```bash
cargo run -p cli-indexer -- --help
cargo run -p cli-indexer -- <SUBCOMMAND> --help
```

Global options: `-h`, `--help` only (no global flags).

### Typical workflow

```bash
# 1. Build one set index
cargo run -p cli-indexer -- build \
  --root /path/to/equinox-cards/cards-unique-COREKS \
  --set COREKS \
  --out ./build/sets_index

# 2. Repeat for other sets, then merge
cargo run -p cli-indexer -- merge \
  --index-dir ./build/sets_index \
  --sets COREKS,CORE,ALIZE \
  --out ./build/full_index/ALL_SETS

# 3. Inspect
cargo run -p cli-indexer -- query \
  --index-dir ./build/full_index \
  --set ALL_SETS \
  --id-gd 24,191 \
  --list 10
```

---

### `build`

Crawl a dataset directory and write a per-set index under `<out>/<SET>/`.

| Option | Required | Description |
|--------|----------|-------------|
| `--root <PATH>` | yes | Dataset root containing `json/<SET>/...` card files |
| `--set <CODE>` | yes | Set code (e.g. `COREKS`, `CORE`, `ALIZE`, `BISE`) |
| `--out <PATH>` | yes | Output directory; files go in `<out>/<SET>/` |
| `--limit <N>` | no | Stop after indexing **N** files (testing / partial builds) |
| `--profile` | no | Print phase timings (read, parse, process, write) |
| `--merge-duplicated-abilities` | no | Collapse idGd entries that share the same element type and effect text (default: `true`) |

**Environment**

| Variable | Effect |
|----------|--------|
| `CLI_INDEXER_PROFILE=1` or `true` | Same as `--profile` (case-insensitive) |

**Example**

```bash
cargo run -p cli-indexer -- build \
  --root "../equinox-cards/cards-unique-COREKS" \
  --set COREKS \
  --out ./build/sets_index \
  --profile
```

On success, prints a one-line summary: output path, file count, family count, idGd bitmap count, and `total_bit_span`.

---

### `decode`

Map a global **`card_index`** (bit position) back to a card reference using `catalog.json`.

| Option | Required | Description |
|--------|----------|-------------|
| `--catalog <PATH>` | yes | Path to `catalog.json` (per-set or merged) |
| `--bit <N>` | yes | `card_index` to decode (`u32`) |

**Example**

```bash
cargo run -p cli-indexer -- decode \
  --catalog ./build/full_index/ALL_SETS/catalog.json \
  --bit 42
```

Prints the reference string and `familyId` / `uniqueID`.

---

### `query`

Query an existing index directory. You must pass **either** `--id-gd` **or** `--refid` (mutually exclusive).

| Option | Required | Description |
|--------|----------|-------------|
| `--index-dir <PATH>` | yes | Parent of set folders (e.g. `./build/sets_index` for per-set indexes, `./build/full_index` for merged `ALL_SETS`) |
| `--set <NAME>` | yes | Set or merged folder name (e.g. `COREKS`, `ALL_SETS`) |
| `--id-gd <IDS>` | one of | Comma-separated idGd values (e.g. `24,191,76`) |
| `--refid <REF>` | one of | Single card reference (e.g. `ALT_COREKS_B_AX_04_U_10`) |
| `--list <N>` | no | Decode and print up to **N** matches (idGd mode only) |
| `--show-effect` | no | Print translated effect text instead of a stats table |
| `--locale <LOCALE>` | no | Locale for effect text (default: `en_US`) |
| `--whole-card` | no | Use whole-card bitmaps (`id_gd/<id>.roar`) instead of per-line (`_m1`, `_m2`, `_m3`, `_ec`) |

**`--id-gd` semantics**

1. Each id is looked up in `idgd_catalog.json` and classified as **TRIGGER**, **CONDITION**, or **OUTPUT**.
2. Bitmaps for ids in the same bucket are **unioned**.
3. The final result is **(trigger union) ∩ (condition union) ∩ (output union)**; empty buckets are skipped.
4. Default mode intersects across **per-line** sub-indexes (main lines M1–M3 and echo); `--whole-card` uses combined per-id bitmaps instead.

**`--refid`**

- Looks up one card and prints effect-oriented output (implies `--show-effect`-style layout).
- Cannot be combined with `--id-gd`, `--list`, `--show-effect`, or `--whole-card`.

**Examples**

```bash
# Count cards matching idGd 24 (table output if --list given)
cargo run -p cli-indexer -- query \
  --index-dir ./build/full_index --set ALL_SETS --id-gd 24

# Multi-idGd query with effect text
cargo run -p cli-indexer -- query \
  --index-dir ./build/full_index --set ALL_SETS \
  --id-gd 24,191,76 --show-effect --list 5 --locale fr_FR

# Single card by reference
cargo run -p cli-indexer -- query \
  --index-dir ./build/full_index --set ALL_SETS \
  --refid ALT_COREKS_B_AX_04_U_10
```

---

### `merge`

Merge two or more **existing** per-set indexes into one global index. Output files are written **directly** under `--out` (not `<out>/<SET>/`).

| Option | Required | Description |
|--------|----------|-------------|
| `--index-dir <PATH>` | yes | Directory containing `<SET>/catalog.json` for each source set |
| `--sets <LIST>` | yes | Comma-separated set codes in **precedence order** (overlap grouping and tie-breaking) |
| `--out <PATH>` | yes | Output folder for the merged index (e.g. `./build/full_index/ALL_SETS`) |
| `--merge-duplicated-abilities` | no | Collapse idGd entries that share the same element type and effect text (default: `true`) |

**Example**

```bash
cargo run -p cli-indexer -- merge \
  --index-dir ./build/sets_index \
  --sets COREKS,CORE,ALIZE,BISE \
  --out ./build/full_index/ALL_SETS
```

See [ALL_SETS index format](ALL_SETS-index-format.md) for merge ordering and on-disk layout.

---

### `add-extra-filter`

Register a card-list Roaring filter on an existing index from a refs file.

| Option | Required | Description |
|--------|----------|-------------|
| `--index-dir <PATH>` | yes | Index root with `catalog.json` and `manifest.json` |
| `--filter-id <ID>` | yes | Stable filter slug; writes `extra/<filter-id>.roar` |
| `--refs-file <PATH>` | yes | One card reference per line; blank lines and `#` comments ignored |
| `--type <format\|property>` | no | Optional filter category stored in `extra_catalog.json` |
| `--negated` | no | If set, refs are an exception list (`negated: true` in catalog) |
| `--replace` | no | Overwrite an existing filter with the same `--filter-id` (default: error if id or `.roar` exists) |

**Example**

```bash
cargo run -p cli-indexer -- add-extra-filter \
  --index-dir ./build/full_index/ALL_SETS \
  --filter-id exclude-banned \
  --refs-file ./lists/banned.txt \
  --type property \
  --negated

# Update the same filter in place
cargo run -p cli-indexer -- add-extra-filter \
  --index-dir ./build/full_index/ALL_SETS \
  --filter-id exclude-banned \
  --refs-file ./lists/banned-v2.txt \
  --replace
```

---

### `audit-missing`

Report cards that are allocated in the catalog bit span but missing or invalid in `cards.bin`, focusing on families where `max_unique_id != card_count` (likely gaps).

| Option | Required | Description |
|--------|----------|-------------|
| `--index-dir <PATH>` | yes | Parent of set folders |
| `--set <NAME>` | yes | Set folder to audit |
| `--json` | no | Emit JSON keyed by `ALT_<SET>_B_<family_id>` with missing reference arrays (default: human-readable text) |

**Example**

```bash
cargo run -p cli-indexer -- audit-missing \
  --index-dir ./build/sets_index \
  --set COREKS \
  --json
```

---

### `bench-query`

Benchmark random idGd queries against an index. Preloads bitmaps and `cards.bin` into memory so timings exclude per-query disk I/O.

| Option | Required | Default | Description |
|--------|----------|---------|-------------|
| `--index-dir <PATH>` | yes | — | Parent of set folders |
| `--set <NAME>` | yes | — | Set or merged folder name |
| `--queries <N>` | no | `5000` | Number of **timed** queries |
| `--warmup <N>` | no | `5` | Warmup iterations (not recorded) |
| `--multi-ids <MIN-MAX>` | no | — | Multi-id mode: pick random **K** ids per query with `MIN ≤ K ≤ MAX`, bucket as TRIGGER/CONDITION/OUTPUT, then intersect unions (e.g. `6-12`) |
| `--seed <U64>` | no | random | RNG seed; if omitted, a time-based seed is chosen and printed in the report |
| `--json-out <PATH>` | no | — | Write machine-readable benchmark JSON |
| `--print-samples <N>` | no | — | Print first **N** sampled queries (debug; adds I/O noise) |
| `--whole-card` | no | `false` | Use whole-card bitmaps instead of per-line |
| `--roaring-only` | no | `false` | Skip `first_50` / `offset_10000_50` (no `cards.bin` read); still times intersect, count, window ops |
| `--json-samples` | no | `false` | Add per-query cardinality / intersect ms to JSON output |

**Modes**

- **Single-id** (default): each timed query picks one id from the non-empty pool and runs the same intersection logic as `query`.
- **Multi-id** (`--multi-ids`): each query picks K ids with incremental non-empty sampling (viability matches production `/effects/filtered` — no zero-card results).

**Timed ops** (in report order): **sample_combinations** (multi-id only — non-empty pack generation), **intersect**, **count**, **first_50**, **offset_10000_50** (skipped with `--roaring-only`), **window_skip**, **window_select**, **window_advance**.

In **multi-id** mode, a progress bar on stderr shows combination generation; the report includes total wall time and per-pack stats for sampling.

Use a **release** build for meaningful timings: `cargo build --release -p cli-indexer`.

**Example**

```bash
cargo build --release -p cli-indexer
./target/release/cli-indexer bench-query \
  --index-dir ./build/full_index \
  --set ALL_SETS \
  --queries 10000 \
  --multi-ids 6-12 \
  --seed 42 \
  --json-out ./bench.json
```

---

## `uniques-http-api`

HTTP server in [`uniques-http-api/`](../uniques-http-api/). It has **no CLI flags**; configuration is via environment variables (and optional `.env` / `.env.local` in that crate’s directory).

```bash
cargo run -p uniques-http-api
```

### Environment variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `INDEX_PATH` | **yes** | — | Path to the index folder itself (e.g. `../build/full_index/ALL_SETS`) |
| `PORT` | no | `8080` | TCP port; bind address is always `0.0.0.0` |

**Local development**

Copy [`uniques-http-api/.env.example`](../uniques-http-api/.env.example) to `.env` and optionally `.env.local` from [`.env.local.template`](../uniques-http-api/.env.local.template). Typical local values:

```env
PORT=8234
INDEX_PATH=../build/full_index/ALL_SETS
```

`load_env()` loads `.env` first, then **overrides** with `.env.local`.

**Docker / Cloud Run**

- Image embeds index at `/opt/index/ALL_SETS` by default.
- Cloud Run sets `PORT`; override `INDEX_PATH` if the index is mounted elsewhere.

See [uniques-http-api README](../uniques-http-api/README.md) and [API spec](api-spec.md).

---

## `demo-ui`

The React demo is a separate npm project ([`demo-ui/`](../demo-ui/)); it does not ship a Rust CLI. Configure the API base URL via Vite env (see `demo-ui/README.md` and `demo-ui/.env.example`).
