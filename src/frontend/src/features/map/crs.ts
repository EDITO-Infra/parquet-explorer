/**
 * CRS normalization for the map pipeline.
 *
 * GeoParquet geometry columns may carry any CRS. MapLibre only renders
 * WGS84 lon/lat, so projected coordinates are transformed to EPSG:4326
 * before features are built. Unknown CRS degrades to no transform.
 */
import proj4 from 'proj4'
import type { Geometry, Position } from 'geojson'

proj4.defs(
  'EPSG:3035',
  '+proj=laea +lat_0=52 +lon_0=10 +x_0=4321000 +y_0=3210000 +ellps=GRS80 +units=m +no_defs',
)

const WGS84_EQUIVALENTS = new Set([
  'OGC:CRS84',
  'EPSG:4326',
  'URN:OGC:DEF:CRS:OGC:1.3:CRS84',
])

export type CoordTransform = (position: Position) => Position

/** Returns undefined when the CRS is already lon/lat WGS84 or unknown. */
export function makeCoordTransform(
  crs: string | null | undefined,
): CoordTransform | undefined {
  if (!crs || WGS84_EQUIVALENTS.has(crs.trim().toUpperCase())) return undefined
  if (!proj4.defs(crs)) return undefined
  const converter = proj4(crs, 'EPSG:4326')
  return ([x, y]) => {
    const [lon, lat] = converter.forward([x, y])
    return [lon, lat]
  }
}

export function transformGeometry(
  geometry: Geometry,
  transform: CoordTransform,
): Geometry {
  if (geometry.type === 'GeometryCollection') {
    return {
      type: 'GeometryCollection',
      geometries: geometry.geometries.map(g => transformGeometry(g, transform)),
    }
  }
  return {
    ...geometry,
    coordinates: mapPositions(geometry.coordinates, transform),
  } as Geometry
}

function mapPositions(value: unknown, transform: CoordTransform): unknown {
  if (Array.isArray(value) && typeof value[0] === 'number') {
    return transform(value as Position)
  }
  return (value as unknown[]).map(item => mapPositions(item, transform))
}
