/**
 * Shared frontend data contracts.
 *
 * Most interfaces in this file mirror Rust API request/response models. A small
 * number describe browser-only viewer state. These are
 * TypeScript types only: they are erased when the browser bundle is built and do
 * not execute at runtime.
 */
export interface ColumnInfo {
  name: string
  arrow_type: string
  parquet_physical_type?: string | null
  nullable: boolean
}

export interface GeoColumnInfo {
  name: string
  logical_type: 'GEOMETRY' | 'GEOGRAPHY' | string
  crs: string
  edge_interpolation?: string | null
  is_primary: boolean
  geometry_types: string[]
  orientation?: string | null
  epoch?: number | null
  metadata_bbox?: number[] | null
  row_groups_with_bbox: number
  row_groups_total: number
  dataset_bbox?: number[] | null
}

export interface GeoParquetInfo {
  version: string
  primary_column: string
  v2_metadata_checks_passed: boolean
  warnings: string[]
}

export interface Capabilities {
  arrow_ipc: boolean
  projection: boolean
  paging: boolean
  spatial_row_group_pruning: boolean
  spatial_exact: boolean
  filters: boolean
  sorting: boolean
  export_arrow: boolean
  export_parquet: boolean
}

export interface DatasetInfo {
  dataset_id: string
  name?: string | null
  uri: string
  num_rows: number
  num_row_groups: number
  columns: ColumnInfo[]
  geo_columns: GeoColumnInfo[]
  geo_parquet?: GeoParquetInfo | null
  capabilities: Capabilities
}

export type FilterOp = 'eq' | 'neq' | 'lt' | 'lte' | 'gt' | 'gte' | 'contains' | 'is_null' | 'is_not_null'

export interface FilterClause {
  column: string
  op: FilterOp
  value?: string | number | boolean | null
}

export interface TraceEvent {
  stage: string
  message: string
  detail?: string | null
  elapsed_ms: number
  duration_ms?: number | null
}

export interface TraceSnapshot {
  trace_id: string
  operation: string
  status: 'running' | 'complete' | 'error' | string
  elapsed_ms: number
  events: TraceEvent[]
}

export interface ResultRequest {
  trace_id?: string
  columns?: string[]
  filters?: FilterClause[]
}

export interface CountRequest {
  trace_id?: string
  filters?: FilterClause[]
}

export interface CountResponse {
  count: number
}

export interface PageRequest {
  trace_id?: string
  columns?: string[]
  offset: number
  limit: number
  filters?: FilterClause[]
}

export interface SpatialRequest {
  trace_id?: string
  bbox: [number, number, number, number]
  geometry_column?: string
  columns?: string[]
  max_features: number
  filters?: FilterClause[]
}

// Read-only physical-layout analysis API. These mirror the Rust response models
// so the UI can add an Analysis tab later without re-deriving Parquet logic in
// React.
export interface AnalysisSummaryResponse {
  dataset_id: string
  analysis_source: string
  rows: number
  columns: number
  leaf_columns: number
  row_groups: number
  compressed_data_bytes: number
  uncompressed_data_bytes: number
  compression_ratio?: number | null
  average_row_group_rows?: number | null
  average_row_group_compressed_bytes?: number | null
  largest_row_group_compressed_bytes: number
  row_groups_with_sorting_columns: number
  column_chunks_with_statistics: number
  column_chunks_total: number
  statistics_coverage?: number | null
  column_index_declared: boolean
  offset_index_declared: boolean
}

export interface ColumnAnalysis {
  name: string
  leaf_index: number
  physical_type: string
  logical_type?: string | null
  compressed_bytes: number
  uncompressed_bytes: number
  percent_of_compressed_data: number
  num_values: number
  average_compressed_bytes_per_value?: number | null
  compression_codecs: string[]
  encodings: string[]
  row_groups_with_statistics: number
  row_groups_with_geo_statistics: number
  row_groups_total: number
  column_index_declared: boolean
  offset_index_declared: boolean
}

export interface ColumnsAnalysisResponse {
  dataset_id: string
  analysis_source: string
  compressed_data_bytes: number
  columns: ColumnAnalysis[]
}

export interface RowGroupColumnAnalysis {
  name: string
  leaf_index: number
  compressed_bytes: number
  uncompressed_bytes: number
  num_values: number
  compression: string
  encodings: string[]
  statistics_available: boolean
  geo_statistics_available: boolean
  data_page_offset: number
  dictionary_page_offset?: number | null
  byte_range_offset: number
  byte_range_length: number
  column_index_declared: boolean
  offset_index_declared: boolean
}

export interface RowGroupAnalysis {
  index: number
  rows: number
  compressed_bytes: number
  uncompressed_bytes: number
  file_offset?: number | null
  sorting_columns_declared: boolean
  columns: RowGroupColumnAnalysis[]
}

export interface RowGroupsAnalysisResponse {
  dataset_id: string
  analysis_source: string
  row_groups: RowGroupAnalysis[]
}

export interface PageAnalysis {
  page_index: number
  offset: number
  compressed_page_size: number
  first_row_index: number
  row_count: number
  unencoded_byte_array_data_bytes?: number | null
}

export interface ColumnPagesAnalysis {
  row_group: number
  column: string
  leaf_index: number
  column_index_available: boolean
  offset_index_available: boolean
  pages: PageAnalysis[]
}

export interface PagesAnalysisResponse {
  dataset_id: string
  analysis_source: string
  storage_calls: number
  storage_ranges: number
  storage_bytes: number
  page_indexes_available: boolean
  columns: ColumnPagesAnalysis[]
  notes: string[]
}

export interface QueryCostRequest {
  columns?: string[]
  offset?: number
  limit?: number
  filters?: FilterClause[]
}

export interface QueryCostContributor {
  column: string
  compressed_bytes: number
  percent_of_estimated_read: number
}

export interface QueryCostResponse {
  dataset_id: string
  analysis_source: string
  estimate_kind: string
  requested_rows: number
  requested_columns: number
  row_groups: number[]
  column_chunks: number
  estimated_compressed_bytes_read: number
  contributors: QueryCostContributor[]
  warnings: string[]
}

export interface AnalysisFinding {
  code: string
  severity: string
  title: string
  detail: string
  recommendation?: string | null
}

export interface AnalysisRecommendationsResponse {
  dataset_id: string
  analysis_source: string
  interactive_read_health: string
  findings: AnalysisFinding[]
}
