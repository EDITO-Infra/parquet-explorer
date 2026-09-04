//! Row-oriented query execution for table pages, complete results, and counts.
//!
//! All query paths start from the cached `OpenedDataset` metadata and reusable
//! source client. Filter compilation and projection are kept in focused modules
//! so future row-group/page pruning can be inserted without coupling API features.

mod filter;
mod projection;
mod pruning;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use crate::model::{CountRequest, PageRequest, ResultRequest};

use super::{
    CoreEngine,
    dataset::OpenedDataset,
    ipc::{IpcByteStream, spawn_ipc_stream},
    source::{ReadMetrics, ReaderBuilder},
    trace::TraceReporter,
};
use filter::apply_filters;
use projection::apply_projection;
use pruning::prune_row_groups;

impl CoreEngine {
    /// Stream the complete projected result, optionally filtered, without paging.
    pub async fn result_stream(
        &self,
        dataset_id: &str,
        req: ResultRequest,
    ) -> Result<IpcByteStream> {
        let trace = self
            .traces
            .start(req.trace_id.as_deref(), "result", "Preparing complete result");
        let dataset = self.dataset(dataset_id)?;
        if let Some(trace) = &trace {
            trace.event("dataset_found", "Dataset handle found");
        }
        let (mut builder, read_metrics) = builder_with_diagnostics(&dataset, trace.as_ref());
        let pruning = prune_row_groups(&dataset, &req.filters)?;
        if !req.filters.is_empty() {
            if let Some(trace) = &trace {
                trace.event_with_detail(
                    "attribute_pruning",
                    "Pruning row groups with column statistics",
                    format!(
                        "{} of {} row groups selected · {} filters pruned row groups",
                        pruning.row_groups.len(),
                        dataset.reader_metadata.metadata().num_row_groups(),
                        pruning.filters_pruning_row_groups
                    ),
                );
            }
            builder = builder.with_row_groups(pruning.row_groups);
        }
        let builder = apply_filters(builder, &req.filters)?;
        let builder = apply_projection(builder, req.columns.as_deref())?
            .with_batch_size(self.batch_size);
        if let Some(trace) = &trace {
            trace.event("reader_ready", "Reader plan ready");
        }
        let stream = builder
            .build()
            .context("could not build complete result reader")?;
        let schema = Arc::clone(stream.schema());
        Ok(spawn_ipc_stream(
            stream,
            schema,
            self.ipc_channel_capacity,
            trace,
            Some(read_metrics),
        ))
    }

    /// Count matching rows. Unfiltered counts use cached footer metadata;
    /// filtered counts scan only predicate columns plus one output column.
    pub async fn count_rows(&self, dataset_id: &str, req: CountRequest) -> Result<u64> {
        let trace = self
            .traces
            .start(req.trace_id.as_deref(), "count", "Counting matching rows");
        let dataset = self.dataset(dataset_id)?;
        if req.filters.is_empty() {
            let count = u64::try_from(dataset.info.num_rows)
                .context("dataset row count cannot be represented as u64")?;
            if let Some(trace) = &trace {
                trace.finish_with_detail("Count ready", format!("{count} rows"));
            }
            return Ok(count);
        }

        if let Some(trace) = &trace {
            trace.event("metadata_cache", "Reusing cached Parquet metadata");
        }
        let metrics = ReadMetrics::default();
        let mut builder = dataset.reader_builder(metrics);
        let pruning = prune_row_groups(&dataset, &req.filters)?;
        if let Some(trace) = &trace {
            trace.event_with_detail(
                "attribute_pruning",
                "Pruning row groups with column statistics",
                format!(
                    "{} of {} row groups selected · {} filters pruned row groups",
                    pruning.row_groups.len(),
                    dataset.reader_metadata.metadata().num_row_groups(),
                    pruning.filters_pruning_row_groups
                ),
            );
        }
        builder = builder.with_row_groups(pruning.row_groups);
        let builder = apply_filters(builder, &req.filters)?;

        let count_projection = dataset
            .info
            .columns
            .first()
            .map(|column| vec![column.name.clone()]);
        let builder = apply_projection(builder, count_projection.as_deref())?
            .with_batch_size(self.batch_size);
        let mut stream = builder
            .build()
            .context("could not build filtered count reader")?;

        use futures::TryStreamExt;
        let mut total = 0u64;
        while let Some(batch) = stream
            .try_next()
            .await
            .context("filtered count read failed")?
        {
            total = total
                .checked_add(batch.num_rows() as u64)
                .ok_or_else(|| anyhow!("filtered row count overflow"))?;
        }
        if let Some(trace) = &trace {
            trace.finish_with_detail("Count ready", format!("{total} rows"));
        }
        Ok(total)
    }

    /// Stream a projected/filtered offset+limit row window as Arrow IPC.
    pub async fn page_stream(&self, dataset_id: &str, req: PageRequest) -> Result<IpcByteStream> {
        let trace = self
            .traces
            .start(req.trace_id.as_deref(), "page", "Preparing data query");
        let dataset = self.dataset(dataset_id)?;
        if let Some(trace) = &trace {
            trace.event("dataset_found", "Dataset handle found");
        }
        let (mut builder, read_metrics) = builder_with_diagnostics(&dataset, trace.as_ref());
        let pruning = prune_row_groups(&dataset, &req.filters)?;
        if !req.filters.is_empty() {
            if let Some(trace) = &trace {
                trace.event_with_detail(
                    "attribute_pruning",
                    "Pruning row groups with column statistics",
                    format!(
                        "{} of {} row groups selected · {} filters pruned row groups",
                        pruning.row_groups.len(),
                        dataset.reader_metadata.metadata().num_row_groups(),
                        pruning.filters_pruning_row_groups
                    ),
                );
            }
            builder = builder.with_row_groups(pruning.row_groups);
        }
        let builder = apply_filters(builder, &req.filters)?;
        if let Some(trace) = &trace {
            let column_count = req
                .columns
                .as_ref()
                .map_or(dataset.info.columns.len(), Vec::len);
            trace.event_with_detail(
                "plan_query",
                "Planning columns, filters, and row window",
                format!(
                    "{} columns · offset {} · limit {} · {} filters",
                    column_count,
                    req.offset,
                    req.limit,
                    req.filters.len()
                ),
            );
        }
        let builder = apply_projection(builder, req.columns.as_deref())?
            .with_offset(req.offset)
            .with_limit(req.limit)
            .with_batch_size(self.batch_size);
        let stream = builder
            .build()
            .context("could not build Parquet page reader")?;
        if let Some(trace) = &trace {
            trace.event("reader_ready", "Parquet reader ready");
        }
        let schema = Arc::clone(stream.schema());
        Ok(spawn_ipc_stream(
            stream,
            schema,
            self.ipc_channel_capacity,
            trace,
            Some(read_metrics),
        ))
    }
}

fn builder_with_diagnostics(
    dataset: &OpenedDataset,
    trace: Option<&TraceReporter>,
) -> (ReaderBuilder, ReadMetrics) {
    let metrics = ReadMetrics::default();
    let builder = dataset.reader_builder(metrics.clone());
    if let Some(trace) = trace {
        trace.event_with_detail(
            "metadata_cache",
            "Reusing cached Parquet footer and schema",
            format!(
                "{} row groups · no metadata storage read",
                dataset.reader_metadata.metadata().num_row_groups()
            ),
        );
    }
    (builder, metrics)
}

pub(super) use filter::apply_filters as apply_query_filters;
pub(super) use projection::apply_projection as apply_query_projection;
pub(super) use pruning::prune_row_groups as prune_query_row_groups;
