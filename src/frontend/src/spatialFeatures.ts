import type { RecordBatch } from 'apache-arrow'
import type { Feature, Geometry } from 'geojson'
import { plainValue } from './arrow'
import type { DatasetInfo } from './types'
import type { SpatialSource } from './spatial'
import { parseWkb } from './wkb'

export function spatialColumnNames(source: SpatialSource): string[] {
  return source.kind === 'coordinates'
    ? [source.longitude, source.latitude]
    : [source.geometry.name]
}

export function selectMapPropertyColumns(
  dataset: DatasetInfo,
  source: SpatialSource,
  preferred: string[] = [],
  maxColumns = 8,
): string[] {
  const excluded = new Set(spatialColumnNames(source))
  const available = new Set(dataset.columns.map(column => column.name))
  const defaults = [
    'id',
    'scientificName',
    'acceptedScientificName',
    'species',
    'occurrenceID',
    'name',
    'title',
  ]

  const selected: string[] = []
  for (const name of [...preferred, ...defaults]) {
    if (!available.has(name) || excluded.has(name) || selected.includes(name)) continue
    selected.push(name)
    if (selected.length >= maxColumns) return selected
  }

  for (const column of dataset.columns) {
    if (excluded.has(column.name) || selected.includes(column.name)) continue
    selected.push(column.name)
    if (selected.length >= maxColumns) break
  }

  return selected
}

export function appendSpatialBatchFeatures(
  batch: RecordBatch,
  source: SpatialSource,
  propertyColumns: string[],
  features: Feature[],
  rowOffset: number,
): void {
  if (source.kind === 'coordinates') {
    const longitude = batch.getChild(source.longitude)
    const latitude = batch.getChild(source.latitude)
    if (!longitude || !latitude) return

    for (let row = 0; row < batch.numRows; row += 1) {
      const x = longitude.get(row)
      const y = latitude.get(row)
      if (
        typeof x !== 'number'
        || typeof y !== 'number'
        || !Number.isFinite(x)
        || !Number.isFinite(y)
      ) continue

      const snapshotRow = rowOffset + row
      const properties = batchProperties(batch, propertyColumns, row)
      properties[source.longitude] = x
      properties[source.latitude] = y
      properties.__pv_snapshot_row = snapshotRow + 1

      features.push({
        type: 'Feature',
        id: `${snapshotRow}:0`,
        geometry: { type: 'Point', coordinates: [x, y] },
        properties,
      })
    }
    return
  }

  const geometryVector = batch.getChild(source.geometry.name)
  if (!geometryVector) return

  for (let row = 0; row < batch.numRows; row += 1) {
    const bytes = geometryVector.get(row)
    if (!(bytes instanceof Uint8Array)) continue

    const parsed = parseWkb(bytes)
    if (!parsed) continue

    const snapshotRow = rowOffset + row
    const properties = batchProperties(batch, propertyColumns, row)
    properties.__pv_snapshot_row = snapshotRow + 1

    renderableGeometries(parsed).forEach((geometry, partIndex) => {
      features.push({
        type: 'Feature',
        id: `${snapshotRow}:${partIndex}`,
        geometry,
        properties: { ...properties },
      })
    })
  }
}

function batchProperties(
  batch: RecordBatch,
  propertyColumns: string[],
  row: number,
): Record<string, unknown> {
  const properties: Record<string, unknown> = {}
  for (const name of propertyColumns) {
    properties[name] = plainValue(batch.getChild(name)?.get(row))
  }
  return properties
}

function renderableGeometries(geometry: Geometry): Geometry[] {
  if (geometry.type !== 'GeometryCollection') return [geometry]
  return geometry.geometries.flatMap(renderableGeometries)
}
