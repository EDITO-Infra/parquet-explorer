# Parquet Viewer

A native-Rust Parquet/GeoParquet 2 explorer with a React table + interactive MapLibre map/globe frontend.

```text
browser
  ├─ React virtual table
  ├─ Arrow JS
  └─ MapLibre map (Mercator default, optional globe projection)
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
4. exposes **Switch to map** from the Data tab, which freezes the current filter state into one streamed Arrow snapshot;
5. renders the full matching spatial result on a MapLibre map with switchable Mercator/globe projection;
6. supports click selection plus hand-drawn box selection for points, lines, polygons, multiparts, and decoded geometry collections;
7. downloads Arrow or Parquet subsets.

The map snapshot has no 3,000-feature application cap. Coordinate table pages are reused as an immediate map seed, while the backend streams the complete matching result once through `/snapshot`; pan/zoom and basemap/projection changes do not re-query Parquet. WKB is still decoded to GeoJSON in the browser. The UI includes Light/Streets/Dark OpenFreeMap basemaps, a Map/Globe projection toggle, and a persistent light/dark application theme. For very large or complex snapshots, the next rendering upgrade remains GeoArrow-Wasm + `@geoarrow/deck.gl-geoarrow`; Arrow IPC remains the transport.

## API

Base path: `/api/v1`.

| Endpoint | Response |
|---|---|
| `GET /health` | JSON |
| `POST /datasets/open` | JSON dataset metadata |
| `GET /datasets/{id}/metadata` | JSON |
| `GET /datasets/{id}/schema` | JSON |
| `POST /datasets/{id}/page` | streaming Arrow IPC, max 10,000 rows per page |
| `POST /datasets/{id}/count` | exact row count for the active filters |
| `POST /datasets/{id}/snapshot` | streaming Arrow IPC for the complete frozen filtered result |
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

Exact GeoRust geometry filtering remains the next spatial milestone; the ordinary table filter path is already implemented with native Arrow/Parquet predicates.

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


## Testing

https://s3.waw3-1.cloudferro.com/emodnet/emodnet_biology/12639/marine_biodiversity_observations_occurrence_2026-08-19.parquet

IMISDatasetId = 9064
## Repository layout

```text
src/
  server/               native axum/Arrow/Parquet backend
  frontend/             React + Arrow JS + MapLibre map/globe
deploy/
  nginx.conf             production SPA + /api reverse proxy
Dockerfile               backend
Dockerfile.frontend      frontend
```

## Source security

A public paste-a-URL service is an SSRF boundary. The server rejects embedded URL credentials and, by default, private/loopback/link-local/special-use HTTP targets. For production, also enforce network-layer egress restrictions; hostname validation alone cannot fully solve redirects or DNS rebinding.

## Validation note

This environment does not contain a Rust toolchain and cannot currently reach npm long enough to install frontend dependencies. CI is included to run `cargo check/test/clippy` and the Vite production build in a normal networked environment.

## Table filtering

The Data tab supports server-side filters with `=`, `!=`, `<`, `<=`, `>`, `>=`, `contains`, `is null`, and `is not null`. Multiple filters are ANDed. Filters are compiled to Parquet-RS `RowFilter` predicates, so paging happens over matching rows and filter columns do not have to be part of the output projection. When filters are active, the UI requests an exact matching-row count and pages through the full result set with page sizes up to 10,000 rows. Table columns can be shown or hidden with checkboxes; hidden columns are omitted from the page projection. Arrow and Parquet exports inherit the active Data-tab filters.

`contains` is case-sensitive and intended for string columns. Exact matching should be preferred on very large files when possible.

## Frozen map snapshots

For a dataset with a detected coordinate pair or geometry column, the Data tab shows **Switch to map**. Clicking it clones the current filter clauses and matching-row count into a snapshot specification. The map then streams all matching spatial rows through `/snapshot` in one filtered scan instead of issuing viewport queries or repeated 10,000-row page requests.

The currently loaded coordinate table page is converted directly to map features first, so the user sees those rows while the full stream is being materialized. Large WKB columns are not forced into ordinary table page projections unless already visible; their map snapshot begins from the dedicated stream instead. Once the snapshot finishes, it remains mounted when switching tabs, so returning to Map does not reload it. Changing Data-tab filters does not mutate an existing map snapshot; use **Refresh map snapshot** / **Switch to map** to capture the new filter state.

Every snapshot feature gets a stable ID based on its filtered row position. Geometry collections are flattened into renderable component geometries while retaining the source-row properties. Users can click any rendered feature or enable **Box select** and drag a rectangle to select intersecting rendered points, lines, polygons, or multipart geometries. Selection only affects the frozen map view; it does not silently rewrite the Data-tab filters.
