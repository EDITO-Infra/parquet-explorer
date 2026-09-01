# Engine

## What the engine is

The Rust backend is a small read/query engine around Parquet, Arrow, and ranged object-store access. It exists so the browser can explore large remote files without downloading the whole file or asking JavaScript to implement Parquet storage semantics.

The engine is **read-only**. It creates readers and streams results; it does not own, rewrite, or reorganize source files.

## Why Rust is used here

The useful property is not simply that Rust is fast. The backend sits directly on the data path and needs to combine several things efficiently:

- asynchronous network I/O;
- Parquet metadata and selective column/row-group reads;
- decompression and decoding into Arrow RecordBatches;
- streaming Arrow IPC without building the full result in memory;
- predictable memory ownership while requests are concurrent;
- instrumentation close to the actual storage reads.

Rust and the Arrow/Parquet ecosystem let those operations stay in one native process with relatively little data conversion. The frontend then receives Arrow, the same columnar representation produced by the reader, rather than a second row-oriented serialization invented for the application.

The format-level reasons this matters are covered in [parquet.md](parquet.md).

## Module responsibilities

```text
engine.rs
  CoreEngine and high-level query orchestration

engine/source.rs
  remote object-store access
  Parquet AsyncFileReader adapter
  exact storage I/O metrics

engine/filter.rs
  conversion from API filter clauses to Parquet/Arrow predicates

engine/ipc.rs
  RecordBatch -> Arrow IPC stream
  stream/encoding/backpressure diagnostics

engine/trace.rs
  request-scoped diagnostic events and measured durations

engine/analysis.rs
  read-only interpretation of Parquet physical metadata
  query-cost estimates and advisory findings
```

`api.rs` sits above the engine and should contain HTTP mechanics rather than data-engine logic.

## Dataset handles

Opening a dataset creates a generated dataset ID and stores a `DatasetEntry` in memory. The entry contains the already-extracted `DatasetInfo`, including the validated source URI.

This is intentionally lightweight:

```text
dataset ID
   ↓
in-memory DatasetEntry
   ↓
validated remote URI + cached viewer metadata
```

It is **not**:

```text
dataset ID
   ↓
fully downloaded Parquet file
```

Each data operation can create a fresh reader against the original URI. This keeps opening cheap and avoids a server-side file-management layer.

## Opening a file

Conceptually, `open_dataset` does four things:

1. check the in-memory handle limit;
2. create a reader for the validated URI and load enough Parquet metadata to prove the source is readable;
3. derive application metadata such as schema, row count, GeoParquet information, and capabilities;
4. cache a lightweight dataset handle.

No table rows are read merely to open the file.

## Reading rows

A page/snapshot/spatial request follows this general pipeline:

```text
request
  ↓
resolve dataset handle
  ↓
create remote Parquet reader
  ↓
apply filters
  ↓
apply projected columns
  ↓
select row window / row groups where applicable
  ↓
Parquet -> Arrow RecordBatch stream
  ↓
Arrow IPC encoder
  ↓
HTTP byte stream
```

The exact planning varies by operation, but the important design is that batches remain streaming. The server should not collect an entire result simply to serialize it afterward.

### Projection matters

Parquet is columnar. Asking for fewer columns can dramatically reduce storage reads, particularly when one binary/geometry column dominates the file. The engine therefore accepts an explicit projection for table-style operations.

### A row limit is not a byte limit

A request for 1,000 rows does not guarantee that only the bytes representing 1,000 rows will be fetched. Parquet is organized into row groups, column chunks, and pages. Depending on file layout and available indexes, producing the first 1,000 rows can require fetching much larger compressed chunks.

That distinction is why the engine has both diagnostics and physical-layout analysis.

## Filtering

API filter clauses are translated in `engine/filter.rs`. Filtering is applied in the native read path rather than after all rows reach the browser.

A filter column may need to be read even if the column is not part of the output projection. This matters for query-cost estimates: the physical read set is not always identical to the columns visible in the table.

The filtering layer should remain conservative about semantics. Unsupported or ambiguous operations should fail clearly rather than silently change meaning.

## Arrow IPC streaming

The Parquet reader produces Arrow `RecordBatch` values. `engine/ipc.rs` serializes those batches to Arrow IPC and feeds an HTTP byte stream through a bounded channel.

The bounded channel serves two purposes:

- it prevents the producer from running arbitrarily far ahead of the HTTP consumer;
- time waiting on the channel gives a useful server-side backpressure measurement.

Backpressure time is **not** network transfer time. The browser measures when response bytes actually arrive.

## Storage reads and diagnostics

`engine/source.rs` is deliberately instrumented because storage is often the dominant cost for remote Parquet.

For a traced request it can report:

- object-store call count;
- number of requested ranges;
- bytes requested and received;
- measured time awaiting storage.

The Parquet library can perform storage I/O while the application is awaiting the next RecordBatch. Because storage and reader work are interleaved internally, the engine reports some reader timings as derived values:

```text
RecordBatch polling wall time
  - measured object-store await time
  = remaining reader work estimate
```

That remainder covers work such as decompression and Parquet-to-Arrow decoding. It is useful, but it is labeled as derived rather than presented as a directly timed internal Parquet function.

## Diagnostics contract

A trace contains two kinds of information:

- **milestones**, shown as an elapsed timestamp such as `@ 240 ms`;
- **measured/derived operations**, shown as durations such as `64.0 s`.

The UI must not calculate an operation duration by subtracting arbitrary adjacent milestones. That was intentionally removed because it can attribute waiting time to the wrong stage.

Tracing is optional. A failed diagnostics update must never fail the data request itself.

## Analysis inside the engine

`engine/analysis.rs` operates mostly on Parquet metadata rather than table rows. It exposes reusable functions for:

- file summary;
- per-column physical storage;
- row-group/column-chunk layout;
- optional page-index inspection;
- viewer-style query-cost estimates;
- advisory findings.

These functions are called through `CoreEngine` wrappers so HTTP is not the only possible caller. See [analysis.md](analysis.md) for the conceptual model and [ANALYSIS_API.md](ANALYSIS_API.md) for the current API contract.

## Concurrency and memory model

The service uses asynchronous I/O so remote reads do not require one operating-system thread per waiting request. Arrow batches and bounded streaming are used to avoid materializing large result sets unnecessarily.

Dataset metadata is shared behind an in-memory lock because handles are small and mostly read after creation. Source data is not placed behind that lock.

As the service evolves, the important invariant is more useful than any current implementation detail: **do not turn an interactive range-reader into a whole-file cache by accident.**

## Error handling

Engine functions return errors upward rather than deciding HTTP status codes themselves. This keeps the core logic usable from non-HTTP callers and leaves protocol-specific mapping to the API/error layer.

Errors should include enough context to identify the failing stage (opening metadata, building a reader, applying a filter, encoding a stream) without exposing secrets from the runtime environment.

## Extending the engine

When adding functionality, first decide which category it belongs to:

- returns rows -> query/read path;
- describes physical layout -> analysis;
- measures one request -> diagnostics;
- handles transport -> API/IPC;
- changes a source file -> **not this application**.

Keeping these categories separate is more important than preserving the exact current file/module layout.
