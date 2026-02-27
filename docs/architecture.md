# Architecture

## Service topology (v1)

- Browser UI (React + MapLibre)
- Query API service (Python/FastAPI/DuckDB)
- Tile API service (pluggable; Rust/Go/Python)

Reference tile service scaffold: `src/tile-rust`

```text
Browser -> Query API (/api/*)
Browser -> Tile API  (/tiles/*)
```

## Contracts for swapability

- `DatasetCatalog`: dataset registration + resolution
- `QueryEngine`: schema, preview, query operations
- `TileProvider`: tile retrieval/generation

These are backend adapter points. Keep API responses stable while swapping implementations.

## Data access

- S3 URIs supported via DuckDB `httpfs`
- Local mounted storage supported by absolute paths
- Partitioned Parquet expected

## Geo handling

- Map mode requires a WKB-compatible binary column (`BLOB`/`VARBINARY`)
- If missing, dataset is still queryable but map tab should show not-eligible reason

## Runtime guardrails

- Single-statement, read-only SQL
- Statement timeout
- Memory limit
- Max returned rows

## Migration path

1. Keep frontend + API contracts unchanged.
2. Replace Python `QueryEngine` with Rust/Go service if needed.
3. Replace/provide dedicated Rust/Go tile service by setting `PV_TILE_SERVICE_URL`.

## Current rust tile service state

- Endpoint contract implemented (`/tiles/{z}/{x}/{y}.mvt`, `/health`)
- Returns empty MVT payload in v1 scaffold
- Designed to be replaced with full MVT generation without backend/frontend API changes
