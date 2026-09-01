//! Object-store backed async Parquet reader and storage diagnostics.
//!
//! `ObjectStoreReader` adapts `object_store` range reads to Parquet-RS'
//! `AsyncFileReader`. Every awaited storage call is counted and timed so query
//! diagnostics can separate remote I/O from decompression/Parquet decoding.

use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use bytes::Bytes;
use futures::FutureExt;
use futures::future::BoxFuture;
use object_store::path::Path;
use object_store::{GetOptions, GetRange, ObjectStore, ObjectStoreExt};
use parquet::arrow::ParquetRecordBatchStreamBuilder;
use parquet::arrow::arrow_reader::ArrowReaderOptions;
use parquet::arrow::async_reader::{AsyncFileReader, MetadataSuffixFetch};
use parquet::errors::{ParquetError, Result as ParquetResult};
use parquet::file::metadata::{ParquetMetaData, ParquetMetaDataReader};
use url::Url;

/// Object-store I/O counters shared with the Parquet reader. These timings are
/// intentionally measured around the actual object_store awaits so diagnostics
/// can distinguish remote storage wait from the rest of Parquet-RS reader work.
#[derive(Clone, Default)]
pub struct ReadMetrics {
    inner: Arc<ReadMetricsInner>,
}

#[derive(Default)]
struct ReadMetricsInner {
    calls: AtomicU64,
    ranges: AtomicU64,
    requested_bytes: AtomicU64,
    returned_bytes: AtomicU64,
    io_nanos: AtomicU64,
}

#[derive(Clone, Copy, Default)]
pub struct ReadMetricsSnapshot {
    pub calls: u64,
    pub ranges: u64,
    pub requested_bytes: u64,
    pub returned_bytes: u64,
    pub io_ms: u64,
}

impl ReadMetrics {
    fn record(
        &self,
        calls: u64,
        ranges: u64,
        requested_bytes: u64,
        returned_bytes: u64,
        duration: Duration,
    ) {
        self.inner.calls.fetch_add(calls, Ordering::Relaxed);
        self.inner.ranges.fetch_add(ranges, Ordering::Relaxed);
        self.inner
            .requested_bytes
            .fetch_add(requested_bytes, Ordering::Relaxed);
        self.inner
            .returned_bytes
            .fetch_add(returned_bytes, Ordering::Relaxed);
        self.inner.io_nanos.fetch_add(
            u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    pub fn snapshot(&self) -> ReadMetricsSnapshot {
        ReadMetricsSnapshot {
            calls: self.inner.calls.load(Ordering::Relaxed),
            ranges: self.inner.ranges.load(Ordering::Relaxed),
            requested_bytes: self.inner.requested_bytes.load(Ordering::Relaxed),
            returned_bytes: self.inner.returned_bytes.load(Ordering::Relaxed),
            io_ms: self.inner.io_nanos.load(Ordering::Relaxed) / 1_000_000,
        }
    }

    /// Start a fresh accounting window. Builder creation has already completed
    /// its footer/schema reads before this is called, so subsequent counters are
    /// data-column reads performed while polling RecordBatches.
    pub fn reset(&self) {
        self.inner.calls.store(0, Ordering::Relaxed);
        self.inner.ranges.store(0, Ordering::Relaxed);
        self.inner.requested_bytes.store(0, Ordering::Relaxed);
        self.inner.returned_bytes.store(0, Ordering::Relaxed);
        self.inner.io_nanos.store(0, Ordering::Relaxed);
    }
}

/// Minimal `object_store` adapter for Parquet-RS' current async reader API.
///
/// Parquet-RS 59.2 deprecated its built-in `ParquetObjectReader` integration in
/// favor of user-defined `AsyncFileReader` implementations. Keeping this
/// adapter here lets the backend use HTTP/S3/Azure/GCS/local range reads without
/// coupling the engine to Parquet's deprecated object_store feature.
#[derive(Clone)]
pub struct ObjectStoreReader {
    store: Arc<dyn ObjectStore>,
    path: Path,
    metrics: ReadMetrics,
}

impl ObjectStoreReader {
    fn new(store: Arc<dyn ObjectStore>, path: Path, metrics: ReadMetrics) -> Self {
        Self {
            store,
            path,
            metrics,
        }
    }
}

fn to_parquet_err(error: object_store::Error) -> ParquetError {
    ParquetError::External(Box::new(error))
}

impl AsyncFileReader for ObjectStoreReader {
    fn get_bytes(&mut self, range: Range<u64>) -> BoxFuture<'_, ParquetResult<Bytes>> {
        let requested = range.end.saturating_sub(range.start);
        let metrics = self.metrics.clone();
        async move {
            let started = Instant::now();
            let result = self
                .store
                .get_range(&self.path, range)
                .await
                .map_err(to_parquet_err);
            let returned = result.as_ref().map_or(0, |bytes| bytes.len() as u64);
            metrics.record(1, 1, requested, returned, started.elapsed());
            result
        }
        .boxed()
    }

    fn get_byte_ranges(
        &mut self,
        ranges: Vec<Range<u64>>,
    ) -> BoxFuture<'_, ParquetResult<Vec<Bytes>>> {
        let range_count = ranges.len() as u64;
        let requested = ranges
            .iter()
            .map(|range| range.end.saturating_sub(range.start))
            .sum();
        let metrics = self.metrics.clone();
        async move {
            let started = Instant::now();
            let result = self
                .store
                .get_ranges(&self.path, &ranges)
                .await
                .map_err(to_parquet_err);
            let returned = result.as_ref().map_or(0, |chunks| {
                chunks.iter().map(|bytes| bytes.len() as u64).sum()
            });
            metrics.record(1, range_count, requested, returned, started.elapsed());
            result
        }
        .boxed()
    }

