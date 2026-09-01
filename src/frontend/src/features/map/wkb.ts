/**
 * Minimal browser-side WKB -> GeoJSON geometry decoder.
 *
 * WKB decoding happens only when a map path actually needs geometry. The table
 * can therefore avoid paying geometry CPU costs when binary geometry is not
 * being rendered.
 */
import type { Geometry, LineString, Point, Polygon, Position } from 'geojson'

// Minimal browser adapter for the first usable map. It supports the standard WKB
// geometry family plus ISO Z/M/ZM offsets and EWKB Z/M/SRID flags. The backend
// contract remains Arrow/WKB so this file can be replaced by GeoArrow-Wasm.
type Cursor = { view: DataView; offset: number; little: boolean }

export function parseWkb(input: Uint8Array): Geometry | null {
  if (!input.byteLength) return null
  try {
    return readGeometry({ view: new DataView(input.buffer, input.byteOffset, input.byteLength), offset: 0, little: true })
  } catch {
    return null
  }
}

function readGeometry(cursor: Cursor): Geometry | null {
  cursor.little = u8(cursor) === 1
  const raw = u32(cursor)
  const ewkbZ = (raw & 0x80000000) !== 0
  const ewkbM = (raw & 0x40000000) !== 0
  const hasSrid = (raw & 0x20000000) !== 0
  let type = raw & 0x1fffffff
  let hasZ = ewkbZ
  let hasM = ewkbM
  if (type >= 3000) { type -= 3000; hasZ = true; hasM = true }
  else if (type >= 2000) { type -= 2000; hasM = true }
  else if (type >= 1000) { type -= 1000; hasZ = true }
  if (hasSrid) u32(cursor)
  const dimensions = 2 + Number(hasZ) + Number(hasM)

  switch (type) {
    case 1: {
      const point = coordinate(cursor, dimensions)
      if (!Number.isFinite(point[0]) || !Number.isFinite(point[1])) return null
      return { type: 'Point', coordinates: point }
    }
    case 2: return { type: 'LineString', coordinates: coordinateArray(cursor, dimensions) }
    case 3: return { type: 'Polygon', coordinates: ringArray(cursor, dimensions) }
    case 4: return { type: 'MultiPoint', coordinates: nested(cursor, 'Point').map(g => (g as Point).coordinates) }
    case 5: return { type: 'MultiLineString', coordinates: nested(cursor, 'LineString').map(g => (g as LineString).coordinates) }
    case 6: return { type: 'MultiPolygon', coordinates: nested(cursor, 'Polygon').map(g => (g as Polygon).coordinates) }
    case 7: {
      const geometries: Geometry[] = []
      const count = u32(cursor)
      for (let i = 0; i < count; i += 1) {
        const geometry = readGeometry(cursor)
        if (geometry) geometries.push(geometry)
      }
      return { type: 'GeometryCollection', geometries }
    }
    default: return null
  }
}

function nested(cursor: Cursor, expected: Geometry['type']): Geometry[] {
  const count = u32(cursor)
  const result: Geometry[] = []
  for (let i = 0; i < count; i += 1) {
    const value = readGeometry(cursor)
    if (value?.type === expected) result.push(value)
  }
  return result
}

function coordinateArray(cursor: Cursor, dimensions: number): Position[] {
  const count = u32(cursor)
  const result: Position[] = new Array(count)
  for (let i = 0; i < count; i += 1) result[i] = coordinate(cursor, dimensions)
  return result
}

function ringArray(cursor: Cursor, dimensions: number): Position[][] {
  const count = u32(cursor)
  const result: Position[][] = new Array(count)
  for (let i = 0; i < count; i += 1) result[i] = coordinateArray(cursor, dimensions)
  return result
}

function coordinate(cursor: Cursor, dimensions: number): Position {
  const values: number[] = []
  for (let i = 0; i < dimensions; i += 1) values.push(f64(cursor))
  return values.slice(0, 2)
}

function u8(cursor: Cursor): number { return cursor.view.getUint8(cursor.offset++) }
function u32(cursor: Cursor): number {
  const value = cursor.view.getUint32(cursor.offset, cursor.little)
  cursor.offset += 4
  return value
}
function f64(cursor: Cursor): number {
  const value = cursor.view.getFloat64(cursor.offset, cursor.little)
  cursor.offset += 8
  return value
}
