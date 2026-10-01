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
use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions};
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

/// Parsed, reusable storage location for one opened dataset.
///
/// Keeping the object-store client and path alive avoids reparsing the URI and
/// rebuilding the remote client for every table, filter, map, or analysis call.
#[derive(Clone)]
pub(super) struct DatasetSource {
    store: Arc<dyn ObjectStore>,
    path: Path,
    uri: String,
}

impl DatasetSource {
    pub(super) fn reader(&self, metrics: ReadMetrics) -> ObjectStoreReader {
        ObjectStoreReader::new(Arc::clone(&self.store), self.path.clone(), metrics)
    }

    pub(super) fn builder(
        &self,
        metadata: &ArrowReaderMetadata,
        metrics: ReadMetrics,
    ) -> ReaderBuilder {
        ParquetRecordBatchStreamBuilder::new_with_metadata(self.reader(metrics), metadata.clone())
    }

    pub(super) async fn builder_with_options(
        &self,
        options: ArrowReaderOptions,
    ) -> Result<(ReaderBuilder, ReadMetrics)> {
        let metrics = ReadMetrics::default();
        let reader = self.reader(metrics.clone());
        let builder = ParquetRecordBatchStreamBuilder::new_with_options(reader, options)
            .await
            .with_context(|| format!("could not open Parquet metadata: {}", self.uri))?;
        Ok((builder, metrics))
    }
}

/// Parse a source once and load the Arrow/Parquet metadata that will be cached
/// on the opened dataset handle.
pub(super) async fn open_source(
    uri: &str,
) -> Result<(DatasetSource, ArrowReaderMetadata, ReadMetrics)> {
    let url = Url::parse(uri).with_context(|| format!("invalid source URI: {uri}"))?;
    let (store, path) = object_store::parse_url(&url)
        .with_context(|| format!("unsupported or invalid object-store URI: {uri}"))?;
    let source = DatasetSource {
        store: Arc::from(store),
        path,
        uri: uri.to_string(),
    };
    let metrics = ReadMetrics::default();
    let mut reader = source.reader(metrics.clone());
    let metadata = ArrowReaderMetadata::load_async(&mut reader, ArrowReaderOptions::new())
        .await
        .with_context(|| format!("could not open Parquet metadata: {uri}"))?;
    Ok((source, metadata, metrics))
}
