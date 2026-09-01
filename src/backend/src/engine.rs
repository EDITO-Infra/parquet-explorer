//! Core read-only dataset/query engine.
//!
//! A dataset handle stores lightweight metadata and a validated source URI.
//! Individual queries create Parquet-RS readers against that URI, apply native
//! projection/filter/row-group selection, and stream Arrow IPC to the browser.
//! Physical-layout inspection lives in `engine::analysis`; request diagnostics
//! live in `engine::trace` and object-store timing in `engine::source`.

mod analysis;
mod filter;
mod ipc;
mod source;
mod trace;

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use parking_lot::RwLock;
use parquet::{
    arrow::{ProjectionMask, arrow_writer::ArrowWriter},
    basic::LogicalType,
    file::metadata::ParquetMetaData,
};
use serde::Deserialize;
use tempfile::{NamedTempFile, TempPath};
use uuid::Uuid;

use crate::model::{
    AnalysisRecommendationsResponse, AnalysisSummaryResponse, Capabilities, ColumnInfo,
    ColumnsAnalysisResponse, CountRequest, DatasetEntry, DatasetInfo, ExportFormat, ExportRequest,
    GeoColumnInfo, GeoParquetInfo, PageRequest, PagesAnalysisQuery, PagesAnalysisResponse,
    QueryCostRequest, QueryCostResponse, RowGroupsAnalysisResponse, ResultRequest, SpatialRequest,
    TraceSnapshot,
};
use filter::apply_filters;
use ipc::{IpcByteStream, spawn_ipc_stream};
use source::{ReadMetrics, ReaderBuilder, builder_for_uri, builder_for_uri_with_metrics};
use trace::{TraceReporter, TraceStore};

/// Temporary file plus HTTP metadata returned by an export operation.
pub struct ExportArtifact {
    pub path: TempPath,
    pub content_type: &'static str,
    pub filename: &'static str,
}

/// Shared native engine state.
///
/// Dataset handles are kept in memory and point at validated source URIs. The
/// source data itself is never copied into this structure; each query performs
/// ranged reads directly against the original Parquet object.
pub struct CoreEngine {
    datasets: RwLock<HashMap<String, DatasetEntry>>,
    batch_size: usize,
    ipc_channel_capacity: usize,
    max_open_datasets: usize,
    /// How long an unused process-local dataset handle remains valid.
    /// `None` means expiration was explicitly disabled by configuration.
    dataset_idle_timeout: Option<Duration>,
    traces: TraceStore,
}

impl CoreEngine {
    /// Construct an engine with fixed query limits and dataset-handle lifetime.
    ///
    /// `dataset_idle_timeout_seconds = 0` disables idle expiration. In normal
    /// deployments a finite timeout is preferred because a browser can vanish
    /// without sending `DELETE /datasets/{id}`.
    pub fn new(
        batch_size: usize,
        max_open_datasets: usize,
        dataset_idle_timeout_seconds: u64,
    ) -> Self {
        Self {
            datasets: RwLock::new(HashMap::new()),
            batch_size,
            ipc_channel_capacity: 4,
            max_open_datasets,
            dataset_idle_timeout: (dataset_idle_timeout_seconds > 0)
                .then(|| Duration::from_secs(dataset_idle_timeout_seconds)),
            traces: TraceStore::default(),
        }
    }

    /// Validate that a Parquet source is readable, extract viewer metadata, and
    /// register a lightweight in-memory dataset handle.
    pub async fn open_dataset(
        &self,
        uri: &str,
        name: Option<String>,
        trace_id: Option<&str>,
    ) -> Result<DatasetInfo> {
        let trace = self.traces.reporter(trace_id);

        // A browser may disappear without explicitly closing its handle. Drop
        // idle entries before applying the capacity limit so abandoned sessions
        // cannot permanently consume all available handle slots.
        self.prune_expired_datasets();
        if self.datasets.read().len() >= self.max_open_datasets {
            if let Some(trace) = &trace { trace.fail("Too many datasets are already open"); }
            bail!("maximum number of open dataset handles reached");
        }

        if let Some(trace) = &trace {
            trace.event("query_parquet", "Querying Parquet metadata");
        }
        let builder = match builder_for_uri(uri).await {
            Ok(builder) => builder,
            Err(error) => {
                if let Some(trace) = &trace { trace.fail("Could not read Parquet metadata"); }
                return Err(error);
            }
        };
        if let Some(trace) = &trace {
            trace.event_with_detail(
                "parquet_ready",
                "Parquet exists and metadata is readable",
                format!("{} row groups", builder.metadata().num_row_groups()),
            );
            trace.event("read_schema", "Reading schema and file statistics");
        }

        let dataset_id = Uuid::new_v4().to_string();
        let info = build_dataset_info(&dataset_id, name, uri, &builder)?;

        let mut datasets = self.datasets.write();
        Self::prune_expired_locked(&mut datasets, self.dataset_idle_timeout, Instant::now());
        if datasets.len() >= self.max_open_datasets {
            if let Some(trace) = &trace { trace.fail("Too many datasets are already open"); }
            bail!("maximum number of open dataset handles reached");
        }
        let now = Instant::now();
        datasets.insert(
            dataset_id,
            DatasetEntry {
                info: info.clone(),
                opened_at: now,
                last_accessed_at: now,
            },
        );
        if let Some(trace) = &trace {
            trace.finish_with_detail(
                "Dataset ready",
                format!("{} rows · {} columns", info.num_rows, info.columns.len()),
            );
        }
        Ok(info)
    }

