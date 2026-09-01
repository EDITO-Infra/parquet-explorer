# Developer documentation

These documents explain the project at different levels. They are intentionally more conceptual than source-code documentation so they should remain useful as individual functions and components change.

Start here if you are new to the project:

1. [Architecture](architecture.md) — the end-to-end shape of the application and the boundaries between browser, API, engine, and storage.
2. [Parquet primer](parquet.md) — the parts of the Parquet format that matter when building an interactive viewer.
3. [Engine](engine.md) — how the Rust backend opens files, plans reads, streams Arrow, and records diagnostics.
4. [Frontend](frontend.md) — how React consumes metadata and Arrow streams, maintains table/map state, and reports browser-side timings.
5. [Analysis](analysis.md) — how physical-layout analysis differs from query diagnostics and how to interpret the results.
6. [Analysis API reference](ANALYSIS_API.md) — endpoint-level behavior and current request/response semantics.

The README remains the quickest way to run the application. These documents are meant to answer the next questions: **what talks to what, why is it arranged this way, and where should a change belong?**
