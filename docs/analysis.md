# Analysis

## Purpose

Analysis is a read-only description of how a Parquet dataset is physically organized and how that layout may affect interactive reads.

It is separate from:

- **schema/metadata** — what fields and metadata exist;
- **diagnostics** — what happened during one real request.

Analysis is mainly concerned with row groups, column chunks, statistics/index availability, compressed sizes, and estimated read amplification. See [Parquet](parquet.md) for the underlying format concepts.

## Current capabilities

The backend currently exposes analysis for:

- dataset-level physical summary;
- per-column storage characteristics;
- row-group/column-chunk layout;
- optional page-index inspection;
- estimated query read cost;
- advisory findings for interactive access.

Most of this uses footer metadata already associated with the dataset. Page-index inspection may require additional metadata range reads, but it does not scan data pages just to reconstruct missing indexes.

## Query cost

`query-cost` estimates how much compressed Parquet data a viewer-style query may touch. It is an estimate, not a benchmark.

For straightforward offset/limit queries, the engine can identify overlapping row groups and sum relevant projected column chunks. Filtered queries may require conservative estimates because footer metadata cannot always predict where enough matching rows will be found.

The estimate is useful when compared with a real diagnostics trace:

```text
analysis estimate  -> what the file layout suggests
request diagnostics -> what the reader/storage actually did
```

A large difference between the two is itself useful evidence.

## Recommendations

Recommendations are advisory findings, not optimization commands. A layout that is inefficient for browser-style exploration can still be appropriate for another workload.

This viewer does not rewrite files.

## Frontend status

The API is implemented and can be called directly or through the typed frontend API helpers.

**TODO:** add a user-facing Analysis view. Until that exists, avoid documenting proposed cards, scores, charts, or interaction details as current behavior.

For the current routes and their intended cost, see [Analysis API](ANALYSIS_API.md).
