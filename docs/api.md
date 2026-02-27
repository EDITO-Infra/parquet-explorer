# API (v1)

## Health

- `GET /health`

## Dataset registration and discovery

- `GET /api/datasets`
- `POST /api/datasets`
  - body: `{ "id": "dataset_name", "uri": "s3://bucket/path/*.parquet" }`

## Data exploration

- `GET /api/schema?dataset=<id>`
- `GET /api/preview?dataset=<id>&limit=100`
- `POST /api/query?dataset=<id>`
  - body: `{ "sql": "SELECT * FROM __dataset LIMIT 50", "limit": 1000, "offset": 0 }`

`__dataset` is the logical table alias available inside query SQL.

## Geo eligibility

- `GET /api/geo/eligible?dataset=<id>&geom_column=geom`

Returns whether map mode is available for the dataset.

## Tiles

- `GET /tiles/{z}/{x}/{y}.mvt?dataset=<id>&geom_column=geom&where=...`

If `PV_TILE_SERVICE_URL` is set, request is forwarded to the external tile provider.
Otherwise endpoint returns `501`.

The repository includes a Rust tile service scaffold at `src/tile-rust` implementing this contract.
