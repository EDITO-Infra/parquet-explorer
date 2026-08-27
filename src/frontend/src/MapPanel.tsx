import { useEffect, useRef, useState } from 'react'
import {
  Map,
  NavigationControl,
  setWorkerUrl,
} from 'maplibre-gl'
import type {
  GeoJSONSource,
  LngLatBounds,
  Map as MapLibreMap,
} from 'maplibre-gl'
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'
import type { Feature, FeatureCollection, Geometry } from 'geojson'
import { pageBatches, spatialBatches } from './api'
import { plainValue } from './arrow'
import { parseWkb } from './wkb'
import type { DatasetInfo, FilterClause } from './types'
import type {
  CoordinateSpatialSource,
  GeometrySpatialSource,
  SpatialSource,
} from './spatial'

setWorkerUrl(workerUrl)

const SOURCE_ID = 'parquet-features'
const EMPTY: FeatureCollection = { type: 'FeatureCollection', features: [] }

// Keep the initial interaction fast. This is intentionally much smaller than
// the old 20k synchronous WKB -> GeoJSON path.
const MAP_FEATURE_LIMIT = 3_000

export function MapPanel({
  dataset,
  source,
  filters = [],
}: {
  dataset: DatasetInfo
  source: SpatialSource
  filters?: FilterClause[]
}) {
  const container = useRef<HTMLDivElement | null>(null)
  const mapRef = useRef<MapLibreMap | null>(null)
  const requestId = useRef(0)
  const refreshRef = useRef<((map: MapLibreMap) => Promise<void>) | null>(null)
  const filtersRef = useRef(filters)

  const [featureCount, setFeatureCount] = useState(0)
  const [status, setStatus] = useState('Initializing globe…')

  useEffect(() => {
    filtersRef.current = filters

    const map = mapRef.current
    const refresh = refreshRef.current
    if (map && refresh && map.loaded()) {
      void refresh(map)
    }
  }, [filters])

  useEffect(() => {
    if (!container.current) return

    const map = new Map({
      container: container.current,
      style: 'https://demotiles.maplibre.org/globe.json',
      center: [0, 20],
      zoom: 1.4,
      attributionControl: { compact: true },
    })

    mapRef.current = map
    map.addControl(new NavigationControl(), 'top-right')

    async function refreshViewport(mapInstance: MapLibreMap) {
      const thisRequest = ++requestId.current
      setStatus('Loading viewport…')

      try {
        const features = source.kind === 'coordinates'
          ? await loadCoordinateFeatures(
              dataset,
              source,
              mapInstance,
              filtersRef.current,
              thisRequest,
              requestId,
              setProgress,
            )
          : await loadGeometryFeatures(
              dataset,
              source,
              mapInstance,
              filtersRef.current,
              thisRequest,
              requestId,
              setProgress,
            )

        if (thisRequest !== requestId.current) return

        setMapFeatures(mapInstance, features)
        setFeatureCount(features.length)
        setStatus(
          source.kind === 'coordinates'
            ? 'Exact coordinate viewport'
            : dataset.capabilities.spatial_exact
              ? 'Exact geometry viewport'
              : 'Geometry row-group candidates',
        )
      } catch (error) {
        if (thisRequest === requestId.current) {
          setStatus(String(error))
        }
      }

      function setProgress(features: Feature[]) {
        if (thisRequest !== requestId.current) return
        setMapFeatures(mapInstance, features)
        setFeatureCount(features.length)
        setStatus(`Loading viewport… ${features.length.toLocaleString()} features`)
      }
    }

    refreshRef.current = refreshViewport

    map.on('load', () => {
      // The globe style already uses globe projection, but setting it explicitly
      // keeps this component correct if the style URL changes later.
      map.setProjection({ type: 'globe' })

      map.addSource(SOURCE_ID, {
        type: 'geojson',
        data: EMPTY,
      })

      map.addLayer({
        id: 'pv-polygons',
        type: 'fill',
        source: SOURCE_ID,
        filter: ['in', ['geometry-type'], ['literal', ['Polygon', 'MultiPolygon']]],
        paint: {
          'fill-color': '#2f80ed',
          'fill-opacity': 0.34,
        },
      })

      map.addLayer({
        id: 'pv-lines',
        type: 'line',
        source: SOURCE_ID,
        filter: ['in', ['geometry-type'], ['literal', ['LineString', 'MultiLineString']]],
        paint: {
          'line-color': '#57a0ff',
          'line-width': 1.5,
          'line-opacity': 0.8,
        },
      })

      map.addLayer({
        id: 'pv-points',
        type: 'circle',
        source: SOURCE_ID,
        filter: ['in', ['geometry-type'], ['literal', ['Point', 'MultiPoint']]],
        paint: {
          'circle-color': '#68d7ff',
          'circle-radius': 3.2,
          'circle-opacity': 0.78,
        },
      })

      if (source.kind === 'geometry') {
        fitGeometryDataset(map, source)
      }

      void refreshViewport(map)
      map.on('moveend', () => void refreshViewport(map))
    })

    return () => {
      requestId.current += 1
      refreshRef.current = null
      map.remove()
      mapRef.current = null
    }
  }, [dataset.dataset_id, source])

  return (
    <div className="map-shell">
      <div className="map-toolbar">
        <span>
          {source.kind === 'coordinates' ? (
            <>
              <strong>{source.longitude}</strong>
              {' / '}
              <strong>{source.latitude}</strong>
              {' · coordinates'}
            </>
          ) : (
            <>
              <strong>{source.geometry.name}</strong>
              {' · '}
              {source.geometry.logical_type}
            </>
          )}
        </span>
        <span>{featureCount.toLocaleString()} features · {status}</span>
      </div>

      <div ref={container} className="map-canvas" />

      <div className="map-note">
        Coordinate datasets are filtered in native Rust and rendered without WKB decoding.
        Geometry datasets use the existing bounded WKB fallback until the direct GeoArrow/deck.gl path is added.
      </div>
    </div>
  )
}

