# Parquet analysis API

The analysis API exposes the physical layout of the currently opened Parquet source. It is intentionally **read-only**. This viewer does not rewrite, upload, move, rename, or replace source files.

All routes are below `/api/v1/datasets/{dataset_id}/analysis` and require a dataset handle created by `POST /api/v1/datasets/open`.

## Why this API exists

The table/map endpoints answer "give me rows." The diagnostics endpoint answers "what happened during this specific request." The analysis endpoints answer a different question: **"what about this Parquet file's physical layout explains its behavior?"**

Keeping that logic in the Rust API rather than React means:

- the browser UI can stay simple;
- developers can query the same information with curl/tests/scripts;
- diagnostics and analysis can be compared directly;
- a separate optimization application can reuse the analysis contract later without giving this viewer file-management responsibilities.

## Cost classes

| Endpoint | Reads data pages? | Expected cost |
|---|---:|---|
| `GET /summary` | No | Parquet footer metadata |
| `GET /columns` | No | Parquet footer metadata |
| `GET /row-groups` | No | Parquet footer metadata |
| `POST /query-cost` | No | Parquet footer metadata |
| `GET /recommendations` | No | Parquet footer metadata |
| `GET /pages` | No | Footer + optional ColumnIndex/OffsetIndex range reads |

`/pages` deliberately does **not** scan data pages to reconstruct missing indexes. If a file has no page index, the response says so.

## `GET /summary`

Returns the high-level physical layout:

- row and column counts;
- top-level vs Parquet leaf-column count;
- compressed/uncompressed column-data totals;
- compression ratio;
- average/largest compressed row-group size;
- row-group statistics coverage;
- whether ColumnIndex/OffsetIndex structures are declared.

Use this for a compact Analysis overview in the UI.

## `GET /columns`

Aggregates each Parquet **leaf** column across row groups. Important fields include:

- `compressed_bytes` / `uncompressed_bytes`;
- `percent_of_compressed_data`;
- `average_compressed_bytes_per_value`;
- compression codecs and encodings observed;
- row groups with ordinary statistics;
- row groups with geospatial statistics;
- page-index declarations.

This endpoint answers questions such as "does geometry account for 95% of the file?"

## `GET /row-groups`

Returns each row group and each physical column chunk inside it, including:

- row count;
- compressed and uncompressed size;
- column chunk byte range;
- data/dictionary page offsets;
- compression and encodings;
- statistics availability;
- page-index declarations.

This is the best endpoint for explaining large read amplification. For example, a 1,000-row query may overlap only row group 0, but that row group's geometry column chunk may itself be hundreds of MiB.

## `GET /pages`

Optional query parameters:

```text
row_group=<zero-based index>
column=<Parquet leaf path or top-level field>
```

Example:

```bash
curl 'http://localhost:8080/api/v1/datasets/ID/analysis/pages?row_group=0&column=geometry'
```

The server opens Parquet metadata with `PageIndexPolicy::Optional`. If an OffsetIndex exists, the response exposes each page's:

- byte offset;
- compressed page size;
- first row index;
- derived row count;
- optional unencoded BYTE_ARRAY payload size.

The response also reports `storage_calls`, `storage_ranges`, and `storage_bytes` for the metadata/page-index inspection itself.

## `POST /query-cost`

Request shape:

```json
{
  "columns": ["id", "name", "geometry"],
  "offset": 0,
  "limit": 1000,
  "filters": []
}
```

The endpoint does not execute the query. It estimates which row groups and compressed column chunks the current viewer-style query can touch.

For an **unfiltered** request, `offset` and `limit` identify overlapping row groups. The estimate sums compressed sizes of projected leaf column chunks.

Filter columns are added even when they are not projected, because Parquet `RowFilter` may need to read them to evaluate predicates.

For a **filtered** request, generic footer metadata cannot know which row groups contain enough matching rows to satisfy the final offset/limit. The API therefore returns `estimate_kind = "conservative_upper_bound"` and includes all row groups.

The response includes per-column contributors sorted by compressed bytes, making it suitable for UI statements such as:

```text
Estimated read: 648.5 MiB
geometry:       622.1 MiB (95.9%)
name:            12.4 MiB (1.9%)
...
```

Compare this estimate with actual `data_storage` values in `/diagnostics/{trace_id}`. Large agreement means the physical layout explains the read; a large disagreement points toward reader/object-store behavior worth investigating.

## `GET /recommendations`

Returns advisory findings only. It never changes the file.

Initial findings cover:

- very large row groups;
- geometry dominating compressed storage;
- missing page indexes;
- low row-group statistics coverage.

Thresholds live in `src/server/src/engine/analysis.rs` and are intentionally visible constants rather than hidden UI heuristics. These findings should be treated as clues for interactive workloads, not universal Parquet correctness rules.

## Rust implementation

The layers are deliberately separated:

```text
api.rs
  HTTP extraction/status/JSON only
      |
      v
engine.rs
  public CoreEngine analysis_* methods
      |
      v
engine/analysis.rs
  Parquet metadata interpretation and query-cost logic
      |
      v
engine/source.rs
  object_store + Parquet AsyncFileReader
```

The frontend has typed helpers in `src/frontend/src/api.ts` and matching interfaces in `src/frontend/src/types.ts`, but no Analysis UI is required to use these endpoints.

## Related design documentation

This file is the concrete API reference. For the broader mental model, see [analysis.md](analysis.md). For the backend read path see [engine.md](engine.md), and for the relevant Parquet storage concepts see [parquet.md](parquet.md).
