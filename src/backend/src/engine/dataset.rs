//! Opened dataset execution context and dataset lifecycle operations.

use std::{sync::Arc, time::Instant};

use anyhow::Result;
use parquet::arrow::arrow_reader::ArrowReaderMetadata;
use uuid::Uuid;

use crate::model::DatasetInfo;

use super::{
    CoreEngine,
    metadata::build_dataset_info,
    source::{DatasetSource, ReadMetrics, ReaderBuilder, open_source},
};

/// Immutable execution context shared by all operations for one opened source.
///
/// The source client and Parquet/Arrow metadata are loaded once. Query, spatial,
/// export, and analysis features construct cheap readers from this cached state
/// instead of reparsing the URI and rereading the footer for every request.
pub(super) struct OpenedDataset {
    pub(super) info: DatasetInfo,
    pub(super) source: DatasetSource,
    pub(super) reader_metadata: ArrowReaderMetadata,
}

impl OpenedDataset {
    pub(super) fn reader_builder(&self, metrics: ReadMetrics) -> ReaderBuilder {
        self.source.builder(&self.reader_metadata, metrics)
    }
}

impl CoreEngine {
    /// Validate that a Parquet source is readable, cache its execution metadata,
    /// and register a process-local dataset handle.
    pub async fn open_dataset(
        &self,
        uri: &str,
        name: Option<String>,
        trace_id: Option<&str>,
    ) -> Result<DatasetInfo> {
        let trace = self.traces.reporter(trace_id);
        self.registry.ensure_capacity()?;

        if let Some(trace) = &trace {
            trace.event("query_parquet", "Reading and caching Parquet metadata");
        }
        let started = Instant::now();
        let (source, reader_metadata, metrics) = match open_source(uri).await {
            Ok(opened) => opened,
            Err(error) => {
                if let Some(trace) = &trace {
                    trace.fail("Could not read Parquet metadata");
                }
                return Err(error);
            }
        };
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let storage = metrics.snapshot();

        if let Some(trace) = &trace {
            trace.event_with_duration(
                "metadata_storage",
                "Fetching Parquet metadata from storage",
                storage.io_ms,
                format!(
                    "{} object-store calls · {} ranges · {} bytes fetched",
                    storage.calls, storage.ranges, storage.returned_bytes
                ),
            );
            trace.event_with_duration(
                "metadata_decode",
                "Parsing and caching Parquet metadata",
                elapsed_ms.saturating_sub(storage.io_ms),
                "Cached for reuse by later dataset operations",
            );
            trace.event_with_detail(
                "parquet_ready",
                "Parquet metadata cached",
                format!("{} row groups", reader_metadata.metadata().num_row_groups()),
            );
        }

        let dataset_id = Uuid::new_v4().to_string();
        let info = build_dataset_info(&dataset_id, name, uri, &reader_metadata)?;
        let dataset = Arc::new(OpenedDataset {
            info: info.clone(),
            source,
            reader_metadata,
        });
        self.registry.insert(dataset)?;

        if let Some(trace) = &trace {
            trace.finish_with_detail(
                "Dataset ready",
                format!("{} rows · {} columns", info.num_rows, info.columns.len()),
            );
        }
        Ok(info)
    }

    /// Return the stable public metadata cached when the dataset was opened.
    pub fn metadata(&self, dataset_id: &str) -> Result<DatasetInfo> {
        Ok(self.dataset(dataset_id)?.info.clone())
    }

    pub fn dataset_cleanup_interval(&self) -> Option<std::time::Duration> {
        self.registry.cleanup_interval()
    }

    pub fn prune_expired_datasets(&self) -> usize {
        self.registry.prune_expired()
    }

    pub fn close_dataset(&self, dataset_id: &str) -> Result<()> {
        self.registry.remove(dataset_id)
    }
}
