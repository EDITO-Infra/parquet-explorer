/**
 * MapLibre owner for the current browser-side result.
 *
 * Data loading belongs to App: an unfiltered map receives the current table
 * page, while filtered/All modes receive the shared complete-result cache.
 * MapPanel only renders and interacts with those features; pan/zoom never issue
 * Parquet queries.
 */
import { useEffect, useRef, useState } from 'react'
import {
  Map,
  NavigationControl,
  setWorkerUrl,
} from 'maplibre-gl'
import type {
  GeoJSONSource,
  Map as MapLibreMap,
  MapGeoJSONFeature,
} from 'maplibre-gl'
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'
import type { Feature, FeatureCollection, Geometry, Position } from 'geojson'
import type { DatasetInfo } from '../../types'
import type { SpatialSource } from './spatial'

setWorkerUrl(workerUrl)

const SOURCE_ID = 'parquet-features'
const FEATURE_LAYER_IDS = ['pv-polygons', 'pv-lines', 'pv-points'] as const
const SELECTED_LAYER_IDS = ['pv-polygons-selected', 'pv-lines-selected', 'pv-points-selected'] as const
const EMPTY: FeatureCollection = { type: 'FeatureCollection', features: [] }

const BASEMAPS = {
  positron: {
    label: 'Light',
    style: 'https://tiles.openfreemap.org/styles/positron',
  },
  liberty: {
    label: 'Streets',
    style: 'https://tiles.openfreemap.org/styles/liberty',
  },
  dark: {
    label: 'Dark',
    style: 'https://tiles.openfreemap.org/styles/dark',
  },
} as const

type BasemapId = keyof typeof BASEMAPS
type ProjectionMode = 'mercator' | 'globe'
type Theme = 'light' | 'dark'
type FeatureId = string | number

type SelectionBox = {
  left: number
  top: number
  width: number
  height: number
}

type BoundsAccumulator = {
  west: number
  south: number
  east: number
  north: number
  valid: boolean
}