    /// Start a request trace when the caller supplied a non-empty trace ID.
    pub fn start_trace(&self, trace_id: Option<&str>, operation: &str, message: &str) {
        self.traces.start(trace_id, operation, message);
    }

    /// Append a milestone to an existing trace.
    pub fn trace_event(&self, trace_id: Option<&str>, stage: &str, message: &str) {
        if let Some(trace) = self.traces.reporter(trace_id) {
            trace.event(stage, message);
        }
    }

    /// Mark an existing trace as failed.
    pub fn trace_fail(&self, trace_id: Option<&str>, message: &str) {
        if let Some(trace) = self.traces.reporter(trace_id) {
            trace.fail(message);
        }
    }

    /// Return the current immutable diagnostics snapshot for polling clients.
    pub fn trace_snapshot(&self, trace_id: &str) -> Option<TraceSnapshot> {
        self.traces.snapshot(trace_id)
    }

    /// Return the lightweight metadata cached when the dataset was opened.
    ///
    /// Resolving the handle counts as activity and refreshes its idle lifetime.
    pub fn metadata(&self, dataset_id: &str) -> Result<DatasetInfo> {
        Ok(self.entry(dataset_id)?.info)
    }

    /// Return a cheap physical-layout summary derived from the Parquet footer.
    ///
    /// This does not read row data and never modifies the source object.
    pub async fn analysis_summary(&self, dataset_id: &str) -> Result<AnalysisSummaryResponse> {
        let entry = self.entry(dataset_id)?;
        analysis::summary(
            dataset_id,
            &entry.info.uri,
            entry.info.columns.len(),
        )
        .await
    }

    /// Aggregate storage, encoding, compression and statistics information for
    /// each Parquet leaf column.
    pub async fn analysis_columns(&self, dataset_id: &str) -> Result<ColumnsAnalysisResponse> {
        let entry = self.entry(dataset_id)?;
        analysis::columns(dataset_id, &entry.info.uri).await
    }

    /// Describe every row group and its column chunks from footer metadata.
    pub async fn analysis_row_groups(&self, dataset_id: &str) -> Result<RowGroupsAnalysisResponse> {
        let entry = self.entry(dataset_id)?;
        analysis::row_groups(dataset_id, &entry.info.uri).await
    }

    /// Load optional Parquet page indexes and expose their page locations.
    ///
    /// This may issue small additional storage reads for page-index structures,
    /// but it does not read the underlying data-page payloads.
    pub async fn analysis_pages(
        &self,
        dataset_id: &str,
        query: PagesAnalysisQuery,
    ) -> Result<PagesAnalysisResponse> {
        let entry = self.entry(dataset_id)?;
        analysis::pages(dataset_id, &entry.info.uri, query).await
    }

    /// Estimate compressed bytes touched by a row query without executing it.
    pub async fn analysis_query_cost(
        &self,
        dataset_id: &str,
        request: QueryCostRequest,
    ) -> Result<QueryCostResponse> {
        let entry = self.entry(dataset_id)?;
        analysis::query_cost(dataset_id, &entry.info.uri, request).await
    }

    /// Generate read-only observations about characteristics that can affect
    /// interactive browser access. No rewrite or optimization is performed.
    pub async fn analysis_recommendations(
        &self,
        dataset_id: &str,
    ) -> Result<AnalysisRecommendationsResponse> {
        let entry = self.entry(dataset_id)?;
        analysis::recommendations(dataset_id, &entry.info.uri, &entry.info).await
    }

    /// Return the cadence used by the background handle-cleanup task.
    ///
    /// Handle lookup itself enforces the exact idle timeout. The periodic task
    /// exists only to release abandoned entries even when no later request
    /// happens to touch them. Capping cleanup at one minute keeps that task cheap
    /// while avoiding long-lived stale entries for normal timeout values.
    pub fn dataset_cleanup_interval(&self) -> Option<Duration> {
        self.dataset_idle_timeout.map(|timeout| {
            Duration::from_secs(timeout.as_secs().clamp(1, 60))
        })
    }

