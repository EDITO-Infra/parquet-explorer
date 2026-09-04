# Querying datasets through the API

The HTTP API is available under `/api/v1`. A query has two stages:

1. Open a remote Parquet or GeoParquet URL to obtain a temporary `dataset_id`.
2. Use that ID to inspect, count, page, stream, or export rows.

The examples below assume the backend is running at `http://localhost:8080`.

## Check the backend

```bash
curl --fail http://localhost:8080/api/v1/health
```

A healthy backend returns JSON similar to:

```json
{
  "status": "ok",
  "native_engine": "parquet-viewer-backend/0.3.0"
}
```

## 1. Open a dataset

```bash
curl --fail --request POST \
  http://localhost:8080/api/v1/datasets/open \
  --header 'content-type: application/json' \
  --data '{
    "uri": "https://example.org/data.parquet",
    "name": "Example dataset"
  }'
```

The response includes metadata, available columns, capabilities, and a generated `dataset_id`:

```json
{
  "dataset_id": "9d5d6fd5-...",
  "name": "Example dataset",
  "uri": "https://example.org/data.parquet",
  "num_rows": 12345,
  "columns": [],
  "geo_columns": [],
  "capabilities": {}
}
```

Copy `dataset_id` into the URLs below. It is a temporary, process-local handle: it expires after the configured idle timeout and becomes invalid when the backend restarts.

## 2. Inspect the schema

Use the schema response to find the exact column names and Arrow data types before constructing filters or projections:

```bash
curl --fail \
  http://localhost:8080/api/v1/datasets/DATASET_ID/schema
```

Dataset metadata is also available from:

```bash
curl --fail \
  http://localhost:8080/api/v1/datasets/DATASET_ID/metadata
```

## 3. Query a page of rows

`POST /page` projects selected columns, applies all filters, and returns an offset/limit window as an [Apache Arrow IPC stream](https://arrow.apache.org/docs/format/Columnar.html#ipc-streaming-format). It does not return JSON rows.

```bash
curl --fail --request POST \
  http://localhost:8080/api/v1/datasets/DATASET_ID/page \
  --header 'content-type: application/json' \
  --header 'accept: application/vnd.apache.arrow.stream' \
  --data '{
    "columns": ["name", "depth"],
    "offset": 0,
    "limit": 100,
    "filters": [
      {"column": "depth", "op": "gte", "value": 10}
    ]
  }' \
  --output page.arrow
```

The page limit must be between 1 and 10,000 rows (and may be lower if the backend is configured with a smaller `PV_MAX_PAGE_SIZE`). Global sorting is reserved by the request contract but is not currently implemented.

### Read the Arrow response in Python

Install `pyarrow` in your own environment, then read the downloaded stream:

```python
import pyarrow.ipc as ipc

with open("page.arrow", "rb") as source:
    table = ipc.open_stream(source).read_all()

print(table.schema)
print(table.to_pylist()[:5])
```

The same response can be streamed directly by any Arrow IPC-compatible client instead of first writing it to disk.

## Filters

A filter has the shape:

```json
{"column": "column_name", "op": "gte", "value": 10}
```

Supported operators are:

| Operator | Meaning | Value required |
|---|---|---:|
| `eq` | equal | yes |
| `neq` | not equal | yes |
| `lt` | less than | yes |
| `lte` | less than or equal | yes |
| `gt` | greater than | yes |
| `gte` | greater than or equal | yes |
| `contains` | contains a string/binary value | yes |
| `is_null` | is null | no |
| `is_not_null` | is not null | no |

Multiple filters are combined with **AND**. Filter values must be JSON strings, numbers, or booleans compatible with the column's Arrow type. A filter column must currently be a top-level Parquet leaf, but it does not need to appear in `columns`.

## Count matching rows

The count endpoint returns JSON. With no filters, it uses footer metadata; a filtered count evaluates the predicates:

```bash
curl --fail --request POST \
  http://localhost:8080/api/v1/datasets/DATASET_ID/count \
  --header 'content-type: application/json' \
  --data '{
    "filters": [
      {"column": "depth", "op": "gte", "value": 10}
    ]
  }'
```

```json
{"count": 42}
```

## Stream the complete matching result

Use `/result` when you need every matching row rather than one page. The response is also an Arrow IPC stream, so large results should be consumed incrementally:

```bash
curl --fail --request POST \
  http://localhost:8080/api/v1/datasets/DATASET_ID/result \
  --header 'content-type: application/json' \
  --header 'accept: application/vnd.apache.arrow.stream' \
  --data '{
    "columns": ["name", "depth"],
    "filters": [
      {"column": "depth", "op": "gte", "value": 10}
    ]
  }' \
  --output result.arrow
```

Unlike `/page`, `/result` has no offset or limit. Prefer `/page` for interactive browsing and use `/result` only when the complete result is intentional.

## Query a GeoParquet bounding box

For datasets whose open/schema response identifies a geometry column, `/spatial` accepts `[min_x, min_y, max_x, max_y]` in the geometry column's CRS:

```bash
curl --fail --request POST \
  http://localhost:8080/api/v1/datasets/DATASET_ID/spatial \
  --header 'content-type: application/json' \
  --header 'accept: application/vnd.apache.arrow.stream' \
  --data '{
    "bbox": [-5.0, 48.0, 10.0, 58.0],
    "geometry_column": "geometry",
    "columns": ["name", "geometry"],
    "max_features": 2000,
    "filters": []
  }' \
  --output spatial.arrow
```

This endpoint currently performs conservative row-group bounding-box pruning, not exact feature-level intersection. The response header `x-parquet-viewer-spatial-filter: row-group-bbox-candidates` makes that behavior explicit; clients must perform exact geometry filtering if they require it.

## Export a subset

`/export` materializes a temporary Arrow or Parquet file and returns it as a download:

```bash
curl --fail --request POST \
  http://localhost:8080/api/v1/datasets/DATASET_ID/export \
  --header 'content-type: application/json' \
  --data '{
    "format": "parquet",
    "columns": ["name", "depth"],
    "offset": 0,
    "limit": 1000,
    "filters": []
  }' \
  --output subset.parquet
```

`format` is `parquet` (the default) or `arrow`. Spatial export is not currently implemented.

## Diagnostics and cleanup

Add a client-generated `trace_id` string to an open, page, result, count, spatial, or export request, then inspect the backend trace:

```bash
curl --fail \
  http://localhost:8080/api/v1/diagnostics/TRACE_ID
```

Close the temporary handle when it is no longer needed:

```bash
curl --fail --request DELETE \
  http://localhost:8080/api/v1/datasets/DATASET_ID
```

A successful close returns HTTP `204 No Content`. If a request returns `404` after expiry or a backend restart, open the source URL again to obtain a new handle.

## Errors and source restrictions

JSON errors include a human-readable `error` and a stable `code` (for example, `invalid_request`, `dataset_not_found`, or `private_network_disallowed`). Unexpected failures return the safe code `internal_error`; details remain in backend logs.

Because opening arbitrary URLs is an SSRF boundary, the backend rejects private and special-use network targets by default. Source-policy rejections return HTTP `400 Bad Request`; this established status is retained for API compatibility, although some policy failures could also be modeled as `403 Forbidden`. Deployments can further restrict source schemes and hosts through backend configuration. See [Architecture](architecture.md) and [Installation](installation.md) for operational details.