export function MapPanel({
  dataset,
  source,
  features,
  rowsLoaded,
  scopeLabel,
  dataKey,
  dataRevision,
  loading,
  complete,
  theme,
  active,
}: {
  dataset: DatasetInfo
  source: SpatialSource
  features: Feature[]
  rowsLoaded: number
  scopeLabel: string
  dataKey: string
  dataRevision: number
  loading: boolean
  complete: boolean
  theme: Theme
  active: boolean
}) {
  const initialBasemap: BasemapId = theme === 'dark' ? 'dark' : 'positron'
  const container = useRef<HTMLDivElement | null>(null)
  const mapRef = useRef<MapLibreMap | null>(null)
  const fittedDataKeyRef = useRef<string | null>(null)
  const finalFittedDataKeyRef = useRef<string | null>(null)
  const featuresRef = useRef<Feature[]>([])
  const selectedIdsRef = useRef<FeatureId[]>([])
  const projectionRef = useRef<ProjectionMode>('mercator')
  const basemapRef = useRef<BasemapId>(initialBasemap)
  const basemapAutomatic = useRef(true)
  const boxSelectRef = useRef(false)
  const dragStartRef = useRef<{ x: number; y: number } | null>(null)

  const [featureCount, setFeatureCount] = useState(0)
  const [processedRows, setProcessedRows] = useState(0)
  const [status, setStatus] = useState('Initializing map…')
  const [projection, setProjection] = useState<ProjectionMode>('mercator')
  const [basemap, setBasemap] = useState<BasemapId>(initialBasemap)
  const [boxSelect, setBoxSelect] = useState(false)
  const [selectionBox, setSelectionBox] = useState<SelectionBox | null>(null)
  const [selectedIds, setSelectedIds] = useState<FeatureId[]>([])
  const [selectedProperties, setSelectedProperties] = useState<Record<string, unknown> | null>(null)

  // A hidden tab can leave MapLibre with stale dimensions. Resize after the map
  // becomes visible rather than rebuilding the map.
  useEffect(() => {
    if (!active) return
    const frame = window.requestAnimationFrame(() => mapRef.current?.resize())
    return () => window.cancelAnimationFrame(frame)
  }, [active])

  // Follow the application theme until the user explicitly chooses a basemap.
  useEffect(() => {
    if (!basemapAutomatic.current) return
    const next: BasemapId = theme === 'dark' ? 'dark' : 'positron'
    if (next === basemapRef.current) return

    basemapRef.current = next
    setBasemap(next)
    mapRef.current?.setStyle(BASEMAPS[next].style)
  }, [theme])

  useEffect(() => {
    selectedIdsRef.current = selectedIds
    const map = mapRef.current
    if (map) setSelectionFilter(map, selectedIds)
  }, [selectedIds])

  useEffect(() => {
    boxSelectRef.current = boxSelect
    const map = mapRef.current
    if (!map) return

    if (boxSelect) {
      map.dragPan.disable()
      map.getCanvas().style.cursor = 'crosshair'
    } else {
      map.dragPan.enable()
      map.getCanvas().style.cursor = ''
      dragStartRef.current = null
      setSelectionBox(null)
    }
  }, [boxSelect])

  // MapLibre instance lifecycle. This effect creates the imperative map object,
  // installs interaction handlers, and removes everything during cleanup.
  useEffect(() => {
    if (!container.current) return

    const map = new Map({
      container: container.current,
      style: BASEMAPS[basemapRef.current].style,
      center: [0, 20],
      zoom: 1.4,
      attributionControl: { compact: true },
    })

    mapRef.current = map
    map.addControl(new NavigationControl(), 'top-right')

    const restoreOverlay = () => {
      ensureParquetLayers(map)
      map.setProjection({ type: projectionRef.current })
      setMapFeatures(map, featuresRef.current)
      setSelectionFilter(map, selectedIdsRef.current)
    }

    map.on('style.load', restoreOverlay)

    map.on('click', event => {
      if (boxSelectRef.current || dragStartRef.current) return
      const layers = availableFeatureLayers(map)
      if (layers.length === 0) return
      const hit = map.queryRenderedFeatures(event.point, { layers })
        .find(feature => feature.id != null)

      if (!hit || hit.id == null) {
        setSelectedIds([])
        setSelectedProperties(null)
        return
      }

      setSelectedIds([hit.id])
      setSelectedProperties(featureProperties(hit))
    })

    map.on('mousedown', event => {
      if (!boxSelectRef.current) return
      event.preventDefault()
      dragStartRef.current = { x: event.point.x, y: event.point.y }
      setSelectionBox({ left: event.point.x, top: event.point.y, width: 0, height: 0 })
    })

    map.on('mousemove', event => {
      const start = dragStartRef.current
      if (!boxSelectRef.current || !start) return
      setSelectionBox(rectangleFromPoints(start, event.point))
    })

    map.on('mouseup', event => {
      const start = dragStartRef.current
      if (!boxSelectRef.current || !start) return

      const end = { x: event.point.x, y: event.point.y }
      dragStartRef.current = null
      setSelectionBox(null)

      const left = Math.min(start.x, end.x)
      const right = Math.max(start.x, end.x)
      const top = Math.min(start.y, end.y)
      const bottom = Math.max(start.y, end.y)

      if (right - left < 3 || bottom - top < 3) return

      const layers = availableFeatureLayers(map)
      if (layers.length === 0) return
      const hits = map.queryRenderedFeatures(
        [[left, top], [right, bottom]],
        { layers },
      )
      const ids = uniqueFeatureIds(hits)
      setSelectedIds(ids)
      setSelectedProperties(ids.length === 1
        ? featureProperties(hits.find(feature => feature.id === ids[0]))
        : null)
    })

    return () => {
      map.remove()
      mapRef.current = null
    }
  }, [dataset.dataset_id, source])

  // Render data supplied by App. `dataRevision` is deliberately separate from
  // row-count progress so a large growing GeoJSON source is not rebuilt for
  // every Arrow batch.
  useEffect(() => {
    const map = mapRef.current
    if (!map || !active) return

    const next = features.slice()
    featuresRef.current = next
    setFeatureCount(next.length)
    setSelectedIds([])
    setSelectedProperties(null)
    setMapFeatures(map, next)

    const needsInitialFit = next.length > 0 && fittedDataKeyRef.current !== dataKey
    const needsFinalFit = next.length > 0 && complete && finalFittedDataKeyRef.current !== dataKey
    if (needsInitialFit || needsFinalFit) {
      const bounds = emptyBounds()
      for (const feature of next) extendBoundsWithGeometry(bounds, feature.geometry)
      fitSnapshotBounds(map, bounds)
      fittedDataKeyRef.current = dataKey
      if (complete) finalFittedDataKeyRef.current = dataKey
    }
  }, [active, dataKey, dataRevision, features, complete])

  // Status text can update for every streamed batch without touching MapLibre's
  // GeoJSON source.
  useEffect(() => {
    setProcessedRows(rowsLoaded)
    setStatus(loading
      ? `Loading ${scopeLabel.toLowerCase()}… ${rowsLoaded.toLocaleString()} rows`
      : complete
        ? 'Map ready · pan and zoom do not re-query the file'
        : 'Preparing map data…')
  }, [rowsLoaded, scopeLabel, loading, complete])

  function changeProjection(next: ProjectionMode) {
    if (next === projectionRef.current) return
    projectionRef.current = next
    setProjection(next)
    mapRef.current?.setProjection({ type: next })
  }

  function changeBasemap(next: BasemapId) {
    if (next === basemapRef.current) return
    basemapAutomatic.current = false
    basemapRef.current = next
    setBasemap(next)
    mapRef.current?.setStyle(BASEMAPS[next].style)
  }

  function clearSelection() {
    setSelectedIds([])
    setSelectedProperties(null)
  }

  return (
    <div className="map-shell">
      <div className="map-toolbar">
        <span className="map-source-label">
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
          {' · '}
          <strong>{scopeLabel}</strong> · {rowsLoaded.toLocaleString()} rows loaded
        </span>

        <div className="map-toolbar-controls">
          <button
            type="button"
            className={`secondary compact${boxSelect ? ' active-tool' : ''}`}
            aria-pressed={boxSelect}
            onClick={() => setBoxSelect(current => !current)}
          >
            Box select
          </button>
          {selectedIds.length > 0 && (
            <button type="button" className="secondary compact" onClick={clearSelection}>
              Clear {selectedIds.length.toLocaleString()} selected
            </button>
          )}

          <label className="map-basemap-select">
            <span>Basemap</span>
            <select
              value={basemap}
              aria-label="Basemap"
              onChange={event => changeBasemap(event.target.value as BasemapId)}
            >
              {Object.entries(BASEMAPS).map(([id, option]) => (
                <option key={id} value={id}>{option.label}</option>
              ))}
            </select>
          </label>

          <div className="segmented-control" role="group" aria-label="Map projection">
            <button
              type="button"
              className={projection === 'mercator' ? 'active' : ''}
              aria-pressed={projection === 'mercator'}
              onClick={() => changeProjection('mercator')}
            >
              Map
            </button>
            <button
              type="button"
              className={projection === 'globe' ? 'active' : ''}
              aria-pressed={projection === 'globe'}
              onClick={() => changeProjection('globe')}
            >
              Globe
            </button>
          </div>
        </div>
      </div>

      <div className="map-stage">
        <div ref={container} className="map-canvas" />
        {selectionBox && (
          <div
            className="map-selection-box"
            style={selectionBox}
            aria-hidden="true"
          />
        )}
        <div className="map-status-card">
          {featureCount.toLocaleString()} features · {processedRows.toLocaleString()} rows processed · {status}
        </div>

        {selectedIds.length > 0 && (
          <div className="map-selection-card">
            <div className="map-selection-card-head">
              <strong>{selectedIds.length.toLocaleString()} selected</strong>
              <button type="button" className="icon-button" aria-label="Clear selection" onClick={clearSelection}>×</button>
            </div>
            {selectedProperties && (
              <dl>
                {Object.entries(selectedProperties).slice(0, 10).map(([key, value]) => (
                  <div key={key}>
                    <dt>{key}</dt>
                    <dd title={displayValue(value)}>{displayValue(value)}</dd>
                  </div>
                ))}
              </dl>
            )}
          </div>
        )}
      </div>

      <div className="map-note">
        Map reflects the current table state. Unfiltered data uses the current page; filtered or All results use the shared browser cache. Pan and zoom do not query the file.
      </div>
    </div>
  )
}

