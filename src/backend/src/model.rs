//! Shared JSON request/response models for the backend API.
//!
//! These types intentionally contain data only. Query execution belongs in the
//! engine and HTTP concerns belong in `api/`. Keeping the wire contract here
//! makes endpoint behavior easier to review and document.

use serde::{Deserialize, Serialize};

/// Request to validate and register a Parquet source URL for this browser session.
#[derive(Debug, Clone, Deserialize)]
pub struct DatasetOpenRequest {
    pub uri: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub trace_id: Option<String>,
}

/// Lightweight dataset metadata returned when a source is opened.
///
/// This is not the entire Parquet footer; it is the stable subset needed by the
/// viewer UI and subsequent API calls.
#[derive(Debug, Clone, Serialize)]
pub struct DatasetInfo {
    pub dataset_id: String,
    pub name: Option<String>,
    pub uri: String,
    pub num_rows: i64,
    pub num_row_groups: usize,
    pub columns: Vec<ColumnInfo>,
    pub geo_columns: Vec<GeoColumnInfo>,
    pub geo_parquet: Option<GeoParquetInfo>,
    pub layout: DatasetLayoutSummary,
    pub capabilities: Capabilities,
}


/// Compact physical-layout information derived once from the Parquet footer.
///
/// Detailed row-group/page metadata stays server-side or behind the analysis
/// endpoints; this summary is cheap enough to return with `/datasets/open`.
#[derive(Debug, Clone, Serialize)]
pub struct DatasetLayoutSummary {
    pub compressed_data_bytes: u64,
    pub uncompressed_data_bytes: u64,
    pub average_row_group_rows: Option<f64>,
    pub average_row_group_compressed_bytes: Option<f64>,
    pub largest_row_group_compressed_bytes: u64,
    pub statistics_coverage: Option<f64>,
    pub column_index_declared: bool,
    pub offset_index_declared: bool,
}

/// User-facing schema information for one top-level Arrow field.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub arrow_type: String,
    pub parquet_physical_type: Option<String>,
    pub nullable: bool,
}

