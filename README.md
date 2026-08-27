# Parquet Globe

A native-Rust Parquet/GeoParquet 2 explorer with a React table + globe frontend.

```text
browser
  ├─ React virtual table
  ├─ Arrow JS
  └─ MapLibre globe
       │
       │ JSON metadata / control
       │ Arrow IPC table + spatial streams
       ▼
axum / Tokio
  ├─ object_store        ranged HTTP/object-store reads
  ├─ parquet-rs 59.2     Parquet + native GEOMETRY/GEOGRAPHY
  └─ arrow-rs 59.2       RecordBatch + Arrow IPC
```

There is no Python, FastAPI, PyArrow, PyO3, DuckDB, or tile microservice in the request path.

## Current frontend

Paste an HTTP(S) Parquet URL and the app:

1. opens only the Parquet metadata first;
2. shows schema, row counts, native GeoParquet information and row-group spatial statistics;
3. pages the table through Arrow IPC rather than JSON rows;
4. opens native `GEOMETRY` / `GEOGRAPHY` columns on a MapLibre globe;
5. issues `/spatial` viewport queries as the map moves;
6. downloads Arrow or Parquet subsets.

The first map adapter intentionally caps viewport results and decodes WKB to GeoJSON in the browser. This makes the scaffold usable immediately without changing the backend contract. The performance upgrade is to replace only `src/frontend/src/wkb.ts` + the MapLibre GeoJSON source with GeoArrow-Wasm + `@geoarrow/deck.gl-geoarrow`; Arrow IPC remains the transport.

## API

Base path: `/api/v1`.

| Endpoint | Response |
|---|---|
| `GET /health` | JSON |
| `POST /datasets/open` | JSON dataset metadata |
| `GET /datasets/{id}/metadata` | JSON |
| `GET /datasets/{id}/schema` | JSON |
| `POST /datasets/{id}/page` | streaming Arrow IPC |
| `POST /datasets/{id}/spatial` | streaming Arrow IPC |
| `POST /datasets/{id}/export` | Parquet or Arrow file |
| `DELETE /datasets/{id}` | 204 |

Example:

```bash
curl -X POST http://localhost:8080/api/v1/datasets/open \
  -H 'content-type: application/json' \
  -d '{"uri":"https://example.org/data.parquet"}'
```

## GeoParquet 2

The backend uses Parquet-native `GEOMETRY` and `GEOGRAPHY` logical types rather than guessing that binary columns are geometry. It also parses the GeoParquet `geo` metadata, exposes the primary geometry column, CRS/edge metadata, geometry types and row-group bounding boxes.

`/spatial` currently uses **conservative row-group bounding-box pruning**. It is fast but may include feature-level false positives, and therefore reports:

```text
X-Parquet-Viewer-Spatial-Filter: row-group-bbox-candidates
```

Exact GeoRust filtering is the next backend milestone. The GeoArrow-RS main branch has moved to Arrow-RS 59, while some published 0.8 crates still declare Arrow-RS 58. The clean options are to pin a compatible GeoArrow git revision or wait for the next crates.io release; either way, the HTTP contract does not change.

## Why there are no `mod.rs` files

This project uses the modern flat Rust module layout:

```text
src/server/src/
  main.rs
  api.rs
  config.rs
  engine.rs
  error.rs
  model.rs
  security.rs
  engine/
    ipc.rs
    source.rs
```

`engine.rs` declares `mod ipc; mod source;`, so Rust loads `engine/ipc.rs` and `engine/source.rs`. The earlier scaffold used `api/mod.rs` and `engine/mod.rs`; both layouts are valid Rust, but `api.rs` / `engine.rs` is clearer for a service this size.

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

Then open `http://localhost:5173`. Vite proxies `/api` to `http://localhost:8080`.

Or use Docker:

```bash
docker compose up --build
```

and open `http://localhost:3000`.

## Repository layout

```text
src/
  server/               native axum/Arrow/Parquet backend
  frontend/             React + Arrow JS + MapLibre globe
deploy/
  nginx.conf             production SPA + /api reverse proxy
Dockerfile               backend
Dockerfile.frontend      frontend
```

## Source security

A public paste-a-URL service is an SSRF boundary. The server rejects embedded URL credentials and, by default, private/loopback/link-local/special-use HTTP targets. For production, also enforce network-layer egress restrictions; hostname validation alone cannot fully solve redirects or DNS rebinding.

## Validation note

This environment does not contain a Rust toolchain and cannot currently reach npm long enough to install frontend dependencies. CI is included to run `cargo check/test/clippy` and the Vite production build in a normal networked environment.