async function loadCoordinateFeatures(
  dataset: DatasetInfo,
  source: CoordinateSpatialSource,
  map: MapLibreMap,
  baseFilters: FilterClause[],
  thisRequest: number,
  requestId: { current: number },
  onProgress: (features: Feature[]) => void,
): Promise<Feature[]> {
  const propertyColumns = selectPropertyColumns(
    dataset,
    new Set([source.longitude, source.latitude]),
  )
  const columns = [source.longitude, source.latitude, ...propertyColumns]
  const filterSets = coordinateViewportFilterSets(
    map.getBounds(),
    source,
    baseFilters,
  )

  const features: Feature[] = []
  const perRequestLimit = Math.max(
    1,
    Math.ceil(MAP_FEATURE_LIMIT / filterSets.length),
  )

  for (const viewportFilters of filterSets) {
    if (thisRequest !== requestId.current) break

    for await (
      const batch of pageBatches(dataset.dataset_id, {
        columns,
        offset: 0,
        limit: perRequestLimit,
        filters: viewportFilters,
      })
    ) {
      if (thisRequest !== requestId.current) break

      const longitude = batch.getChild(source.longitude)
      const latitude = batch.getChild(source.latitude)
      if (!longitude || !latitude) continue

      for (let row = 0; row < batch.numRows; row += 1) {
        const x = longitude.get(row)
        const y = latitude.get(row)

        if (
          typeof x !== 'number'
          || typeof y !== 'number'
          || !Number.isFinite(x)
          || !Number.isFinite(y)
        ) {
          continue
        }

        const properties: Record<string, unknown> = {}
        for (const name of propertyColumns) {
          properties[name] = plainValue(batch.getChild(name)?.get(row))
        }

        features.push({
          type: 'Feature',
          geometry: {
            type: 'Point',
            coordinates: [x, y],
          },
          properties,
        })

        if (features.length >= MAP_FEATURE_LIMIT) break
      }

      // Show pixels after each Arrow RecordBatch instead of waiting for the
      // whole request to finish.
      onProgress(features)

      if (features.length >= MAP_FEATURE_LIMIT) break
    }

    if (features.length >= MAP_FEATURE_LIMIT) break
  }

  return features
}