    /// Remove all dataset handles whose idle lifetime has elapsed.
    ///
    /// Returns the number removed so the server can emit a compact lifecycle log.
    /// No network requests are made and source Parquet objects are never touched.
    pub fn prune_expired_datasets(&self) -> usize {
        let mut datasets = self.datasets.write();
        Self::prune_expired_locked(&mut datasets, self.dataset_idle_timeout, Instant::now())
    }

    /// Drop an in-memory dataset handle. The source Parquet object is untouched.
    pub fn close_dataset(&self, dataset_id: &str) -> Result<()> {
        if self.datasets.write().remove(dataset_id).is_none() {
            bail!("dataset not found: {dataset_id}");
        }
        Ok(())
    }

    /// Stream the complete projected result, optionally filtered, without paging.
    ///
    /// This is intentionally separate from `page_stream`: filtered browser
    /// results and explicit All loads use one stream and paginate locally.
    pub async fn result_stream(
        &self,
        dataset_id: &str,
        req: ResultRequest,
    ) -> Result<IpcByteStream> {
        let trace = self.traces.start(
            req.trace_id.as_deref(),
            "result",
            "Preparing complete result",
        );
        let entry = self.entry(dataset_id)?;
        if let Some(trace) = &trace { trace.event("dataset_found", "Dataset handle found"); }
        let (builder, read_metrics) = builder_with_diagnostics(&entry.info.uri, trace.as_ref()).await?;
        if let Some(trace) = &trace {
            trace.event_with_detail(
                "parquet_ready",
                "Parquet metadata ready",
                format!("{} row groups", builder.metadata().num_row_groups()),
            );
        }
        let builder = apply_filters(builder, &req.filters)?;
        let builder = apply_projection(builder, req.columns.as_deref())?
            .with_batch_size(self.batch_size);
        if let Some(trace) = &trace { trace.event("reader_ready", "Reader plan ready"); }
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

    /// Count matching rows. Unfiltered counts use footer metadata; filtered
    /// counts scan only the predicate columns plus one projected output column.
    pub async fn count_rows(&self, dataset_id: &str, req: CountRequest) -> Result<u64> {
        let trace = self.traces.start(req.trace_id.as_deref(), "count", "Counting matching rows");
        let entry = self.entry(dataset_id)?;
        if req.filters.is_empty() {
            let count = u64::try_from(entry.info.num_rows)
                .context("dataset row count cannot be represented as u64")?;
            if let Some(trace) = &trace { trace.finish_with_detail("Count ready", format!("{count} rows")); }
            return Ok(count);
        }
        if let Some(trace) = &trace { trace.event("query_parquet", "Opening Parquet for filtered count"); }

        let builder = builder_for_uri(&entry.info.uri).await?;
        let builder = apply_filters(builder, &req.filters)?;

        // Counting still has to evaluate every matching row, but project the
        // output down to a single column so we do not materialize the entire
        // table merely to determine the exact filtered result count. Filter
        // columns are loaded independently by Parquet's RowFilter.
        let count_projection = entry
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
        while let Some(batch) = stream.try_next().await.context("filtered count read failed")? {
            total = total
                .checked_add(batch.num_rows() as u64)
                .ok_or_else(|| anyhow!("filtered row count overflow"))?;
        }
        if let Some(trace) = &trace { trace.finish_with_detail("Count ready", format!("{total} rows")); }
        Ok(total)
    }

    /// Stream a projected/filtered offset+limit row window as Arrow IPC.
    pub async fn page_stream(&self, dataset_id: &str, req: PageRequest) -> Result<IpcByteStream> {
        let trace = self.traces.start(req.trace_id.as_deref(), "page", "Preparing data query");
        let entry = self.entry(dataset_id)?;
        if let Some(trace) = &trace { trace.event("dataset_found", "Dataset handle found"); }
        let (builder, read_metrics) = builder_with_diagnostics(&entry.info.uri, trace.as_ref()).await?;
        if let Some(trace) = &trace {
            trace.event_with_detail(
                "parquet_ready",
                "Parquet metadata ready",
                format!("{} row groups available", builder.metadata().num_row_groups()),
            );
        }
        let builder = apply_filters(builder, &req.filters)?;
        if let Some(trace) = &trace {
            let column_count = req.columns.as_ref().map_or(entry.info.columns.len(), Vec::len);
            trace.event_with_detail(
                "plan_query",
                "Planning columns, filters, and row window",
                format!("{} columns · offset {} · limit {} · {} filters", column_count, req.offset, req.limit, req.filters.len()),
            );
        }
        let builder = apply_projection(builder, req.columns.as_deref())?
            .with_offset(req.offset)
            .with_limit(req.limit)
            .with_batch_size(self.batch_size);
        let stream = builder
            .build()
            .context("could not build Parquet page reader")?;
        if let Some(trace) = &trace { trace.event("reader_ready", "Parquet reader ready"); }
        let schema = Arc::clone(stream.schema());
        Ok(spawn_ipc_stream(
            stream,
            schema,
            self.ipc_channel_capacity,
            trace,
            Some(read_metrics),
        ))
    }

    /// Stream conservative spatial candidates selected using row-group bboxes.
    /// Exact feature-level geometry filtering is intentionally not claimed here.
    pub async fn spatial_stream(
        &self,
        dataset_id: &str,
        mut req: SpatialRequest,
    ) -> Result<IpcByteStream> {
        let trace = self.traces.start(req.trace_id.as_deref(), "spatial", "Preparing spatial query");
        validate_bbox(req.bbox)?;
        let entry = self.entry(dataset_id)?;
        if let Some(trace) = &trace { trace.event("dataset_found", "Dataset handle found"); }
        let geometry = choose_geometry_column(&entry.info, req.geometry_column.as_deref())?;
        let requested = req.columns.take().map(|mut columns| {
            if !columns.iter().any(|name| name == &geometry) {
                columns.push(geometry.clone());
            }
            columns
        });

        let (builder, read_metrics) = builder_with_diagnostics(&entry.info.uri, trace.as_ref()).await?;
        let total_row_groups = builder.metadata().num_row_groups();
        if let Some(trace) = &trace { trace.event("parquet_ready", "Parquet metadata ready"); }
        let row_groups = intersecting_row_groups(builder.metadata(), &geometry, req.bbox)?;
        if let Some(trace) = &trace {
            trace.event_with_detail(
                "prune_row_groups",
                "Pruning row groups with spatial statistics",
                format!("{} of {} row groups selected", row_groups.len(), total_row_groups),
            );
        }
        let builder = apply_filters(builder, &req.filters)?;
        let builder = apply_projection(builder, requested.as_deref())?
            .with_row_groups(row_groups)
            .with_limit(req.max_features)
            .with_batch_size(self.batch_size);
        let stream = builder
            .build()
            .context("could not build spatial Parquet reader")?;
        let schema = Arc::clone(stream.schema());
        Ok(spawn_ipc_stream(
            stream,
            schema,
            self.ipc_channel_capacity,
            trace,
            Some(read_metrics),
        ))
    }

    /// Materialize a read-only subset into a temporary Arrow or Parquet export.
    pub async fn export(&self, dataset_id: &str, req: ExportRequest) -> Result<ExportArtifact> {
        if req.spatial_bbox.is_some() {
            bail!(
                "spatial export is disabled until exact feature-level GeoArrow/GeoRust filtering is implemented"
            );
        }

        let entry = self.entry(dataset_id)?;
        let mut builder = builder_for_uri(&entry.info.uri).await?;
        builder = apply_filters(builder, &req.filters)?;
        builder = apply_projection(builder, req.columns.as_deref())?
            .with_offset(req.offset)
            .with_batch_size(self.batch_size);
        if let Some(limit) = req.limit {
            builder = builder.with_limit(limit);
        }

        let mut stream = builder.build().context("could not build export reader")?;
        let schema = Arc::clone(stream.schema());
        let temp = NamedTempFile::new().context("could not create temporary export file")?;
        let path = temp.into_temp_path();
        let output = File::create(&path).context("could not open temporary export file")?;

        match req.format {
            ExportFormat::Arrow => {
                let mut writer = arrow_ipc::writer::StreamWriter::try_new(output, schema.as_ref())
                    .context("could not create Arrow IPC export writer")?;
                use futures::TryStreamExt;
                while let Some(batch) = stream.try_next().await.context("export read failed")? {
                    writer.write(&batch).context("Arrow export write failed")?;
                }
                writer.finish().context("Arrow export finalize failed")?;
                Ok(ExportArtifact {
                    path,
                    content_type: "application/vnd.apache.arrow.stream",
                    filename: "subset.arrow",
                })
            }
            ExportFormat::Parquet => {
                let mut writer = ArrowWriter::try_new(output, Arc::clone(&schema), None)
                    .context("could not create Parquet export writer")?;
                use futures::TryStreamExt;
                while let Some(batch) = stream.try_next().await.context("export read failed")? {
                    writer
                        .write(&batch)
                        .context("Parquet export write failed")?;
                }
                writer.close().context("Parquet export finalize failed")?;
                Ok(ExportArtifact {
                    path,
                    content_type: "application/vnd.apache.parquet",
                    filename: "subset.parquet",
                })
            }
        }
    }

    /// Resolve a temporary dataset handle and refresh its idle lifetime.
    ///
    /// All engine operations that depend on an opened dataset should go through
    /// this method. That gives the handle one consistent lifecycle rule instead
    /// of requiring every API handler to remember to update activity timestamps.
    fn entry(&self, dataset_id: &str) -> Result<DatasetEntry> {
        let now = Instant::now();
        let mut datasets = self.datasets.write();

        if let Some(timeout) = self.dataset_idle_timeout {
            let expired = datasets
                .get(dataset_id)
                .map(|entry| now.duration_since(entry.last_accessed_at) >= timeout)
                .unwrap_or(false);
            if expired {
                datasets.remove(dataset_id);
                bail!(
                    "dataset not found: {dataset_id} (idle handle expired; reopen the source dataset)"
                );
            }
        }

        let entry = datasets
            .get_mut(dataset_id)
            .ok_or_else(|| anyhow!("dataset not found: {dataset_id}"))?;
        entry.last_accessed_at = now;
        Ok(entry.clone())
    }

    /// Retain only live handles while the caller already owns the dataset lock.
    fn prune_expired_locked(
        datasets: &mut HashMap<String, DatasetEntry>,
        timeout: Option<Duration>,
        now: Instant,
    ) -> usize {
        let Some(timeout) = timeout else { return 0; };
        let before = datasets.len();

        datasets.retain(|dataset_id, entry| {
            let idle_for = now.duration_since(entry.last_accessed_at);
            let keep = idle_for < timeout;
            if !keep {
                tracing::debug!(
                    dataset_id = %dataset_id,
                    idle_seconds = idle_for.as_secs_f64(),
                    open_seconds = now.duration_since(entry.opened_at).as_secs_f64(),
                    "expired idle dataset handle"
                );
            }
            keep
        });

        before - datasets.len()
    }
}

async fn builder_with_diagnostics(
    uri: &str,
    trace: Option<&TraceReporter>,
) -> Result<(ReaderBuilder, ReadMetrics)> {
    if let Some(trace) = trace {
        trace.event("query_parquet", "Reading Parquet footer and metadata");
    }

    let started = Instant::now();
    let (builder, read_metrics) = builder_for_uri_with_metrics(uri).await?;
    let total_ms = millis(started.elapsed());
    let storage = read_metrics.snapshot();

    if let Some(trace) = trace {
        trace.event_with_duration(
            "metadata_storage",
            "Fetching Parquet metadata from storage",
            storage.io_ms,
            format!(
                "{} object-store call{} · {} range{} · {} fetched",
                storage.calls,
                plural(storage.calls),
                storage.ranges,
                plural(storage.ranges),
                format_bytes(storage.returned_bytes),
            ),
        );
        trace.event_with_duration(
            "metadata_decode",
            "Parsing Parquet metadata",
            total_ms.saturating_sub(storage.io_ms),
            "Derived from metadata wall time minus measured storage wait",
        );
    }

    // From this point onward the same reader metrics represent column/page data
    // reads only, which lets the first-batch trace isolate remote storage wait.
    read_metrics.reset();
    Ok((builder, read_metrics))
}

fn millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn plural(value: u64) -> &'static str {
    if value == 1 { "" } else { "s" }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.2} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.2} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn apply_projection(
    mut builder: ReaderBuilder,
    columns: Option<&[String]>,
) -> Result<ReaderBuilder> {
    let Some(columns) = columns else {
        return Ok(builder);
    };
    if columns.is_empty() {
        return Ok(builder);
    }

    let schema = builder.metadata().file_metadata().schema_descr();
    let known_roots: HashSet<&str> = builder
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().as_str())
        .collect();
    for column in columns {
        let root = column.split('.').next().unwrap_or(column);
        if !known_roots.contains(root) {
            bail!("unknown column: {column}");
        }
    }
    let mask = ProjectionMask::columns(schema, columns.iter().map(String::as_str));
    builder = builder.with_projection(mask);
    Ok(builder)
}