/// GeoParquet/Parquet-native geometry metadata used by the map viewer.
#[derive(Debug, Clone, Serialize)]
pub struct GeoColumnInfo {
    pub name: String,
    pub logical_type: String,
    pub crs: String,
    pub edge_interpolation: Option<String>,
    pub is_primary: bool,
    pub geometry_types: Vec<String>,
    pub orientation: Option<String>,
    pub epoch: Option<f64>,
    pub metadata_bbox: Option<Vec<f64>>,
    pub row_groups_with_bbox: usize,
    pub row_groups_total: usize,
    pub dataset_bbox: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoParquetInfo {
    pub version: String,
    pub primary_column: String,
    pub v2_metadata_checks_passed: bool,
    pub warnings: Vec<String>,
}

/// Feature flags describing which operations this backend version supports.
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub arrow_ipc: bool,
    pub projection: bool,
    pub paging: bool,
    pub spatial_row_group_pruning: bool,
    pub spatial_exact: bool,
    pub filters: bool,
    pub sorting: bool,
    pub export_arrow: bool,
    pub export_parquet: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            arrow_ipc: true,
            projection: true,
            paging: true,
            spatial_row_group_pruning: true,
            spatial_exact: false,
            filters: true,
            sorting: false,
            export_arrow: true,
            export_parquet: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    Contains,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FilterClause {
    pub column: String,
    pub op: FilterOp,
    #[serde(default)]
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SortClause {
    pub column: String,
    #[serde(default)]
    pub descending: bool,
}

fn default_page_limit() -> usize {
    1000
}
fn default_spatial_limit() -> usize {
    20_000
}

/// Request for a row-window table page. Projection and filters are pushed into
/// Parquet-RS before Arrow IPC is produced.
#[derive(Debug, Clone, Deserialize)]
pub struct PageRequest {
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_page_limit")]
    pub limit: usize,
    #[serde(default)]
    pub filters: Vec<FilterClause>,
    #[serde(default)]
    pub sort: Vec<SortClause>,
}

/// Request for a complete result stream with projection and optional filters.
///
/// Unlike `PageRequest`, this request has no row window. The browser uses it
/// for filtered-result caching and for an explicit user-requested All load.
#[derive(Debug, Clone, Deserialize)]
pub struct ResultRequest {
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    #[serde(default)]
    pub filters: Vec<FilterClause>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CountRequest {
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub filters: Vec<FilterClause>,
}

#[derive(Debug, Serialize)]
pub struct CountResponse {
    pub count: u64,
}

/// Request for conservative row-group bbox pruning plus a feature limit.
#[derive(Debug, Clone, Deserialize)]
pub struct SpatialRequest {
    #[serde(default)]
    pub trace_id: Option<String>,
    pub bbox: [f64; 4],
    #[serde(default)]
    pub geometry_column: Option<String>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    #[serde(default = "default_spatial_limit")]
    pub max_features: usize,
    #[serde(default)]
    pub filters: Vec<FilterClause>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Arrow,
    Parquet,
}

impl Default for ExportFormat {
    fn default() -> Self {
        Self::Parquet
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportRequest {
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub format: ExportFormat,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub filters: Vec<FilterClause>,
    #[serde(default)]
    pub spatial_bbox: Option<[f64; 4]>,
}

#[derive(Debug, Serialize)]
pub struct SchemaResponse {
    pub dataset_id: String,
    pub columns: Vec<ColumnInfo>,
    pub geo_columns: Vec<GeoColumnInfo>,
}

/// One diagnostics record. Milestones have `duration_ms = None`; measured work
/// records carry an explicit duration.
#[derive(Debug, Clone, Serialize)]
pub struct TraceEvent {
    pub stage: String,
    pub message: String,
    pub detail: Option<String>,
    /// Milliseconds from the start of the trace when this event was recorded.
    pub elapsed_ms: u64,
    /// Actual measured duration for this stage when one is available.
    ///
    /// This is deliberately separate from `elapsed_ms`: diagnostics UIs must not
    /// infer stage duration by subtracting adjacent milestone timestamps.
    pub duration_ms: Option<u64>,
}

/// Current immutable view of one in-memory request trace.
#[derive(Debug, Clone, Serialize)]
pub struct TraceSnapshot {
    pub trace_id: String,
    pub operation: String,
    pub status: String,
    pub elapsed_ms: u64,
    pub events: Vec<TraceEvent>,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub native_engine: String,
}

// -----------------------------------------------------------------------------
// Read-only Parquet analysis API models
// -----------------------------------------------------------------------------
//
// These response types describe the *physical layout* of a Parquet file. They
// intentionally do not contain rewrite/optimization commands: this application
// is a portable viewer/explorer. A separate optimizer can consume the same
// information later without coupling file-management behavior into this backend.

/// Cheap file-level physical-layout summary derived from Parquet metadata.
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisSummaryResponse {
    pub dataset_id: String,
    /// Where the numbers came from. Currently all fields except page details are
    /// derived from the Parquet footer/column-chunk metadata.
    pub analysis_source: &'static str,
    pub rows: i64,
    pub columns: usize,
    pub leaf_columns: usize,
    pub row_groups: usize,
    pub compressed_data_bytes: u64,
    pub uncompressed_data_bytes: u64,
    pub compression_ratio: Option<f64>,
    pub average_row_group_rows: Option<f64>,
    pub average_row_group_compressed_bytes: Option<f64>,
    pub largest_row_group_compressed_bytes: u64,
    pub row_groups_with_sorting_columns: usize,
    pub column_chunks_with_statistics: usize,
    pub column_chunks_total: usize,
    pub statistics_coverage: Option<f64>,
    /// True when at least one column chunk declares a Parquet ColumnIndex.
    pub column_index_declared: bool,
    /// True when at least one column chunk declares a Parquet OffsetIndex.
    pub offset_index_declared: bool,
}

/// Aggregate physical storage information for one Parquet leaf column.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnAnalysis {
    pub name: String,
    pub leaf_index: usize,
    pub physical_type: String,
    pub logical_type: Option<String>,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub percent_of_compressed_data: f64,
    pub num_values: u64,
    pub average_compressed_bytes_per_value: Option<f64>,
    pub compression_codecs: Vec<String>,
    pub encodings: Vec<String>,
    pub row_groups_with_statistics: usize,
    pub row_groups_with_geo_statistics: usize,
    pub row_groups_total: usize,
    pub column_index_declared: bool,
    pub offset_index_declared: bool,
}

/// Dataset-wide column analysis.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnsAnalysisResponse {
    pub dataset_id: String,
    pub analysis_source: &'static str,
    pub compressed_data_bytes: u64,
    pub columns: Vec<ColumnAnalysis>,
}

/// Physical metadata for one column chunk inside a row group.
#[derive(Debug, Clone, Serialize)]
pub struct RowGroupColumnAnalysis {
    pub name: String,
    pub leaf_index: usize,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub num_values: u64,
    pub compression: String,
    pub encodings: Vec<String>,
    pub statistics_available: bool,
    pub geo_statistics_available: bool,
    pub data_page_offset: i64,
    pub dictionary_page_offset: Option<i64>,
    pub byte_range_offset: u64,
    pub byte_range_length: u64,
    pub column_index_declared: bool,
    pub offset_index_declared: bool,
}

/// One Parquet row group's physical layout.
#[derive(Debug, Clone, Serialize)]
pub struct RowGroupAnalysis {
    pub index: usize,
    pub rows: i64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub file_offset: Option<i64>,
    pub sorting_columns_declared: bool,
    pub columns: Vec<RowGroupColumnAnalysis>,
}

/// Dataset-wide row-group analysis.
#[derive(Debug, Clone, Serialize)]
pub struct RowGroupsAnalysisResponse {
    pub dataset_id: String,
    pub analysis_source: &'static str,
    pub row_groups: Vec<RowGroupAnalysis>,
}

/// Optional selectors for the deeper page-index inspection endpoint.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PagesAnalysisQuery {
    /// Restrict the response to one zero-based row-group index.
    #[serde(default)]
    pub row_group: Option<usize>,
    /// Restrict the response to one Parquet leaf path (for example `geometry`).
    #[serde(default)]
    pub column: Option<String>,
}

/// One data page described by the Parquet OffsetIndex.
#[derive(Debug, Clone, Serialize)]
pub struct PageAnalysis {
    pub page_index: usize,
    pub offset: i64,
    pub compressed_page_size: i32,
    pub first_row_index: i64,
    pub row_count: i64,
    /// Available for some BYTE_ARRAY writers and useful for spotting unusually
    /// large geometry/string pages before reading their payloads.
    pub unencoded_byte_array_data_bytes: Option<i64>,
}

/// Page-index details for one row-group/column-chunk pair.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnPagesAnalysis {
    pub row_group: usize,
    pub column: String,
    pub leaf_index: usize,
    pub column_index_available: bool,
    pub offset_index_available: bool,
    pub pages: Vec<PageAnalysis>,
}

/// Deeper page-level analysis. Loading this endpoint may perform additional
/// small object-store reads because page indexes live outside the normal footer.
#[derive(Debug, Clone, Serialize)]
pub struct PagesAnalysisResponse {
    pub dataset_id: String,
    pub analysis_source: &'static str,
    pub storage_calls: u64,
    pub storage_ranges: u64,
    pub storage_bytes: u64,
    pub page_indexes_available: bool,
    pub columns: Vec<ColumnPagesAnalysis>,
    pub notes: Vec<String>,
}

/// Request used to estimate the physical I/O cost of a viewer-style row query.
///
/// This is deliberately shaped like `PageRequest`, but it never reads data
/// pages. It estimates which row groups and column chunks the current reader
/// plan would need based on footer metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryCostRequest {
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_page_limit")]
    pub limit: usize,
    #[serde(default)]
    pub filters: Vec<FilterClause>,
}