function ensureParquetLayers(map: MapLibreMap) {
  if (!map.getSource(SOURCE_ID)) {
    map.addSource(SOURCE_ID, {
      type: 'geojson',
      data: EMPTY,
    })
  }

  if (!map.getLayer('pv-polygons')) {
    map.addLayer({
      id: 'pv-polygons',
      type: 'fill',
      source: SOURCE_ID,
      filter: ['in', ['geometry-type'], ['literal', ['Polygon', 'MultiPolygon']]],
      paint: {
        'fill-color': '#2684ff',
        'fill-opacity': 0.34,
      },
    })
  }

  if (!map.getLayer('pv-lines')) {
    map.addLayer({
      id: 'pv-lines',
      type: 'line',
      source: SOURCE_ID,
      filter: ['in', ['geometry-type'], ['literal', ['LineString', 'MultiLineString']]],
      paint: {
        'line-color': '#168cff',
        'line-width': 1.7,
        'line-opacity': 0.88,
      },
    })
  }

  if (!map.getLayer('pv-points')) {
    map.addLayer({
      id: 'pv-points',
      type: 'circle',
      source: SOURCE_ID,
      filter: ['in', ['geometry-type'], ['literal', ['Point', 'MultiPoint']]],
      paint: {
        'circle-color': '#168cff',
        'circle-radius': 3.4,
        'circle-stroke-color': '#ffffff',
        'circle-stroke-width': 0.7,
        'circle-opacity': 0.84,
      },
    })
  }

  if (!map.getLayer('pv-polygons-selected')) {
    map.addLayer({
      id: 'pv-polygons-selected',
      type: 'fill',
      source: SOURCE_ID,
      filter: emptySelectionFilter(),
      paint: {
        'fill-color': '#ffb020',
        'fill-opacity': 0.52,
        'fill-outline-color': '#fff2c7',
      },
    })
  }

  if (!map.getLayer('pv-lines-selected')) {
    map.addLayer({
      id: 'pv-lines-selected',
      type: 'line',
      source: SOURCE_ID,
      filter: emptySelectionFilter(),
      paint: {
        'line-color': '#ffb020',
        'line-width': 4,
        'line-opacity': 1,
      },
    })
  }

  if (!map.getLayer('pv-points-selected')) {
    map.addLayer({
      id: 'pv-points-selected',
      type: 'circle',
      source: SOURCE_ID,
      filter: emptySelectionFilter(),
      paint: {
        'circle-color': '#ffb020',
        'circle-radius': 6,
        'circle-stroke-color': '#ffffff',
        'circle-stroke-width': 1.5,
        'circle-opacity': 1,
      },
    })
  }
}