fn choose_geometry_column(info: &DatasetInfo, requested: Option<&str>) -> Result<String> {
    let metadata_primary = info
        .geo_parquet
        .as_ref()
        .map(|metadata| metadata.primary_column.as_str());

    if let Some(requested) = requested {
        if info
            .geo_columns
            .iter()
            .any(|column| column.name == requested)
            || (metadata_primary == Some(requested)
                && info.columns.iter().any(|column| column.name == requested))
        {
            return Ok(requested.to_string());
        }
        bail!("not a recognized geospatial column: {requested}");
    }

    if let Some(primary) = metadata_primary {
        if info.geo_columns.iter().any(|column| column.name == primary)
            || info.columns.iter().any(|column| column.name == primary)
        {
            return Ok(primary.to_string());
        }
    }

    info.geo_columns
        .first()
        .map(|column| column.name.clone())
        .ok_or_else(|| anyhow!("dataset has no recognized geospatial column"))
}

fn validate_bbox(bbox: [f64; 4]) -> Result<()> {
    if bbox.iter().any(|value| !value.is_finite()) {
        bail!("bbox values must be finite numbers");
    }
    if bbox[0] > bbox[2] {
        bail!("bbox xmin must be <= xmax");
    }
    if bbox[1] > bbox[3] {
        bail!("bbox ymin must be <= ymax");
    }
    Ok(())
}

