import { useEffect, useMemo, useRef, useState } from 'react'
import * as maplibregl from 'maplibre-gl'
import type { GeoJSONSource, Map as MapLibreMap } from 'maplibre-gl'
import type { Feature, FeatureCollection, Geometry } from 'geojson'
import { spatialBatches } from './api'
import { plainValue } from './arrow'
import { parseWkb } from './wkb'
import type { DatasetInfo, GeoColumnInfo } from './types'

const SOURCE_ID = 'parquet-features'
const EMPTY: FeatureCollection = { type: 'FeatureCollection', features: [] }

export function MapPanel({ dataset }: { dataset: DatasetInfo }) {
  const container = useRef<HTMLDivElement | null>(null)
  const mapRef = useRef<MapLibreMap | null>(null)
  const requestId = useRef(0)
  const [featureCount, setFeatureCount] = useState(0)
  const [status, setStatus] = useState('Initializing globe…')

  const geometry = useMemo(() => pickGeometry(dataset), [dataset])

  useEffect(() => {
    if (!container.current || !geometry) return
    const map = new maplibregl.Map({
      container: container.current,
      style: 'https://demotiles.maplibre.org/style.json',
      center: [0, 20],
      zoom: 1.4,
      attributionControl: { compact: true },
    })
    mapRef.current = map
    map.addControl(new maplibregl.NavigationControl(), 'top-right')
    map.on('load', () => {
      map.setProjection({ type: 'globe' })
      map.addSource(SOURCE_ID, { type: 'geojson', data: EMPTY })
      map.addLayer({
        id: 'pv-polygons', type: 'fill', source: SOURCE_ID,
        filter: ['in', ['geometry-type'], ['literal', ['Polygon', 'MultiPolygon']]],
        paint: { 'fill-color': '#2f80ed', 'fill-opacity': 0.34 },
      })
      map.addLayer({
        id: 'pv-lines', type: 'line', source: SOURCE_ID,
        filter: ['in', ['geometry-type'], ['literal', ['LineString', 'MultiLineString']]],
        paint: { 'line-color': '#57a0ff', 'line-width': 1.5, 'line-opacity': 0.8 },
      })
      map.addLayer({
        id: 'pv-points', type: 'circle', source: SOURCE_ID,
        filter: ['in', ['geometry-type'], ['literal', ['Point', 'MultiPoint']]],
        paint: { 'circle-color': '#68d7ff', 'circle-radius': 3.2, 'circle-opacity': 0.78 },
      })

      fitDataset(map, geometry)
      void refreshViewport(map, geometry)
      map.on('moveend', () => void refreshViewport(map, geometry))
    })

    async function refreshViewport(mapInstance: MapLibreMap, geo: GeoColumnInfo) {
      const thisRequest = ++requestId.current
      const bounds = mapInstance.getBounds()
      const bbox: [number, number, number, number] = [
        bounds.getWest(), bounds.getSouth(), bounds.getEast(), bounds.getNorth(),
      ]
      setStatus('Loading viewport…')
      try {
        const features: Feature[] = []
        const propertyColumns = dataset.columns
          .map(column => column.name)
          .filter(name => name !== geo.name)
          .slice(0, 6)
        const columns = [geo.name, ...propertyColumns]
        for await (const batch of spatialBatches(dataset.dataset_id, {
          bbox, geometry_column: geo.name, columns, max_features: 20_000,
        })) {
          const geometryVector = batch.getChild(geo.name)
          if (!geometryVector) continue
          for (let row = 0; row < batch.numRows; row += 1) {
            const bytes = geometryVector.get(row)
            if (!(bytes instanceof Uint8Array)) continue
            const parsed = parseWkb(bytes)
            if (!parsed) continue
            const properties: Record<string, unknown> = {}
            for (const name of propertyColumns) properties[name] = plainValue(batch.getChild(name)?.get(row))
            features.push({ type: 'Feature', geometry: parsed as Geometry, properties })
          }
        }
        if (thisRequest !== requestId.current) return
        const collection: FeatureCollection = { type: 'FeatureCollection', features }
        ;(mapInstance.getSource(SOURCE_ID) as GeoJSONSource | undefined)?.setData(collection)
        setFeatureCount(features.length)
        setStatus(dataset.capabilities.spatial_exact ? 'Exact viewport filter' : 'Row-group candidates')
      } catch (error) {
        if (thisRequest === requestId.current) setStatus(String(error))
      }
    }

    return () => { requestId.current += 1; map.remove(); mapRef.current = null }
  }, [dataset, geometry])

  if (!geometry) return <div className="empty-state">No native GeoParquet 2 GEOMETRY/GEOGRAPHY column detected.</div>

  return (
    <div className="map-shell">
      <div className="map-toolbar">
        <span><strong>{geometry.name}</strong> · {geometry.logical_type}</span>
        <span>{featureCount.toLocaleString()} features · {status}</span>
      </div>
      <div ref={container} className="map-canvas" />
      <div className="map-note">
        MVP renderer: Arrow IPC → bounded WKB decode → MapLibre globe. The backend contract is already Arrow/WKB; swap this adapter for GeoArrow-Wasm + deck.gl for million-feature rendering.
      </div>
    </div>
  )
}

function pickGeometry(dataset: DatasetInfo): GeoColumnInfo | undefined {
  return dataset.geo_columns.find(column => column.is_primary) ?? dataset.geo_columns[0]
}

function fitDataset(map: MapLibreMap, geometry: GeoColumnInfo) {
  const bbox = geometry.dataset_bbox
  if (!bbox || bbox.length < 4) return
  map.fitBounds([[bbox[0], bbox[1]], [bbox[2], bbox[3]]], { padding: 40, duration: 0, maxZoom: 7 })
}
