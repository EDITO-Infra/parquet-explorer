# Parquet analysis API

The analysis API is read-only and operates on an already opened dataset handle.

Base path:

```text
/api/v1/datasets/{dataset_id}/analysis
```

`dataset_id` comes from `POST /api/v1/datasets/open`. It is a temporary process-local handle, not a persistent identifier from the Parquet file.

## Endpoints

| Endpoint | Purpose | Data-page scan |
|---|---|---:|
| `GET /summary` | high-level physical layout | No |
| `GET /columns` | compressed storage/metadata by leaf column | No |
| `GET /row-groups` | row-group and column-chunk layout | No |
| `GET /pages` | inspect available page indexes | No |
| `POST /query-cost` | estimate physical read cost for a query | No |
| `GET /recommendations` | advisory findings for interactive access | No |

`/pages` may perform extra range reads for optional ColumnIndex/OffsetIndex metadata. It does not scan page payloads to manufacture missing indexes.

## Summary

```http
GET /api/v1/datasets/{id}/analysis/summary
```

Returns dataset-level layout information such as row/column counts, compressed/uncompressed totals, row-group sizing, statistics coverage, and index availability.

## Columns

```http
GET /api/v1/datasets/{id}/analysis/columns
```

Aggregates physical information for Parquet leaf columns, including compressed size, codecs/encodings, and statistics/index availability.

## Row groups

```http
GET /api/v1/datasets/{id}/analysis/row-groups
```

Returns row-group and column-chunk layout information. This is useful for identifying which projected chunks dominate a remote read.

## Pages

```http
GET /api/v1/datasets/{id}/analysis/pages?row_group=0&column=geometry
```

`row_group` and `column` are optional selectors. When page indexes exist, the response exposes the page location information available from those indexes and reports any storage I/O caused by loading them.

## Query cost

```http
POST /api/v1/datasets/{id}/analysis/query-cost
Content-Type: application/json
```

Example request:

```json
{
  "columns": ["id", "name", "geometry"],
  "offset": 0,
  "limit": 1000,
  "filters": []
}
```

The endpoint does not execute the query. It estimates the row groups/column chunks that may be touched and reports their compressed-byte contribution. With filters, the estimate may be a conservative upper bound.

Use `/diagnostics/{trace_id}` for actual measured request behavior.

## Recommendations

```http
GET /api/v1/datasets/{id}/analysis/recommendations
```

Returns advisory findings derived from the physical metadata. These findings are intended as clues for interactive access, not universal Parquet correctness rules.

## Implementation boundary

The HTTP handlers are in `api.rs`; reusable analysis logic lives in `engine/analysis.rs` and is exposed through `CoreEngine` methods. Frontend callers use the typed helpers in `src/frontend/src/lib/api.ts`.

**TODO:** a dedicated frontend Analysis view has not been implemented yet.
