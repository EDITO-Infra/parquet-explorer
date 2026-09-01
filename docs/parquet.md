# Parquet primer for an interactive viewer

## What Parquet is

Apache Parquet is a **columnar file format**. Instead of storing complete rows one after another, it organizes values so readers can often fetch and decode only the columns and horizontal partitions relevant to a query.

That design is excellent for analytics, but it also means that concepts such as "read 1,000 rows" and "download only the bytes for 1,000 rows" are not equivalent.

This document focuses on the parts of Parquet that matter when building or debugging an interactive remote viewer. It is not a complete specification.

## Logical rows, physical layout

Imagine a table:

```text
id | name | depth | geometry
---+------+-------+----------
1  | A    | 12.4  | ...
2  | B    | 18.1  | ...
3  | C    |  9.7  | ...
...
```

A row-oriented format tends to place the values for row 1 near each other, then row 2, and so on.

Parquet instead groups rows into **row groups**, and inside each row group stores each column separately as a **column chunk**:

```text
Parquet file

Row group 0
  id column chunk
  name column chunk
  depth column chunk
  geometry column chunk

Row group 1
  id column chunk
  name column chunk
  depth column chunk
  geometry column chunk

...

Footer metadata
```

This is the fundamental shape behind projection and read amplification.

## Row groups

A row group is a horizontal collection of rows and the main unit that divides a Parquet file for many reading and writing decisions.

If a file has 10 million rows and four equally sized row groups, each row group contains roughly 2.5 million rows.

Large row groups have benefits:

- fewer metadata structures;
- good compression opportunities;
- efficient large analytical scans.

But they can be inconvenient for interactive access. If the first 1,000 requested rows live in a row group whose geometry column chunk is hundreds of megabytes, a reader may need significant I/O before returning that small result.

There is no universally correct row-group size. It depends on workload.

## Column chunks

Within each row group, each Parquet leaf column has a column chunk. Column chunks carry metadata such as compressed/uncompressed size, codec, encodings, offsets, and optional statistics.

Column projection is powerful because a query that requests:

```text
id, name
```

can potentially avoid reading the much larger:

```text
geometry
```

chunk entirely.

This is why a viewer should treat visible/projected columns as part of query planning, not merely hide unwanted values after reading every column.

Nested schemas can produce more physical **leaf columns** than the number of top-level fields visible to a user. Physical analysis therefore often talks about leaf paths rather than only top-level names.

## Pages

A column chunk is further divided into **pages**. Pages are encoded/compressed units containing a portion of the column's values.

Conceptually:

```text
geometry column chunk
  page 0
  page 1
  page 2
  ...
```

Pages can make finer-grained reads/skipping possible, but only when the reader has enough location/statistics information and its execution path can take advantage of it.

A useful caution is that the existence of pages does not automatically mean a remote query fetches only the exact pages covering its first 1,000 logical rows. The actual behavior depends on indexes, reader planning, encodings, and storage access patterns.

## Footer metadata

Parquet stores important metadata near the end of the file. A remote reader commonly begins with a small range request near the end to discover the footer and then obtains the metadata needed to interpret the rest of the object.

The footer describes row groups and column chunks, including locations/sizes and schema information. That is why a viewer can expose substantial physical-layout analysis without downloading all table data.

A typical remote open is therefore closer to:

```text
small read near end of object
  ↓
parse footer metadata
  ↓
know row groups, column chunks, schema, statistics
```

than to "download the file, then inspect it."

## Compression and encoding

Parquet applies **encodings** to values and may then compress pages/chunks with codecs such as ZSTD, Snappy, or others.

These are related but different concepts:

- encoding describes how values are represented (for example dictionary or plain encoding);
- compression reduces the encoded byte stream.

Physical metadata frequently provides both compressed and uncompressed sizes. Their ratio is useful for understanding storage efficiency, but it does not directly predict CPU time: different data types, encodings, and codecs have different decode costs.

## Statistics

Parquet may store statistics such as minimum, maximum, null count, and related values for row groups/pages.

For a predicate such as:

```text
depth > 5000
```

if a row group's maximum depth is 200, that row group cannot match and a reader may be able to skip its relevant data.

Statistics are optional and type-dependent. Their absence is not a corrupt-file signal; it means less information is available for pruning.

Statistics can also be unsuitable for some value types. A huge WKB geometry blob, for example, does not become spatially searchable simply because it is stored as a binary Parquet column.

## Page indexes

Modern Parquet files may contain **ColumnIndex** and **OffsetIndex** structures.

At a high level:

- the ColumnIndex can describe page-level statistics/null information;
- the OffsetIndex describes where pages are located and which rows they begin with.

These indexes can allow a reader or inspection tool to understand pages without scanning every page payload.

They are optional. When they are absent, a portable viewer should not assume it can cheaply reconstruct the same information. Scanning a huge remote file merely to populate a "Stats" panel would defeat the purpose of metadata-first exploration.

## Predicate pushdown and pruning

"Pushdown" is often used broadly for moving filtering work closer to the storage/read layer. In Parquet, several levels may be involved:

1. use metadata/statistics to skip entire row groups;
2. use page indexes/statistics to skip pages where supported;
3. decode necessary values and evaluate row-level predicates.

These mechanisms reduce work only when the file contains useful metadata and the query can exploit it.

A filter column may need to be read even when it is not part of the returned projection because the reader needs its values to determine which rows match.

## Why LIMIT can still be expensive

Consider this request:

```text
columns: all 22
limit:   1,000 rows
```

Suppose the first row group contains 2.5 million rows and its projected column chunks total 648 MiB, mostly a large geometry chunk.

The logical result is small, but the physical reader may still touch a large amount of compressed data before it can produce the first RecordBatch.

This is **read amplification**:

```text
large physical input
        ↓
small logical result
```

For remote data, this can dominate latency even when Parquet decoding and browser rendering are extremely fast.

The viewer therefore measures storage bytes/time separately from Parquet reader work and browser geometry decoding.

## Remote Parquet and range requests

HTTP/object stores commonly support byte-range reads. A Parquet reader can use metadata offsets to ask for selected regions of a remote object instead of downloading it sequentially from byte zero.

This is particularly useful for columnar projection:

```text
request metadata
request id chunk range
request name chunk range
skip geometry chunk
```

Some object-store interfaces can combine multiple requested ranges into one higher-level call. Diagnostic terms such as **object-store calls**, **ranges**, and **bytes** are therefore distinct:

- one call may contain many ranges;
- many ranges may collectively request a very large number of bytes;
- low call count does not imply low I/O volume.

That distinction was important in a real viewer trace where one object-store call contained 22 ranges totaling hundreds of MiB.

## Arrow versus Parquet

Parquet is primarily a storage format. Arrow is an in-memory columnar representation and IPC format.

In this viewer the backend pipeline is approximately:

```text
remote Parquet bytes
  ↓
Parquet decoding
  ↓
Arrow RecordBatch
  ↓
Arrow IPC stream
  ↓
browser Arrow decoder
```

Using Arrow IPC avoids converting typed columnar batches into a large intermediate JSON row representation on the server.

Parquet and Arrow schemas are related but not identical. The reader is responsible for interpreting Parquet's physical/logical representation and producing Arrow arrays appropriate for downstream use.

## Geometry and GeoParquet

Geometry is commonly large and variable-width, so it can dominate a Parquet file even when ordinary scalar columns are small.

GeoParquet adds metadata and conventions that identify geometry columns and spatial information. Newer Parquet logical types can also represent geometry/geography more explicitly.

There are two very different "geometry costs" to keep separate:

1. **reading the geometry column chunk from Parquet** — potentially hundreds of MiB of remote I/O;
2. **decoding WKB into browser geometry objects** — CPU work after the Arrow bytes arrive.

A slow map/table request should measure both. Guessing based on the word "geometry" can lead to optimizing the wrong layer.

## What makes a Parquet file viewer-friendly?

There is no single recipe, but interactive exploration generally benefits when:

- row groups are not excessively large for the expected access pattern;
- commonly viewed columns do not force unrelated large columns to be read;
- useful statistics/indexes exist for common filters;
- geometry-heavy layouts are chosen with spatial access patterns in mind;
- compression provides a reasonable storage/CPU tradeoff;
- the remote store supports efficient range access.

A file can be perfectly valid Parquet and still be poorly matched to an interactive workload. The analysis API describes those characteristics; it intentionally does not label the file itself as "good" or "bad" in an absolute sense.

## Relating this to the viewer

The project documentation splits these concerns deliberately:

- [architecture.md](architecture.md) explains the system boundaries;
- [engine.md](engine.md) explains how the Rust service turns ranged Parquet reads into Arrow streams;
- [analysis.md](analysis.md) explains how the viewer interprets physical metadata;
- [frontend.md](frontend.md) explains what happens after Arrow reaches the browser.

Together they provide enough background to reason about a query trace without requiring a full reading of the Parquet specification or the source code of the underlying libraries.
