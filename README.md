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

## Documentation

Start with [`docs/index.md`](docs/index.md). The docs intentionally focus on stable project behavior and boundaries; planned features are marked TODO until implemented.

The [Parquet primer](docs/parquet.md) is the most format-oriented document. [Architecture](docs/architecture.md), [Engine](docs/engine.md), [Frontend](docs/frontend.md), and [Analysis](docs/analysis.md) explain the project-specific design.

## Current UI

The browser currently provides:

- remote Parquet/GeoParquet URL opening;
- paged/filtered table exploration with column projection;
- schema and file statistics;
- live request progress/diagnostics;
- frozen filtered map snapshots for detected spatial data;
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
| `POST /datasets/{id}/snapshot` | streamed frozen filtered snapshot |
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

See [`docs/ANALYSIS_API.md`](docs/ANALYSIS_API.md) for the analysis routes.

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

## Run

Backend:

```bash
cargo run --manifest-path src/server/Cargo.toml
```

Frontend:

```bash
cd src/frontend
npm install
npm run dev
```

Then open `http://localhost:5173`.

Or run the containerized application:

```bash
docker compose up --build
```

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
src/server/       Rust API + Parquet/Arrow engine
src/frontend/     React + Arrow JS + MapLibre frontend
docs/             developer documentation
deploy/           reverse-proxy configuration
```

## Security

A server that fetches user-provided URLs is an SSRF boundary. The application validates source URLs and rejects private/special-use targets by default. Production deployments should also enforce appropriate network-level egress restrictions.

## Validation

CI is the source of truth for `cargo check/test/clippy` and the frontend production build. Development documentation should avoid claiming a behavior is validated unless it is covered by the current code/tests/CI.
