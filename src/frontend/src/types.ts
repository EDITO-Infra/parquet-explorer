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

export interface PageRequest {
  columns?: string[]
  offset: number
  limit: number
}

export interface SpatialRequest {
  bbox: [number, number, number, number]
  geometry_column?: string
  columns?: string[]
  max_features: number
}
