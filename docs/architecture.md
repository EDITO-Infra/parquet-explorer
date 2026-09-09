# Architecture

## Purpose

Parquet Viewer is a portable, read-only explorer for remote Parquet and GeoParquet data. The browser handles interaction and presentation; a Rust service handles remote storage access, Parquet reading, filtering, analysis, and Arrow IPC streaming.

```text
Remote Parquet object
        │ ranged reads
        ▼
Rust API / CoreEngine
        │ JSON metadata + Arrow IPC
        ▼
React frontend
```

The source object is never rewritten or moved by this application.

## Main boundaries

### Frontend

The browser owns UI state and interaction: the active dataset, table controls, map interaction, progress display, and browser-side decoding. It should not reproduce Parquet physical-layout rules in JavaScript.

### HTTP API

The API is a thin transport layer over `CoreEngine`. It converts HTTP requests into engine calls and engine results into HTTP responses.

### Core engine

The engine owns source validation/read access, Parquet query construction, filtering, Arrow streaming, diagnostics, and read-only physical-layout analysis.

### Source layer

Remote sources are read with byte ranges rather than downloaded eagerly. The source adapter also records I/O measurements used by diagnostics.

Private and special-use source addresses are blocked by default as an SSRF defense. Because the HTTP client resolves the hostname again when connecting, exposed deployments should also enforce network-level egress controls.

## Dataset handles

Opening a source creates a temporary server-side handle:

```text
POST /datasets/open
      │
      ├─ validate source
      ├─ read Parquet metadata
      └─ generate dataset_id
               │
               ▼
CoreEngine.datasets[dataset_id]
```

A handle contains the validated URI and lightweight dataset metadata. It does **not** contain the Parquet file or row data.

The handle is process-local and intentionally temporary:

- each successful lookup refreshes its idle timestamp;
- idle handles expire after `PV_DATASET_IDLE_TIMEOUT_SECONDS` (default 3600 seconds; `0` disables expiry);
- a periodic cleanup task removes abandoned handles;
- the frontend best-effort closes the previous handle when another URL is opened;
- a server restart clears all handles;
- unknown or expired IDs return 404 and the source must be opened again.

This avoids resending/revalidating a full source URL on every request while keeping the service close to stateless.

## Typical request flow

### Open

The frontend sends a URL to `/datasets/open`. The backend validates the source and reads enough Parquet metadata to return `DatasetInfo`. No table scan is required just to open the file.

### Table

Unfiltered exploration uses bounded `/page` requests. Once filters are active, the backend streams the complete projected matching result through `/result` once and the browser pages that cache locally. Choosing **All** explicitly uses the same complete-result path without filters. Table page size remains capped at 500,000 rows.

### Map

Table and Map share the current browser result. Unfiltered exploration is backend-paged and Map shows the current page; filtered or explicit All results are streamed once and paged locally. Pan/zoom and basemap changes do not issue Parquet scans.

### Diagnostics

A request may carry a trace ID. Backend timings are recorded close to storage/reader/IPC work; browser timings are measured separately after bytes reach the frontend.

### Analysis

The analysis API describes the file's physical layout using Parquet metadata. It is read-only and separate from request diagnostics.

## Rules worth preserving

- Keep the viewer read-only.
- Keep Parquet/storage interpretation in Rust.
- Use Arrow IPC for streamed row data rather than a parallel JSON row format.
- Keep diagnostics observational: tracing must not be required for a request to succeed.
- Treat user-provided remote URLs as a security boundary.

Exact module names may change; these ownership boundaries matter more than the current file layout.