fn intersecting_row_groups(
    metadata: &ParquetMetaData,
    geometry_column: &str,
    query: [f64; 4],
) -> Result<Vec<usize>> {
    let schema = metadata.file_metadata().schema_descr();
    let leaf_index = schema
        .columns()
        .iter()
        .position(|column| column.path().string() == geometry_column)
        .ok_or_else(|| anyhow!("geometry column is not a leaf column: {geometry_column}"))?;

    let mut row_groups = Vec::new();
    for (index, row_group) in metadata.row_groups().iter().enumerate() {
        let column = row_group.column(leaf_index);
        let include = match column
            .geo_statistics()
            .and_then(|stats| stats.bounding_box())
        {
            Some(bbox) => bbox_intersects(
                [
                    bbox.get_xmin(),
                    bbox.get_ymin(),
                    bbox.get_xmax(),
                    bbox.get_ymax(),
                ],
                query,
            ),
            // Missing statistics must be retained for conservative correctness.
            None => true,
        };
        if include {
            row_groups.push(index);
        }
    }
    Ok(row_groups)
}

fn bbox_intersects(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

fn build_dataset_info(
    dataset_id: &str,
    name: Option<String>,
    uri: &str,
    builder: &ReaderBuilder,
) -> Result<DatasetInfo> {
    let metadata = builder.metadata();
    let schema_descr = metadata.file_metadata().schema_descr();
    let geo_metadata = parse_geo_metadata(metadata)?;

    let columns: Vec<ColumnInfo> = builder
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let physical = schema_descr
                .columns()
                .iter()
                .find(|column| {
                    column.path().parts().first().map(String::as_str) == Some(field.name().as_str())
                })
                .map(|column| format!("{:?}", column.physical_type()));

            ColumnInfo {
                name: field.name().to_string(),
                arrow_type: format!("{:?}", field.data_type()),
                parquet_physical_type: physical,
                nullable: field.is_nullable(),
            }
        })
        .collect();

    let mut geo_columns = Vec::new();
    for (leaf_index, column) in schema_descr.columns().iter().enumerate() {
        let (logical_type, crs, edge_interpolation) = match column.logical_type_ref() {
            Some(LogicalType::Geometry(geometry)) => (
                "GEOMETRY".to_string(),
                normalize_parquet_crs(geometry.crs.as_deref()),
                Some("PLANAR".to_string()),
            ),
            Some(LogicalType::Geography(geography)) => (
                "GEOGRAPHY".to_string(),
                normalize_parquet_crs(geography.crs.as_deref()),
                geography.algorithm().map(|value| format!("{value:?}")),
            ),
            _ => continue,
        };

        let column_name = column.path().string();
        let column_metadata = geo_metadata
            .as_ref()
            .and_then(|metadata| metadata.columns.get(&column_name));
        let mut row_groups_with_bbox = 0usize;
        let mut stats_extent: Option<[f64; 4]> = None;
        for row_group in metadata.row_groups() {
            if let Some(bbox) = row_group
                .column(leaf_index)
                .geo_statistics()
                .and_then(|stats| stats.bounding_box())
            {
                row_groups_with_bbox += 1;
                let candidate = [
                    bbox.get_xmin(),
                    bbox.get_ymin(),
                    bbox.get_xmax(),
                    bbox.get_ymax(),
                ];
                stats_extent = Some(match stats_extent {
                    None => candidate,
                    Some(current) => [
                        current[0].min(candidate[0]),
                        current[1].min(candidate[1]),
                        current[2].max(candidate[2]),
                        current[3].max(candidate[3]),
                    ],
                });
            }
        }

        let metadata_bbox = column_metadata.and_then(|column| column.bbox.clone());
        let dataset_bbox = stats_extent
            .or_else(|| metadata_bbox.as_deref().and_then(metadata_bbox_xy))
            .map(Vec::from);
        let is_primary = geo_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.primary_column == column_name);

        geo_columns.push(GeoColumnInfo {
            name: column_name,
            logical_type,
            crs,
            edge_interpolation,
            is_primary,
            geometry_types: column_metadata
                .map(|column| column.geometry_types.clone())
                .unwrap_or_default(),
            orientation: column_metadata.and_then(|column| column.orientation.clone()),
            epoch: column_metadata.and_then(|column| column.epoch),
            metadata_bbox,
            row_groups_with_bbox,
            row_groups_total: metadata.num_row_groups(),
            dataset_bbox,
        });
    }

    // GeoParquet 1.x stores WKB in ordinary BYTE_ARRAY/Binary columns and
    // declares their spatial meaning only in the file-level `geo` metadata.
    // Treat these as renderable geo columns as well, while keeping v2
    // conformance checks separate below.
    if let Some(metadata_geo) = geo_metadata.as_ref() {
        let existing = geo_columns
            .iter()
            .map(|column| column.name.clone())
            .collect::<HashSet<_>>();

        for (column_name, column_metadata) in &metadata_geo.columns {
            if existing.contains(column_name.as_str()) {
                continue;
            }
            if !column_metadata.encoding.eq_ignore_ascii_case("WKB") {
                continue;
            }

            let exists_in_schema = builder
                .schema()
                .fields()
                .iter()
                .any(|field| field.name() == column_name);
            if !exists_in_schema {
                continue;
            }

            let metadata_bbox = column_metadata.bbox.clone();
            let dataset_bbox = metadata_bbox
                .as_deref()
                .and_then(metadata_bbox_xy)
                .map(Vec::from);

            geo_columns.push(GeoColumnInfo {
                name: column_name.clone(),
                logical_type: "WKB".to_string(),
                crs: "OGC:CRS84".to_string(),
                edge_interpolation: Some(
                    column_metadata
                        .edges
                        .clone()
                        .unwrap_or_else(|| "planar".to_string())
                        .to_ascii_uppercase(),
                ),
                is_primary: metadata_geo.primary_column == *column_name,
                geometry_types: column_metadata.geometry_types.clone(),
                orientation: column_metadata.orientation.clone(),
                epoch: column_metadata.epoch,
                metadata_bbox,
                row_groups_with_bbox: 0,
                row_groups_total: metadata.num_row_groups(),
                dataset_bbox,
            });
        }
    }

    // GLOBE_COMPAT_VERSION: 2026-08-27-wkb-primary-v3
    // GeoParquet 1.x commonly stores the primary geometry as plain
    // BYTE_ARRAY/Binary WKB. If file-level geo metadata names a primary
    // column and that column is binary, expose it as a renderable WKB geo
    // column even when it has no native Parquet GEOMETRY/GEOGRAPHY type.
    if let Some(metadata_geo) = geo_metadata.as_ref() {
        let primary = metadata_geo.primary_column.as_str();
        let already_known = geo_columns.iter().any(|column| column.name == primary);

        if !already_known {
            if let Some(schema_column) = columns.iter().find(|column| column.name == primary) {
                let binary = matches!(
                    schema_column.arrow_type.as_str(),
                    "Binary" | "LargeBinary" | "BinaryView"
                ) || schema_column.parquet_physical_type.as_deref()
                    == Some("BYTE_ARRAY");

                if binary {
                    let column_metadata = metadata_geo.columns.get(primary);
                    let metadata_bbox = column_metadata.and_then(|column| column.bbox.clone());
                    let dataset_bbox = metadata_bbox
                        .as_deref()
                        .and_then(metadata_bbox_xy)
                        .map(Vec::from);

                    geo_columns.push(GeoColumnInfo {
                        name: primary.to_string(),
                        logical_type: "WKB".to_string(),
                        crs: "OGC:CRS84".to_string(),
                        edge_interpolation: Some(
                            column_metadata
                                .and_then(|column| column.edges.clone())
                                .unwrap_or_else(|| "planar".to_string())
                                .to_ascii_uppercase(),
                        ),
                        is_primary: true,
                        geometry_types: column_metadata
                            .map(|column| column.geometry_types.clone())
                            .unwrap_or_default(),
                        orientation: column_metadata.and_then(|column| column.orientation.clone()),
                        epoch: column_metadata.and_then(|column| column.epoch),
                        metadata_bbox,
                        row_groups_with_bbox: 0,
                        row_groups_total: metadata.num_row_groups(),
                        dataset_bbox,
                    });
                }
            }
        }
    }

    let geo_parquet = build_geo_parquet_info(geo_metadata.as_ref(), &geo_columns);

    Ok(DatasetInfo {
        dataset_id: dataset_id.to_string(),
        name,
        uri: uri.to_string(),
        num_rows: metadata.file_metadata().num_rows(),
        num_row_groups: metadata.num_row_groups(),
        columns,
        geo_columns,
        geo_parquet,
        capabilities: Capabilities::default(),
    })
}