    fn get_metadata<'a>(
        &'a mut self,
        options: Option<&'a ArrowReaderOptions>,
    ) -> BoxFuture<'a, ParquetResult<Arc<ParquetMetaData>>> {
        async move {
            let metadata = ParquetMetaDataReader::new()
                .with_arrow_reader_options(options)
                .load_via_suffix_and_finish(self)
                .await?;
            Ok(Arc::new(metadata))
        }
        .boxed()
    }
}

/// Allow the Parquet metadata reader to fetch the footer with a suffix request,
/// avoiding a preliminary whole-object read (and, where supported, an extra
/// HEAD request just to discover the file size).
impl MetadataSuffixFetch for &mut ObjectStoreReader {
    fn fetch_suffix(&mut self, suffix: usize) -> BoxFuture<'_, ParquetResult<Bytes>> {
        let options = GetOptions {
            range: Some(GetRange::Suffix(suffix as u64)),
            ..Default::default()
        };
        let metrics = self.metrics.clone();

        async move {
            let started = Instant::now();
            let result = async {
                let response = self
                    .store
                    .get_opts(&self.path, options)
                    .await
                    .map_err(to_parquet_err)?;
                response.bytes().await.map_err(to_parquet_err)
            }
            .await;
            let returned = result.as_ref().map_or(0, |bytes| bytes.len() as u64);
            metrics.record(1, 1, suffix as u64, returned, started.elapsed());
            result
        }
        .boxed()
    }
}

pub type ReaderBuilder = ParquetRecordBatchStreamBuilder<ObjectStoreReader>;

pub async fn builder_for_uri(uri: &str) -> Result<ReaderBuilder> {
    let (builder, _) = builder_for_uri_with_metrics(uri).await?;
    Ok(builder)
}

/// Open a Parquet source using the reader's default metadata options.
///
/// The returned [`ReadMetrics`] belongs to the underlying object-store reader,
/// so callers can distinguish storage wait from Parquet decoding/reader work.
pub async fn builder_for_uri_with_metrics(uri: &str) -> Result<(ReaderBuilder, ReadMetrics)> {
    builder_for_uri_with_options(uri, None).await
}

/// Open a Parquet source with explicit Arrow metadata-reading options.
///
/// This is primarily used by the read-only analysis endpoints. Normal data
/// queries keep page-index loading disabled because fetching page indexes is
/// extra work that is not required to display a basic page of rows. The
/// `/analysis/pages` endpoint opts in explicitly so it can inspect page
/// locations without reading data-page payloads.
pub async fn builder_for_uri_with_options(
    uri: &str,
    options: Option<ArrowReaderOptions>,
) -> Result<(ReaderBuilder, ReadMetrics)> {
    let url = Url::parse(uri).with_context(|| format!("invalid source URI: {uri}"))?;
    let (store, path) = object_store::parse_url(&url)
        .with_context(|| format!("unsupported or invalid object-store URI: {uri}"))?;
    let store: Arc<dyn ObjectStore> = Arc::from(store);
    let metrics = ReadMetrics::default();
    let reader = ObjectStoreReader::new(store, path, metrics.clone());

    let builder = match options {
        Some(options) => ParquetRecordBatchStreamBuilder::new_with_options(reader, options).await,
        None => ParquetRecordBatchStreamBuilder::new(reader).await,
    }
    .with_context(|| format!("could not open Parquet metadata: {uri}"))?;

    Ok((builder, metrics))
}
