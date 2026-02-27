import { useEffect, useMemo, useRef, useState } from 'react'
import maplibregl from 'maplibre-gl'

import { getGeoEligibility, getPreview, getSchema, listDatasets, registerDataset, runQuery } from './api'
import type { DatasetRef, QueryResponse, SchemaResponse } from './types'

type Tab = 'data' | 'map'

function DataTable({ result }: { result: QueryResponse | null }) {
  if (!result) return <p>No data loaded.</p>

  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            {result.columns.map((c) => (
              <th key={c}>{c}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {result.rows.map((r, idx) => (
            <tr key={idx}>
              {r.map((v, colIdx) => (
                <td key={colIdx}>{String(v ?? '')}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {result.truncated ? <p className="warn">Result truncated by server limits.</p> : null}
    </div>
  )
}

function MapPanel({ dataset, geomColumn }: { dataset: string; geomColumn: string }) {
  const mapDiv = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (!mapDiv.current) return

    const map = new maplibregl.Map({
      container: mapDiv.current,
      style: 'https://demotiles.maplibre.org/style.json',
      center: [0, 20],
      zoom: 1.5
    })

    map.on('load', () => {
      const sourceId = 'pv-tiles'
      const layerId = 'pv-points'

      map.addSource(sourceId, {
        type: 'vector',
        tiles: [
          `/tiles/{z}/{x}/{y}.mvt?dataset=${encodeURIComponent(dataset)}&geom_column=${encodeURIComponent(geomColumn)}`
        ],
        minzoom: 0,
        maxzoom: 14
      })

      map.addLayer({
        id: layerId,
        type: 'circle',
        source: sourceId,
        'source-layer': 'default',
        paint: {
          'circle-radius': 3,
          'circle-color': '#2563eb',
          'circle-opacity': 0.8
        }
      })
    })

    return () => {
      map.remove()
    }
  }, [dataset, geomColumn])

  return <div ref={mapDiv} className="map" />
}

export default function App() {
  const [tab, setTab] = useState<Tab>('data')
  const [datasets, setDatasets] = useState<DatasetRef[]>([])
  const [dataset, setDataset] = useState('')
  const [schema, setSchema] = useState<SchemaResponse | null>(null)
  const [result, setResult] = useState<QueryResponse | null>(null)
  const [sql, setSql] = useState('SELECT * FROM __dataset LIMIT 100')
  const [datasetId, setDatasetId] = useState('')
  const [datasetUri, setDatasetUri] = useState('')
  const [geomColumn, setGeomColumn] = useState('geom')
  const [geoEligible, setGeoEligible] = useState<boolean>(false)
  const [geoReason, setGeoReason] = useState('')
  const [error, setError] = useState('')

  const selectedDataset = useMemo(() => datasets.find((d) => d.id === dataset), [datasets, dataset])

  async function refreshDatasets() {
    const items = await listDatasets()
    setDatasets(items)
    if (!dataset && items.length > 0) {
      setDataset(items[0].id)
    }
  }

  useEffect(() => {
    void refreshDatasets()
  }, [])

  useEffect(() => {
    if (!dataset) return

    ;(async () => {
      try {
        setError('')
        const [nextSchema, preview, geo] = await Promise.all([
          getSchema(dataset),
          getPreview(dataset, 100),
          getGeoEligibility(dataset, geomColumn)
        ])
        setSchema(nextSchema)
        setResult(preview)
        setGeoEligible(geo.eligible)
        setGeoReason(geo.reason)
      } catch (err) {
        setError(String(err))
      }
    })()
  }, [dataset, geomColumn])

  async function onRegisterDataset() {
    if (!datasetId || !datasetUri) return
    try {
      setError('')
      await registerDataset(datasetId, datasetUri)
      setDatasetId('')
      setDatasetUri('')
      await refreshDatasets()
      setDataset(datasetId)
    } catch (err) {
      setError(String(err))
    }
  }

  async function onRunQuery() {
    if (!dataset) return
    try {
      setError('')
      const response = await runQuery(dataset, sql, 1000)
      setResult(response)
    } catch (err) {
      setError(String(err))
    }
  }

  return (
    <div className="app">
      <header>
        <h1>Parquet Viewer</h1>
        <p>DuckDB-backed explorer with optional MapLibre vector tiles.</p>
      </header>

      <section className="panel">
        <h2>Register dataset</h2>
        <div className="row">
          <input
            placeholder="dataset id"
            value={datasetId}
            onChange={(e) => setDatasetId(e.target.value)}
          />
          <input
            placeholder="s3://bucket/path/*.parquet"
            value={datasetUri}
            onChange={(e) => setDatasetUri(e.target.value)}
          />
          <button onClick={onRegisterDataset}>Register</button>
        </div>
      </section>

      <section className="panel">
        <div className="row">
          <label>Dataset</label>
          <select value={dataset} onChange={(e) => setDataset(e.target.value)}>
            <option value="">Select dataset</option>
            {datasets.map((d) => (
              <option key={d.id} value={d.id}>
                {d.id}
              </option>
            ))}
          </select>
          <label>Geometry column</label>
          <input value={geomColumn} onChange={(e) => setGeomColumn(e.target.value)} />
        </div>
        {selectedDataset ? <p className="muted">{selectedDataset.uri}</p> : null}
      </section>

      <section className="panel tabs">
        <button className={tab === 'data' ? 'active' : ''} onClick={() => setTab('data')}>
          Data
        </button>
        <button className={tab === 'map' ? 'active' : ''} onClick={() => setTab('map')}>
          Map
        </button>
      </section>

      {error ? <p className="error">{error}</p> : null}

      {tab === 'data' ? (
        <section className="panel">
          <h2>Query</h2>
          <textarea value={sql} onChange={(e) => setSql(e.target.value)} rows={5} />
          <div className="row">
            <button onClick={onRunQuery}>Run query</button>
          </div>

          <h3>Schema</h3>
          <ul>
            {schema?.columns.map((c) => (
              <li key={c.name}>
                <strong>{c.name}</strong>: {c.type}
              </li>
            ))}
          </ul>

          <h3>Results</h3>
          <DataTable result={result} />
        </section>
      ) : (
        <section className="panel">
          <h2>Map</h2>
          {geoEligible && dataset ? (
            <MapPanel dataset={dataset} geomColumn={geomColumn} />
          ) : (
            <p className="warn">Map unavailable: {geoReason || 'Dataset is not geo-eligible.'}</p>
          )}
        </section>
      )}
    </div>
  )
}
