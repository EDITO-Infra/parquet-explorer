# Architecture

## Purpose

Parquet Viewer is a portable, read-only browser explorer for Parquet and GeoParquet data. A user provides a remote HTTP(S) Parquet URL, the backend reads only the parts of the object it needs, and the browser presents table, schema/statistics, diagnostics, and map views.

The application deliberately does **not** manage source files. Opening a dataset creates an in-memory handle; closing it removes that handle. The source object remains where it is and is never rewritten by the viewer.

## System shape

```text
Remote Parquet / GeoParquet object
          │
          │ ranged reads
          ▼
┌──────────────────────────────┐
│ Rust backend                 │
│                              │
│  HTTP API                    │
│      ↓                       │
│  CoreEngine                  │
│      ├─ Parquet reader       │
│      ├─ analysis             │
│      ├─ diagnostics          │
│      └─ Arrow IPC streaming  │
└──────────────┬───────────────┘
               │
               │ JSON control/metadata
               │ Arrow IPC row streams
               ▼
┌──────────────────────────────┐
│ Browser frontend             │
│                              │
│  React application state     │
│      ├─ table                │
│      ├─ schema/stats         │
│      ├─ progress/details     │
│      └─ MapLibre map         │
└──────────────────────────────┘
```

There are two broad kinds of traffic between the frontend and backend:

- **JSON** for small control messages, metadata, analysis results, counts, and diagnostics.
- **Arrow IPC streams** for rows. Rows are not converted to large JSON arrays on the server.

## Main responsibilities

### Frontend

The browser owns presentation and interaction. It decides which columns are visible, which filters are active, which table page is requested, and when a map snapshot should be created. It decodes Arrow IPC and WKB geometry after bytes reach the browser.

The frontend should not contain Parquet physical-layout rules. If a conclusion depends on row groups, column chunks, page indexes, or compressed byte sizes, that logic belongs in the backend analysis layer.

### HTTP API

The API translates HTTP requests into engine calls and engine results into HTTP responses. Handlers should remain thin: validate/extract request data, call the engine, set appropriate response headers/status, and return the result.

Keeping HTTP concerns separate makes the engine useful outside this particular React application.

### Core engine

`CoreEngine` is the main backend boundary. It owns lightweight dataset handles and exposes operations such as opening a dataset, reading a page, counting filtered rows, creating a map snapshot, exporting a subset, and inspecting the Parquet layout.

A dataset handle contains metadata and a validated URI. It does not contain a local copy of the source file.

### Source reader

The source layer adapts remote object storage to the asynchronous reader expected by the Parquet implementation. Its most important property for this project is **range access**: the engine can request selected byte ranges instead of downloading the entire object.

The source layer also records exact I/O metrics used by diagnostics: calls, ranges, requested/received bytes, and time spent awaiting storage.

### Analysis

Analysis describes the physical structure of a dataset rather than returning its rows. It answers questions such as:

- How large are the row groups?
- Which columns dominate compressed storage?
- Are ordinary statistics or page indexes available?
- How many compressed bytes is a viewer-style query likely to touch?

Analysis is read-only and is intentionally exposed as an API rather than being hidden inside frontend components.

### Diagnostics

Diagnostics describe **one actual request**. They measure what happened while a query ran: storage I/O, reader work, Arrow encoding, browser transport, Arrow decoding, and geometry decoding.

Analysis and diagnostics complement each other. Analysis can explain why a query is expected to be expensive; diagnostics show what actually happened.

## Typical lifecycle

### 1. Open

The browser sends a URL to `POST /datasets/open`.

The backend validates the source, reads Parquet footer metadata, builds a `DatasetInfo`, and stores a lightweight dataset handle. The browser can now render schema and basic file information without downloading row data.

### 2. Explore rows

The browser sends a page request containing an offset, limit, projection, and optional filters. The engine creates a Parquet reader for that request and streams Arrow RecordBatches as Arrow IPC.

The browser incrementally decodes batches and materializes table rows. Large geometry columns are not required for an ordinary table request unless selected or needed for the current spatial workflow.

### 3. Inspect performance

A request can carry a trace ID. The server records measured backend stages under that ID while the browser records its own transport/CPU stages. The progress UI combines the two without pretending that milestone gaps are measured operation durations.

### 4. Inspect file layout

Analysis endpoints reuse Parquet metadata to expose row-group and column-chunk information. Most analysis is footer-only. Page analysis may load optional page-index structures, but does not scan data pages just to reconstruct missing indexes.

### 5. Map

The map uses a frozen snapshot of the active filters rather than repeatedly querying on every pan or zoom. The backend streams the matching spatial rows once; the browser decodes geometry and MapLibre handles rendering/interaction.

## Important boundaries

These boundaries are intentional and worth preserving as features are added:

- **Read-only viewer:** do not add file rewrite/optimization operations here.
- **Parquet knowledge in Rust:** physical-layout and query-cost logic belongs in the engine/analysis layer.
- **Presentation in React:** labels, tabs, selection, and rendering belong in the frontend.
- **Arrow for rows:** avoid adding a parallel JSON-row data path unless there is a specific reason.
- **Diagnostics are observational:** tracing must never be required for a query to succeed.
- **Remote URLs are a security boundary:** source validation and production egress controls matter because the server performs network requests on behalf of users.

## Where to make changes

A useful rule of thumb:

| Change | Primary location |
|---|---|
| New HTTP route | `src/server/src/api.rs` |
| New reusable read/query capability | `src/server/src/engine.rs` + engine module |
| Parquet layout interpretation | `src/server/src/engine/analysis.rs` |
| Remote read behavior / I/O metrics | `src/server/src/engine/source.rs` |
| Arrow stream encoding | `src/server/src/engine/ipc.rs` |
| Request diagnostics | `src/server/src/engine/trace.rs` |
| Shared API models | `src/server/src/model.rs` and frontend `types.ts` |
| Browser API call | `src/frontend/src/api.ts` |
| Table/map/UI behavior | React components under `src/frontend/src/` |

For a deeper explanation of the backend, see [engine.md](engine.md). For the storage model behind these decisions, see [parquet.md](parquet.md).
