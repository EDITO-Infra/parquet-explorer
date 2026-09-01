# Analysis

## What analysis means in this project

Analysis is a read-only description of how a Parquet dataset is physically organized and how that organization affects interactive reads.

It is intentionally separate from both schema inspection and request diagnostics:

```text
Schema / metadata
  What fields and dataset metadata exist?

Analysis
  How is the file physically laid out and what can we infer from it?

Diagnostics
  What actually happened during this particular request?
```

This separation matters because a slow query can have very different causes. A browser may decode geometry slowly; a remote object store may be slow; or the file may simply require hundreds of megabytes of column chunks to answer a small query. Analysis focuses on the third category.

## Physical units that matter

The most useful Parquet structures for the viewer are:

- **row group** — a horizontal partition of rows;
- **column chunk** — one column's data inside one row group;
- **page** — a smaller encoded unit within a column chunk;
- **statistics** — optional min/max/null information that can help readers skip work;
- **page indexes** — optional structures describing page locations/statistics without scanning page payloads.

See [parquet.md](parquet.md) for a format-oriented explanation.

## Questions the analysis layer should answer

The analysis API is designed around practical questions rather than exposing every field in the Parquet specification:

### How large are the row groups?

Large row groups can be efficient for analytical scans but expensive for interactive exploration if a small request must touch a very large column chunk.

### Which columns occupy the storage?

A geometry, text, image-like binary, or other variable-width column can dominate compressed size. Per-column aggregation makes this visible before blaming CPU or networking generally.

### Are statistics available?

Statistics can allow a reader to rule out irrelevant row groups/pages for suitable predicates. Missing statistics are not necessarily an invalid file; they simply reduce the information available for pruning.

### Are page indexes available?

Page indexes can provide finer-grained location/statistics information. Their absence does not make a file incorrect, and the viewer does not scan all data pages merely to reconstruct them.

### What is the likely I/O cost of this viewer query?

`query-cost` estimates the compressed column chunks that the current style of query may touch. This is a physical-read estimate, not an execution benchmark.

## Query cost versus query result size

One of the most useful concepts is **read amplification**.

Suppose a query asks for 1,000 rows and eventually returns a 522 KiB Arrow stream, but the Parquet reader must fetch 648 MiB of compressed column chunks to produce those rows.

The important observation is not that 1,000 rows are inherently expensive. It is that the file layout and projection cause a much larger physical read than the final result.

A simple ratio can express this:

```text
bytes read from source / bytes returned to caller
```

The exact ratio should be interpreted carefully because response encoding, compression, and filter work differ, but it is an effective clue for interactive-access problems.

## Estimates are deliberately conservative

For an unfiltered offset/limit request, the engine can map the requested row window to row groups and sum compressed sizes for the projected column chunks.

For a filtered query, footer metadata generally cannot know where the Nth matching row will occur. The analysis therefore uses a conservative upper bound rather than claiming false precision.

Similarly, a chunk-size estimate does not guarantee that the underlying reader will issue exactly the same ranges. Compare the estimate with a real diagnostics trace to see whether observed I/O matches the physical layout expectation.

## Recommendations are findings, not commands

The recommendations endpoint converts a few physical characteristics into human-readable findings, for example:

- unusually large row groups for interactive access;
- one geometry column dominating compressed storage;
- missing page indexes;
- low statistics coverage.

These are **not universal Parquet rules**. A file optimized for large sequential analytics may intentionally use a layout that is poor for browser-style random exploration.

For that reason recommendations should contain evidence and context, not just a score or a command such as "optimize this file."

The viewer does not perform optimization. A separate tool can later consume the same analysis concepts when deciding how to rewrite data.

## Analysis cost classes

Most useful analysis can be performed from footer metadata already associated with opening the Parquet file:

```text
summary          footer metadata
columns          footer metadata
row-groups       footer metadata
query-cost       footer metadata
recommendations  footer metadata
```

`pages` is different. It may request optional ColumnIndex/OffsetIndex structures from the remote source. The API reports the storage reads caused by this inspection so an analysis screen does not hide its own cost.

It still does not read page payloads when indexes are missing.

## Analysis and diagnostics together

A useful debugging workflow is:

1. run or observe a slow query;
2. inspect diagnostics to locate the measured bottleneck;
3. inspect row groups/columns/query-cost to explain the physical read;
4. compare estimated and actual bytes.

Example:

```text
Diagnostics
  storage read: 648.5 MiB
  storage wait: 64.0 s
  Parquet reader remainder: 161 ms
  browser WKB decode: 6 ms

Analysis
  estimated read: ~648 MiB
  geometry chunk: ~620 MiB
  first row group: very large
```

That combination strongly suggests the query is storage-bound because of file layout, not because geometry decoding in the browser is slow.

If instead the analysis predicts a small read but diagnostics show a much larger one, the discrepancy is itself valuable: the next investigation should focus on reader or object-store behavior rather than file layout assumptions.

## UI guidance

The analysis API may contain technical detail, but the default UI should summarize it in user-oriented terms:

```text
Interactive access       Poor
Estimated read           648 MiB
Requested rows           1,000
Largest contributor      geometry (96%)
Row groups               4, very large
```

Detailed row-group/page tables can live behind disclosure controls. The goal is to expose the evidence without requiring every user to understand the Parquet specification.

## API reference

For current endpoint names, query parameters, cost classes, and response semantics, see [ANALYSIS_API.md](ANALYSIS_API.md). This document is intentionally more conceptual so it can survive moderate API changes.
