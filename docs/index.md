# Parquet Viewer developer documentation

This documentation explains the parts of the project that should remain useful even as implementation details change. The source code and API models are the authority for exact fields and function signatures.

## Start here

- [Installation and local setup](installation.md) — required Rust/Cargo and Node/npm versions, dependency installation, updates, and local run commands.
- [Architecture](architecture.md) — system boundaries, request flow, and temporary dataset handles.
- [API](API.md) — open a dataset and query, filter, stream, or export its rows.
- [Engine](engine.md) — Rust backend responsibilities and the remote Parquet read path.
- [Frontend](frontend.md) — frontend feature boundaries, shared state, and data flow.
- [Parquet primer](parquet.md) — row groups, column chunks, pages, range reads, and read amplification.
- [Analysis](analysis.md) — what the read-only analysis layer is intended to explain.
- [Analysis API](ANALYSIS_API.md) — currently implemented analysis endpoints.
- [Show & tell](show-and-tell.md) — an end-to-end walkthrough: backend internals, frontend API usage, why querying is fast, and improvement ideas.

## Project boundaries

The viewer is deliberately read-only. It opens remote Parquet/GeoParquet sources, streams selected rows, renders table/map views, and exposes diagnostics and physical-layout analysis. It does not rewrite or manage source files.

`POST /api/v1/datasets/open` returns a temporary `dataset_id`. The ID is a process-local handle maintained by the Rust backend; it is not part of the Parquet file and should not be persisted as a durable identifier. See [Architecture](architecture.md#dataset-handles).

## Documentation policy

Keep these documents focused on:

- stable responsibilities and boundaries;
- behavior that exists today;
- non-obvious design decisions that help a developer reason about the code.

Planned behavior should be marked **TODO** rather than documented as if it already exists. Avoid mirroring every component, function, response field, or implementation step when the source code already expresses it clearly.
