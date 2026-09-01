/**
 * Decide which dataset columns should act as the map's spatial source.
 *
 * Coordinate columns are preferred when available because they are cheap to
 * read/render. Otherwise the viewer falls back to GeoParquet geometry metadata.
 * This module chooses a source; it does not render the map.
 */
import type {
  ColumnInfo,
  DatasetInfo,
  GeoColumnInfo,
} from '../../types'

export type CoordinateSpatialSource = {
  kind: 'coordinates'
  longitude: string
  latitude: string
}

export type GeometrySpatialSource = {
  kind: 'geometry'
  geometry: GeoColumnInfo
}

export type SpatialSource = CoordinateSpatialSource | GeometrySpatialSource

/**
 * Pick the cheapest usable spatial representation for interactive rendering.
 *
 * Coordinate pairs are preferred because point datasets can be filtered in
 * native Rust using ordinary Arrow numeric predicates and rendered without
 * decoding WKB in the browser.
 *
 * If no coordinate pair exists, fall back to native GeoParquet geometry or a
 * GeoParquet 1.x metadata-declared WKB primary column.
 */
export function detectSpatialSource(
  dataset: DatasetInfo,
): SpatialSource | undefined {
  const coordinates = detectCoordinatePair(dataset.columns)
  if (coordinates) return coordinates

  const geometry = detectGeometry(dataset)
  if (geometry) {
    return {
      kind: 'geometry',
      geometry,
    }
  }

  return undefined
}

function detectCoordinatePair(
  columns: ColumnInfo[],
): CoordinateSpatialSource | undefined {
  const numeric = columns.filter(column => isNumeric(column.arrow_type))

  // User intent: any numeric column with "longitude" / "latitude" in its
  // name should be eligible, case-insensitively.
  const longitude = numeric.find(column =>
    column.name.toLowerCase().includes('longitude'),
  )

  const latitude = numeric.find(column =>
    column.name.toLowerCase().includes('latitude'),
  )

  if (!longitude || !latitude) return undefined

  return {
    kind: 'coordinates',
    longitude: longitude.name,
    latitude: latitude.name,
  }
}

function detectGeometry(
  dataset: DatasetInfo,
): GeoColumnInfo | undefined {
  const classified =
    dataset.geo_columns.find(column => column.is_primary)
    ?? dataset.geo_columns[0]

  if (classified) return classified

  // GeoParquet 1.x compatibility: the file-level `geo` metadata can declare
  // a primary WKB column even when Parquet itself only reports Binary.
  const primary = dataset.geo_parquet?.primary_column
  if (!primary) return undefined

  const column = dataset.columns.find(candidate => candidate.name === primary)
  if (!column) return undefined

  if (!['Binary', 'LargeBinary', 'BinaryView'].includes(column.arrow_type)) {
    return undefined
  }

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

function isNumeric(type: string): boolean {
  return (
    type.startsWith('Float')
    || type.startsWith('Int')
    || type.startsWith('UInt')
    || type.startsWith('Decimal')
  )
}