async function loadGeometryFeatures(
  dataset: DatasetInfo,
  source: GeometrySpatialSource,
  map: MapLibreMap,
  filters: FilterClause[],
  thisRequest: number,
  requestId: { current: number },
  onProgress: (features: Feature[]) => void,
): Promise<Feature[]> {
  const geo = source.geometry
  const bbox = mapBoundsTuple(map)
  const propertyColumns = selectPropertyColumns(dataset, new Set([geo.name]))
  const columns = [geo.name, ...propertyColumns]
  const features: Feature[] = []

  for await (
    const batch of spatialBatches(dataset.dataset_id, {
      bbox,
      geometry_column: geo.name,
      columns,
      max_features: MAP_FEATURE_LIMIT,
      filters,
    })
  ) {
    if (thisRequest !== requestId.current) break

    const geometryVector = batch.getChild(geo.name)
    if (!geometryVector) continue

    for (let row = 0; row < batch.numRows; row += 1) {
      const bytes = geometryVector.get(row)
      if (!(bytes instanceof Uint8Array)) continue

      const parsed = parseWkb(bytes)
      if (!parsed) continue

      const properties: Record<string, unknown> = {}
      for (const name of propertyColumns) {
        properties[name] = plainValue(batch.getChild(name)?.get(row))
      }

      features.push({
        type: 'Feature',
        geometry: parsed as Geometry,
        properties,
      })

      if (features.length >= MAP_FEATURE_LIMIT) break
    }

    onProgress(features)
    if (features.length >= MAP_FEATURE_LIMIT) break
  }

  return features
}

function coordinateViewportFilterSets(
  bounds: LngLatBounds,
  source: CoordinateSpatialSource,
  baseFilters: FilterClause[],
): FilterClause[][] {
  const south = clamp(bounds.getSouth(), -90, 90)
  const north = clamp(bounds.getNorth(), -90, 90)
  const rawWest = bounds.getWest()
  const rawEast = bounds.getEast()

  const latitudeFilters: FilterClause[] = [
    ...baseFilters,
    { column: source.latitude, op: 'gte', value: south },
    { column: source.latitude, op: 'lte', value: north },
  ]

  // At the whole-world view, filtering longitude adds no selectivity and can
  // complicate antimeridian handling.
  if (rawEast - rawWest >= 359.999) {
    return [latitudeFilters]
  }

  const west = normalizeLongitude(rawWest)
  const east = normalizeLongitude(rawEast)

  if (west <= east) {
    return [[
      ...latitudeFilters,
      { column: source.longitude, op: 'gte', value: west },
      { column: source.longitude, op: 'lte', value: east },
    ]]
  }

  // Viewport crosses the antimeridian. The current backend filter language is
  // AND-only, so issue two small exact requests and merge the results.
  return [
    [
      ...latitudeFilters,
      { column: source.longitude, op: 'gte', value: west },
      { column: source.longitude, op: 'lte', value: 180 },
    ],
    [
      ...latitudeFilters,
      { column: source.longitude, op: 'gte', value: -180 },
      { column: source.longitude, op: 'lte', value: east },
    ],
  ]
}

function selectPropertyColumns(
  dataset: DatasetInfo,
  excluded: Set<string>,
): string[] {
  const preferredNames = [
    'id',
    'scientificName',
    'acceptedScientificName',
    'species',
    'occurrenceID',
    'name',
    'title',
  ]

  const available = new Set(dataset.columns.map(column => column.name))
  const selected = preferredNames.filter(
    name => available.has(name) && !excluded.has(name),
  )

  if (selected.length >= 4) return selected.slice(0, 4)

  for (const column of dataset.columns) {
    if (excluded.has(column.name) || selected.includes(column.name)) continue
    selected.push(column.name)
    if (selected.length >= 4) break
  }

  return selected
}

function setMapFeatures(map: MapLibreMap, features: Feature[]) {
  const source = map.getSource(SOURCE_ID) as GeoJSONSource | undefined
  if (!source) return

  source.setData({
    type: 'FeatureCollection',
    features: features.slice(),
  })
}

function mapBoundsTuple(
  map: MapLibreMap,
): [number, number, number, number] {
  const bounds = map.getBounds()
  return [
    bounds.getWest(),
    bounds.getSouth(),
    bounds.getEast(),
    bounds.getNorth(),
  ]
}

function fitGeometryDataset(
  map: MapLibreMap,
  source: GeometrySpatialSource,
) {
  const bbox = source.geometry.dataset_bbox
  if (!bbox || bbox.length < 4) return

  map.fitBounds(
    [[bbox[0], bbox[1]], [bbox[2], bbox[3]]],
    {
      padding: 40,
      duration: 0,
      maxZoom: 7,
    },
  )
}

function normalizeLongitude(value: number): number {
  return ((value + 180) % 360 + 360) % 360 - 180
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