function availableFeatureLayers(map: MapLibreMap): string[] {
  return FEATURE_LAYER_IDS.filter(layer => map.getLayer(layer))
}

function setMapFeatures(map: MapLibreMap, features: Feature[]) {
  const source = map.getSource(SOURCE_ID) as GeoJSONSource | undefined
  if (!source) return

  source.setData({
    type: 'FeatureCollection',
    features: features.slice(),
  })
}

function setSelectionFilter(map: MapLibreMap, ids: FeatureId[]) {
  const filter = ids.length
    ? ['in', ['id'], ['literal', ids]]
    : emptySelectionFilter()

  for (const layer of SELECTED_LAYER_IDS) {
    if (map.getLayer(layer)) map.setFilter(layer, filter as never)
  }
}

function emptySelectionFilter() {
  return ['in', ['id'], ['literal', []]] as never
}

function uniqueFeatureIds(features: MapGeoJSONFeature[]): FeatureId[] {
  const seen = new Set<FeatureId>()
  for (const feature of features) {
    if (feature.id != null) seen.add(feature.id)
  }
  return [...seen]
}

function featureProperties(feature?: MapGeoJSONFeature): Record<string, unknown> | null {
  if (!feature) return null
  return { ...feature.properties }
}

function rectangleFromPoints(
  start: { x: number; y: number },
  end: { x: number; y: number },
): SelectionBox {
  return {
    left: Math.min(start.x, end.x),
    top: Math.min(start.y, end.y),
    width: Math.abs(end.x - start.x),
    height: Math.abs(end.y - start.y),
  }
}

function emptyBounds(): BoundsAccumulator {
  return {
    west: Number.POSITIVE_INFINITY,
    south: Number.POSITIVE_INFINITY,
    east: Number.NEGATIVE_INFINITY,
    north: Number.NEGATIVE_INFINITY,
    valid: false,
  }
}

function extendBoundsWithGeometry(bounds: BoundsAccumulator, geometry: Geometry) {
  if (geometry.type === 'GeometryCollection') {
    for (const child of geometry.geometries) extendBoundsWithGeometry(bounds, child)
    return
  }
  visitPositions(geometry.coordinates, position => {
    const x = position[0]
    const y = position[1]
    if (!Number.isFinite(x) || !Number.isFinite(y)) return
    bounds.west = Math.min(bounds.west, x)
    bounds.south = Math.min(bounds.south, y)
    bounds.east = Math.max(bounds.east, x)
    bounds.north = Math.max(bounds.north, y)
    bounds.valid = true
  })
}

function visitPositions(value: Position | Position[] | Position[][] | Position[][][], visit: (position: Position) => void) {
  if (!Array.isArray(value) || value.length === 0) return
  if (typeof value[0] === 'number') {
    visit(value as Position)
    return
  }
  for (const child of value as Position[][][]) visitPositions(child, visit)
}

function fitSnapshotBounds(map: MapLibreMap, bounds: BoundsAccumulator) {
  if (!bounds.valid) return
  const width = bounds.east - bounds.west
  if (width >= 350) {
    map.jumpTo({ center: [0, (bounds.south + bounds.north) / 2], zoom: 1.2 })
    return
  }
  map.fitBounds(
    [[bounds.west, bounds.south], [bounds.east, bounds.north]],
    { padding: 48, duration: 0, maxZoom: 12 },
  )
}

function displayValue(value: unknown): string {
  if (value == null) return '—'
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  try {
    return JSON.stringify(value)
  } catch {
    return String(value)
  }
}