/// Per-column contribution to an estimated Parquet read.
#[derive(Debug, Clone, Serialize)]
pub struct QueryCostContributor {
    pub column: String,
    pub compressed_bytes: u64,
    pub percent_of_estimated_read: f64,
}

/// Footer-based estimate of how much compressed Parquet data a query may read.
#[derive(Debug, Clone, Serialize)]
pub struct QueryCostResponse {
    pub dataset_id: String,
    pub analysis_source: &'static str,
    pub estimate_kind: &'static str,
    pub requested_rows: usize,
    pub requested_columns: usize,
    pub row_groups: Vec<usize>,
    pub column_chunks: usize,
    pub estimated_compressed_bytes_read: u64,
    pub contributors: Vec<QueryCostContributor>,
    pub warnings: Vec<String>,
}

/// A developer/user-facing observation derived from file metadata.
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisFinding {
    pub code: &'static str,
    pub severity: &'static str,
    pub title: String,
    pub detail: String,
    pub recommendation: Option<String>,
}

/// Read-only recommendations for improving interactive Parquet access.
///
/// The endpoint never modifies the source. Recommendations are intentionally
/// phrased as observations that another application may choose to act on.
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisRecommendationsResponse {
    pub dataset_id: String,
    pub analysis_source: &'static str,
    pub interactive_read_health: &'static str,
    pub findings: Vec<AnalysisFinding>,
}
