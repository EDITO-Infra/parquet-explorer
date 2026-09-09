# Parquet Viewer

A read-only Parquet/GeoParquet explorer with a Rust backend and React/MapLibre frontend.

```text
browser
  ├─ table / schema / diagnostics / map
  ├─ Arrow JS
  └─ MapLibre
       │ JSON control + metadata
       │ Arrow IPC row streams
       ▼
Rust API
  ├─ ranged remote reads
  ├─ Parquet -> Arrow
  ├─ filtering / analysis
  └─ request diagnostics
```

The viewer does not download or manage source files as application state. It opens remote HTTP(S) Parquet sources through temporary backend handles and reads the required byte ranges on demand.

## Authorship and funding

Parquet Viewer is developed at the (**Flanders Marine Institute (VLIZ)**)(https://www.vliz.be/) as a part of the (**European Digital Twin Ocean (EDTO)**)[https://www.edito.eu/], a Horizon Europe project supporting the continued development of the European Digital Twin Ocean.

This work is funded by the European Union under **grant agreement No. 101227771**.


> Views and opinions expressed are those of the author(s) only and do not necessarily reflect those of the European Union or the granting authority. Neither the European Union nor the granting authority can be held responsible for them.

## Documentation

Start with [`docs/index.md`](docs/index.md). The docs intentionally focus on stable project behavior and boundaries; planned features are marked TODO until implemented.

The [Parquet primer](docs/parquet.md) is the most format-oriented document. [Architecture](docs/architecture.md), [Engine](docs/engine.md), [Frontend](docs/frontend.md), and [Analysis](docs/analysis.md) explain the project-specific design.

## Current UI

The browser currently provides:

- remote Parquet/GeoParquet URL opening;
- paged/filtered table exploration with column projection;
- schema and file statistics;
- live request progress/diagnostics;
- map views that follow the current table/query state;
- Arrow/Parquet subset export.

The read-only Analysis API is implemented, but a dedicated Analysis UI is not yet present. See [`docs/analysis.md`](docs/analysis.md).

## API

Base path: `/api/v1`.

| Endpoint | Purpose |
|---|---|
| `GET /health` | health check |
| `POST /datasets/open` | open a remote source and return dataset metadata + temporary ID |
| `GET /datasets/{id}/metadata` | dataset metadata |
| `GET /datasets/{id}/schema` | schema |
| `POST /datasets/{id}/page` | streamed Arrow IPC table page |
| `POST /datasets/{id}/count` | exact filtered row count |
| `POST /datasets/{id}/result` | complete projected result stream, optionally filtered |
| `POST /datasets/{id}/spatial` | streamed spatial query |
| `POST /datasets/{id}/export` | Arrow or Parquet subset export |
| `GET /datasets/{id}/analysis/summary` | physical-layout summary |
| `GET /datasets/{id}/analysis/columns` | per-column physical metadata |
| `GET /datasets/{id}/analysis/row-groups` | row-group/column-chunk layout |
| `GET /datasets/{id}/analysis/pages` | available page-index information |
| `POST /datasets/{id}/analysis/query-cost` | estimated physical read cost |
| `GET /datasets/{id}/analysis/recommendations` | advisory findings |
| `GET /diagnostics/{trace_id}` | request trace |
| `DELETE /datasets/{id}` | close temporary dataset handle |

See [`docs/API.md`](docs/API.md) for an end-to-end dataset query guide and [`docs/ANALYSIS_API.md`](docs/ANALYSIS_API.md) for the analysis routes.

## Result loading

Unfiltered table exploration uses bounded `/page` reads. Applying filters streams the complete matching projected result once through `/result`; the browser then pages that cached result locally with a maximum table page size of 500,000 rows. Selecting **All** explicitly uses the same complete-result stream without filters. Map shows the current unfiltered page or reuses the filtered/All cache.

## Dataset IDs

Opening a source returns a generated `dataset_id`:

```bash
curl -X POST http://localhost:8080/api/v1/datasets/open \
  -H 'content-type: application/json' \
  -d '{"uri":"https://example.org/data.parquet"}'
```

The ID is a temporary process-local handle, not an identifier stored in the Parquet file. The frontend keeps the returned `DatasetInfo` in memory and uses `dataset.dataset_id` for later requests.

Handles expire after an idle timeout (`PV_DATASET_IDLE_TIMEOUT_SECONDS`, default `3600`; `0` disables expiry). Opening another URL best-effort closes the previous handle, while backend expiry cleans up abandoned sessions.

## Diagnostics

Requests can carry a trace ID. Backend diagnostics record storage I/O, reader/Arrow work, and response backpressure; the browser separately records transport, Arrow JS decoding, row materialization, and WKB geometry decoding.

Milestones and measured durations are kept distinct so the UI does not infer timings from unrelated timestamps.

## Installation

Prerequisites:

- Rust and Cargo **1.88+** — the latest stable Rust is recommended.
- Node.js **20.19+ or 22.12+** with npm — the latest LTS release is recommended.
- Optional: Docker with Compose support.

From the repository root:

```bash
rustup update stable
cargo fetch --manifest-path src/backend/Cargo.toml --locked
npm ci --prefix src/frontend
```

## Run

Start the backend and frontend in separate terminals:

```bash
cargo run --manifest-path src/backend/Cargo.toml
npm run dev --prefix src/frontend
```

Open `http://localhost:5173`

Docker Compose


```bash
docker compose up --build
```
go to `http://localhost:3000`

## Common development commands

```bash
make check
make test
make dev-backend
make dev-frontend
make build-backend
make build-frontend
```

## Repository layout

```text
src/backend/       Rust API + Parquet/Arrow engine
src/frontend/     React + Arrow JS + MapLibre frontend
docs/             developer documentation
deploy/           reverse-proxy configuration
```

## Security

A server that fetches user-provided URLs is an SSRF boundary. The application validates source URLs and rejects private/special-use targets by default. Production deployments should also enforce appropriate network-level egress restrictions.

## Test dataset

A public dataset useful for local development and performance testing:

```text
https://s3.waw3-1.cloudferro.com/emodnet/emodnet_biology/12639/marine_biodiversity_observations_occurrence_2026-08-19.parquet
```

`IMISDatasetId: 9064`

Local dataset will fail

https://www.lifewatch.be/etn/parquet/detections/RATJADA/RATJADA_detections.parquet

## Validation

CI is the source of truth for `cargo check/test/clippy` and the frontend production build. Development documentation should avoid claiming a behavior is validated unless it is covered by the current code/tests/CI.
