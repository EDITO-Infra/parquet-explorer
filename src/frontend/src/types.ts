export type DatasetRef = {
  id: string
  uri: string
  format: 'parquet'
}

export type QueryResponse = {
  columns: string[]
  rows: unknown[][]
  row_count: number
  truncated: boolean
}

export type ColumnSchema = {
  name: string
  type: string
}

export type SchemaResponse = {
  dataset_id: string
  columns: ColumnSchema[]
}

export type GeoEligibility = {
  eligible: boolean
  reason: string
}