#[derive(Debug, Deserialize)]
struct RawGeoMetadata {
    version: String,
    primary_column: String,
    columns: HashMap<String, RawGeoColumnMetadata>,
}

#[derive(Debug, Deserialize)]
struct RawGeoColumnMetadata {
    encoding: String,
    geometry_types: Vec<String>,
    #[serde(default)]
    edges: Option<String>,
    #[serde(default)]
    orientation: Option<String>,
    #[serde(default)]
    bbox: Option<Vec<f64>>,
    #[serde(default)]
    epoch: Option<f64>,
}

fn parse_geo_metadata(metadata: &ParquetMetaData) -> Result<Option<RawGeoMetadata>> {
    let Some(key_values) = metadata.file_metadata().key_value_metadata() else {
        return Ok(None);
    };
    let Some(value) = key_values
        .iter()
        .rev()
        .find(|entry| entry.key == "geo")
        .and_then(|entry| entry.value.as_deref())
    else {
        return Ok(None);
    };

    let parsed = serde_json::from_str::<RawGeoMetadata>(value)
        .context("invalid GeoParquet 'geo' file metadata JSON")?;
    Ok(Some(parsed))
}

fn build_geo_parquet_info(
    metadata: Option<&RawGeoMetadata>,
    geo_columns: &[GeoColumnInfo],
) -> Option<GeoParquetInfo> {
    let metadata = metadata?;
    let native_names = geo_columns
        .iter()
        .filter(|column| matches!(column.logical_type.as_str(), "GEOMETRY" | "GEOGRAPHY"))
        .map(|column| column.name.as_str())
        .collect::<HashSet<_>>();
    let mut warnings = Vec::new();

    if metadata.version != "2.0.0" {
        warnings.push(format!(
            "geo metadata version is {}, not 2.0.0",
            metadata.version
        ));
    }
    if !metadata.columns.contains_key(&metadata.primary_column) {
        warnings.push("primary_column is not present in geo metadata columns".to_string());
    }
    if !native_names.contains(metadata.primary_column.as_str()) {
        warnings.push("primary_column is not a native GEOMETRY/GEOGRAPHY column".to_string());
    }
    for (name, column) in &metadata.columns {
        let native = geo_columns.iter().find(|candidate| candidate.name == *name);
        if native.is_none() {
            warnings.push(format!(
                "geo metadata column '{name}' is not a native GEOMETRY/GEOGRAPHY column"
            ));
        }
        if column.encoding != "WKB" {
            warnings.push(format!(
                "geo metadata column '{name}' uses unsupported encoding '{}'",
                column.encoding
            ));
        }
        if let Some(orientation) = column.orientation.as_deref() {
            if orientation != "counterclockwise" {
                warnings.push(format!(
                    "geo metadata column '{name}' has invalid orientation '{orientation}'"
                ));
            }
        }
        if let Some(bbox) = column.bbox.as_deref() {
            if !matches!(bbox.len(), 4 | 6 | 8) || bbox.iter().any(|value| !value.is_finite()) {
                warnings.push(format!("geo metadata column '{name}' has an invalid bbox"));
            }
        }
        if column.epoch.is_some_and(|epoch| !epoch.is_finite()) {
            warnings.push(format!(
                "geo metadata column '{name}' has a non-finite coordinate epoch"
            ));
        }

        if let Some(native) = native {
            let native_edges = native
                .edge_interpolation
                .as_deref()
                .unwrap_or("PLANAR")
                .to_ascii_lowercase();
            let metadata_edges = column.edges.as_deref().unwrap_or("planar");
            if native_edges != metadata_edges {
                warnings.push(format!(
                    "geo metadata column '{name}' edges '{metadata_edges}' do not match native Parquet edge interpolation '{native_edges}'"
                ));
            }
        }
    }
    for name in &native_names {
        if !metadata.columns.contains_key(*name) {
            warnings.push(format!(
                "native geospatial column '{name}' is missing from geo metadata"
            ));
        }
    }

    Some(GeoParquetInfo {
        version: metadata.version.clone(),
        primary_column: metadata.primary_column.clone(),
        v2_metadata_checks_passed: metadata.version == "2.0.0" && warnings.is_empty(),
        warnings,
    })
}

fn normalize_parquet_crs(crs: Option<&str>) -> String {
    match crs {
        None => "OGC:CRS84".to_string(),
        Some("srid:0") => "UNDEFINED".to_string(),
        Some(value) => value.to_string(),
    }
}

fn metadata_bbox_xy(values: &[f64]) -> Option<[f64; 4]> {
    match values {
        [xmin, ymin, xmax, ymax] => Some([*xmin, *ymin, *xmax, *ymax]),
        [xmin, ymin, _, xmax, ymax, _] => Some([*xmin, *ymin, *xmax, *ymax]),
        [xmin, ymin, _, _, xmax, ymax, _, _] => Some([*xmin, *ymin, *xmax, *ymax]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_intersection_includes_touching_edges() {
        assert!(bbox_intersects([0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 2.0, 2.0]));
        assert!(!bbox_intersects([0.0, 0.0, 1.0, 1.0], [1.1, 1.1, 2.0, 2.0]));
    }

    #[test]
    fn invalid_bbox_is_rejected() {
        assert!(validate_bbox([10.0, 0.0, 1.0, 2.0]).is_err());
        assert!(validate_bbox([0.0, f64::NAN, 1.0, 2.0]).is_err());
    }
}
