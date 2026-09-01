/**
 * Small geospatial metadata/value helpers shared by the browser.
 *
 * These functions are deliberately independent of React and MapLibre so they
 * can be reused wherever dataset geospatial metadata needs interpretation.
 */
import type { DatasetInfo, GeoColumnInfo } from '../../types'

/**
 * Return a renderable geometry column.
 *
 * Preferred: backend-classified native/legacy geometry in geo_columns.
 * Fallback: GeoParquet 1.x metadata may declare a WKB primary column while
 * older backends still return geo_columns: []. In that case synthesize the
 * minimal geometry descriptor the map needs from the file metadata + schema.
 */
export function pickGeometry(dataset: DatasetInfo): GeoColumnInfo | undefined {
  const classified = dataset.geo_columns.find(column => column.is_primary) ?? dataset.geo_columns[0]
  if (classified) return classified

  const primary = dataset.geo_parquet?.primary_column
  if (!primary) return undefined

  const column = dataset.columns.find(candidate => candidate.name === primary)
  if (!column) return undefined

  if (!['Binary', 'LargeBinary', 'BinaryView'].includes(column.arrow_type)) return undefined

  return {
    name: primary,
    logical_type: 'WKB',
    crs: 'OGC:CRS84',
    edge_interpolation: 'PLANAR',
    is_primary: true,
    geometry_types: [],
    orientation: null,
    epoch: null,
    metadata_bbox: null,
    row_groups_with_bbox: 0,
    row_groups_total: dataset.num_row_groups,
    dataset_bbox: null,
  }
}
