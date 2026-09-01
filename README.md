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

## Developer documentation

The README is the quick-start and current feature/API overview. Broader design documentation lives under [`docs/`](docs/README.md):

- [`architecture.md`](docs/architecture.md) — end-to-end system shape, boundaries, lifecycle, and where changes belong;
- [`engine.md`](docs/engine.md) — Rust backend responsibilities, query/read flow, streaming, storage metrics, and why the native engine exists;
- [`parquet.md`](docs/parquet.md) — a medium-detail Parquet primer focused on row groups, column chunks, pages, indexes, projection, and remote read amplification;
- [`analysis.md`](docs/analysis.md) — the model behind physical-layout analysis, query-cost estimates, findings, and diagnostics comparison;
- [`frontend.md`](docs/frontend.md) — browser state, Arrow streaming/decoding, map snapshots, and progress/diagnostics;
- [`ANALYSIS_API.md`](docs/ANALYSIS_API.md) — the current endpoint-level analysis API reference.

These documents favor stable concepts over line-by-line implementation details so they can remain useful as the code evolves.

## Current frontend

Paste an HTTP(S) Parquet URL and the app:

1. opens only the Parquet metadata first;
2. keeps schema, file statistics, native GeoParquet information and row-group spatial statistics under **Schema & stats** / **File details** instead of crowding the main view;
3. pages the table through Arrow IPC rather than JSON rows, with a live progress bar showing the active read stage;
4. exposes **View on map** from the Table tab, which freezes the current filter state into one streamed Arrow snapshot;
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
| `GET /datasets/{id}/analysis/summary` | cheap footer-derived physical-layout summary |
| `GET /datasets/{id}/analysis/columns` | compressed storage, codecs, encodings and stats coverage by leaf column |
| `GET /datasets/{id}/analysis/row-groups` | row-group + column-chunk physical layout |
| `GET /datasets/{id}/analysis/pages` | optional page-index inspection; accepts `row_group` and `column` query params |
| `POST /datasets/{id}/analysis/query-cost` | estimate compressed bytes touched by a row query without executing it |
| `GET /datasets/{id}/analysis/recommendations` | read-only findings for interactive-access characteristics |
| `POST /datasets/{id}/page` | streaming Arrow IPC, max 10,000 rows per page |
| `POST /datasets/{id}/count` | exact row count for the active filters |
| `POST /datasets/{id}/snapshot` | streaming Arrow IPC for the complete frozen filtered result |
| `POST /datasets/{id}/spatial` | streaming Arrow IPC |
| `POST /datasets/{id}/export` | Parquet or Arrow file |
| `GET /diagnostics/{trace_id}` | live request-stage timings for the progress UI |
| `DELETE /datasets/{id}` | 204 |

Example:

```bash
curl -X POST http://localhost:8080/api/v1/datasets/open \
  -H 'content-type: application/json' \
  -d '{"uri":"https://example.org/data.parquet"}'
```

## Read-only Parquet analysis API

The viewer exposes physical-layout analysis as first-class API endpoints rather than burying this logic in React. This keeps the browser UI optional: scripts, tests, another frontend, or a future standalone optimizer can consume the same information. **None of these endpoints rewrite, upload, move, or replace the source file.**

The cheap endpoints (`summary`, `columns`, `row-groups`, `query-cost`, and `recommendations`) use Parquet footer/column-chunk metadata and do not intentionally read data-page payloads. `analysis/pages` opts into loading Parquet ColumnIndex/OffsetIndex structures; those indexes can require additional object-store range reads, so the response reports the storage calls/ranges/bytes used by that analysis request. It still does not scan the data pages when an index is absent.

Typical inspection flow:

```bash
# 1. File-level layout
curl http://localhost:8080/api/v1/datasets/$DATASET_ID/analysis/summary

# 2. Find which columns dominate storage
curl http://localhost:8080/api/v1/datasets/$DATASET_ID/analysis/columns

# 3. Inspect a large row group / geometry chunk
curl http://localhost:8080/api/v1/datasets/$DATASET_ID/analysis/row-groups

# 4. Inspect page indexes for one column without reading page payloads
curl 'http://localhost:8080/api/v1/datasets/'$DATASET_ID'/analysis/pages?row_group=0&column=geometry'

# 5. Predict the physical read amplification of the viewer query
curl -X POST http://localhost:8080/api/v1/datasets/$DATASET_ID/analysis/query-cost \
  -H 'content-type: application/json' \
  -d '{"columns":["id","name","geometry"],"offset":0,"limit":1000,"filters":[]}'

# 6. Get advisory findings. This endpoint never modifies the source.
curl http://localhost:8080/api/v1/datasets/$DATASET_ID/analysis/recommendations
```

