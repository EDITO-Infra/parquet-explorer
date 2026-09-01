# Engine

## Role

The Rust backend is the data engine for the viewer. It combines asynchronous remote reads, Parquet decoding, Arrow RecordBatches, filtering, diagnostics, and read-only analysis.

Rust is useful here because the storage and Parquet/Arrow work can stay in one native asynchronous process without converting the dataset through an additional row-oriented representation.

## Main modules

Current responsibilities are roughly:

```text
engine.rs             CoreEngine and request orchestration
engine/source.rs      remote range reads + I/O metrics
engine/filter.rs      API filters -> reader predicates
engine/ipc.rs         RecordBatch -> Arrow IPC streaming
engine/trace.rs       request-scoped diagnostics
engine/analysis.rs    read-only physical-layout analysis
```

`api.rs` should remain mostly HTTP plumbing rather than data-engine logic.

## Dataset registry

`CoreEngine` keeps a small in-memory registry keyed by the generated `dataset_id`. Each entry contains the validated URI, dataset metadata, and timestamps used for idle expiry.

The registry is not a data cache. Queries still read from the original remote source.

All operations resolve datasets through the engine's handle lookup path so idle expiry and `last_accessed_at` updates are applied consistently. Expired or missing handles become 404 responses at the API boundary.

## Opening a source

Opening a dataset validates the URI, creates a remote reader, loads Parquet metadata, derives the viewer metadata, and registers a temporary handle.

The goal is to make the file inspectable without reading its row payloads up front.

## Reading rows

The common data path is:

```text
request
  -> resolve dataset handle
  -> create remote Parquet reader
  -> apply filters/projection
  -> produce Arrow RecordBatches
  -> encode Arrow IPC
  -> stream HTTP response
```

The server should keep this path streaming rather than collect complete results unnecessarily.

### Projection

Parquet is columnar, so projected columns materially affect remote I/O. A large binary or geometry column should not be read for an ordinary table page unless the request needs it.

### Row limits

A logical limit is not a byte limit. Depending on row-group/page layout and available indexes, returning 1,000 rows can still require a large physical read. See [Parquet](parquet.md).

## Filtering

Filter clauses are compiled into the native reader path. A predicate column may need to be read even when it is not returned in the final projection.

Unsupported semantics should fail clearly instead of being approximated silently.

## Arrow streaming

Parquet is decoded to Arrow `RecordBatch` values and then encoded as Arrow IPC. A bounded response channel prevents the producer from running arbitrarily far ahead of the HTTP consumer and provides a useful backpressure measurement.

## Diagnostics

Storage access is instrumented because remote I/O can dominate query latency. Traces distinguish milestones from measured durations and currently include information such as:

- object-store calls/ranges/bytes;
- storage wait;
- Parquet reader wall time and derived non-storage remainder;
- Arrow IPC encoding;
- response-channel backpressure.

Browser transport and decode timings are measured separately in the frontend.

Some Parquet work and storage I/O happen inside the same awaited reader operation. Where a duration is derived rather than directly measured, the diagnostics should say so.

## Analysis

`engine/analysis.rs` implements the read-only analysis endpoints described in [Analysis](analysis.md) and [Analysis API](ANALYSIS_API.md). It interprets Parquet metadata; it does not optimize or rewrite files.

## Extension rule

When adding engine behavior, first decide which category it belongs to:

- returns rows -> query/read path;
- describes physical layout -> analysis;
- measures one request -> diagnostics;
- changes a source file -> outside this application.
