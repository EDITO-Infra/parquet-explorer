use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct DatasetOpenRequest {
    pub uri: String,
    #[serde(default)]
    pub name: Option<String>,
}

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
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub arrow_type: String,
    pub parquet_physical_type: Option<String>,
    pub nullable: bool,
}

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
            filters: false,
            sorting: false,
            export_arrow: true,
            export_parquet: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatasetEntry {
    pub info: DatasetInfo,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FilterClause {
    pub column: String,
    pub op: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SortClause {
    pub column: String,
    #[serde(default)]
    pub descending: bool,
}

fn default_page_limit() -> usize { 1000 }
fn default_spatial_limit() -> usize { 20_000 }

#[derive(Debug, Clone, Deserialize)]
pub struct PageRequest {
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

#[derive(Debug, Clone, Deserialize)]
pub struct SpatialRequest {
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
    fn default() -> Self { Self::Parquet }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportRequest {
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

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub native_engine: String,
}