`query-cost` is an estimate, not an execution trace. With no filters it identifies the row groups overlapped by `offset`/`limit` and sums the compressed sizes of the projected column chunks (plus predicate columns when relevant). With filters, it returns a conservative upper bound because generic footer metadata cannot know where enough matching rows will be found. Compare this estimate with `/diagnostics/{trace_id}` to distinguish expected read amplification from unexpected object-store behavior.

The implementation lives in `src/server/src/engine/analysis.rs`. HTTP handlers remain intentionally thin in `api.rs`, while the public engine wrappers (`analysis_summary`, `analysis_columns`, `analysis_row_groups`, `analysis_pages`, `analysis_query_cost`, `analysis_recommendations`) keep the analysis reusable outside HTTP routing. See [`docs/ANALYSIS_API.md`](docs/ANALYSIS_API.md) for field-level behavior, cost classes, examples, and implementation notes.

## Live query diagnostics

The frontend sends an optional `trace_id` with open, page, snapshot, count, and spatial requests. The backend records request-scoped milestones plus explicit measured stage durations. The UI does **not** infer a stage duration by subtracting adjacent milestone timestamps. Milestones are shown as `@ elapsed`; measured work is shown as a duration.

For streamed page/snapshot/spatial reads the backend now separates:

- Parquet footer/metadata storage wait and metadata parsing;
- object-store data reads, including call count, requested ranges, requested bytes, returned bytes, and measured await time;
- Parquet reader work (decompression + Parquet-to-Arrow batch construction), reported as a derived wall-time remainder after measured object-store wait;
- Arrow IPC encoding time and encoded response size;
- server response-channel/backpressure time.

The browser independently measures response-header wait, first response bytes, Arrow IPC schema/batch decoding, table-row materialization, and geometry construction. WKB geometry decoding is timed directly around `appendSpatialBatchFeatures`, so a slow geometry decode is no longer conflated with backend/S3 time. Browser CPU values that subtract response-byte wait are labeled as derived estimates.

This trace contract is request-scoped so the same measurements can become the input to a later Parquet optimization analysis feature: excessive range counts, unexpectedly large projected bytes, geometry-heavy column chunks, poor row-group pruning, and decode-heavy files can all be identified from the same trace. Traces are kept in memory temporarily and addressed by their unguessable request ID.

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
    analysis.rs          read-only physical-layout/query-cost analysis
    ipc.rs               Parquet RecordBatch → Arrow IPC streaming + timings
    source.rs            object-store adapter + exact I/O counters
    trace.rs             request-scoped diagnostics store/reporting
```

`engine.rs` declares its flat child modules, so Rust loads `engine/analysis.rs`, `engine/ipc.rs`, `engine/source.rs`, and `engine/trace.rs`. The earlier scaffold used `api/mod.rs` and `engine/mod.rs`; both layouts are valid Rust, but `api.rs` / `engine.rs` is clearer for a service this size.

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

For a dataset with a detected coordinate pair or geometry column, the Table tab shows **View on map**. Clicking it clones the current filter clauses and matching-row count into a snapshot specification. The map then streams all matching spatial rows through `/snapshot` in one filtered scan instead of issuing viewport queries or repeated 10,000-row page requests.

The currently loaded coordinate table page is converted directly to map features first, so the user sees those rows while the full stream is being materialized. Large WKB columns are not forced into ordinary table page projections unless already visible; their map snapshot begins from the dedicated stream instead. Once the snapshot finishes, it remains mounted when switching tabs, so returning to Map does not reload it. Changing Data-tab filters does not mutate an existing map snapshot; use **Refresh map** / **View on map** to capture the new filter state.

Every snapshot feature gets a stable ID based on its filtered row position. Geometry collections are flattened into renderable component geometries while retaining the source-row properties. Users can click any rendered feature or enable **Box select** and drag a rectangle to select intersecting rendered points, lines, polygons, or multipart geometries. Selection only affects the frozen map view; it does not silently rewrite the Data-tab filters.
