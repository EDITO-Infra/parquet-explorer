import { useEffect, useMemo, useRef, useState } from 'react'
import type { Feature } from 'geojson'
import { batchRows, type PlainRow } from './arrow'
import { closeDataset, countRows, downloadSubset, openDataset, pageBatches } from './api'
import { ColumnPicker } from './ColumnPicker'
import { DataTable } from './DataTable'
import { FilterBar } from './FilterBar'
import { MapPanel } from './MapPanel'
import { detectSpatialSource } from './spatial'
import { appendSpatialBatchFeatures, spatialColumnNames } from './spatialFeatures'
import type { DatasetInfo, FilterClause, MapSnapshotSpec } from './types'

type Tab = 'data' | 'map' | 'schema'
type Theme = 'light' | 'dark'

export default function App() {
  const [uri, setUri] = useState('')
  const [dataset, setDataset] = useState<DatasetInfo | null>(null)
  const [tab, setTab] = useState<Tab>('data')
  const [offset, setOffset] = useState(0)
  const [limit, setLimit] = useState(1000)
  const [rows, setRows] = useState<PlainRow[]>([])
  const [visibleColumns, setVisibleColumns] = useState<string[]>([])
  const [filters, setFilters] = useState<FilterClause[]>([])
  const [totalRows, setTotalRows] = useState<number | null>(null)
  const [countLoading, setCountLoading] = useState(false)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [theme, setTheme] = useState<Theme>(getInitialTheme)
  const [mapSnapshot, setMapSnapshot] = useState<MapSnapshotSpec | null>(null)
  const [mapPageFeatures, setMapPageFeatures] = useState<Feature[]>([])
  const [mapSnapshotSeed, setMapSnapshotSeed] = useState<Feature[]>([])
  const pageRequestId = useRef(0)
  const mapSnapshotId = useRef(0)

  const spatialSource = useMemo(
    () => dataset ? detectSpatialSource(dataset) : undefined,
    [dataset],
  )

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    window.localStorage.setItem('parquet-viewer-theme', theme)
  }, [theme])

  useEffect(() => {
    if (!dataset) return
    void loadPage(dataset, offset, limit, visibleColumns)
  }, [dataset?.dataset_id, offset, limit, filters, visibleColumns])

  useEffect(() => {
    if (!dataset) return
    if (filters.length === 0) {
      setTotalRows(dataset.num_rows)
      setCountLoading(false)
      return
    }

    let cancelled = false
    setCountLoading(true)
    setTotalRows(null)

    void countRows(dataset.dataset_id, filters)
      .then(count => { if (!cancelled) setTotalRows(count) })
      .catch(err => { if (!cancelled) setError(String(err)) })
      .finally(() => { if (!cancelled) setCountLoading(false) })

    return () => { cancelled = true }
  }, [dataset?.dataset_id, filters])

  async function onOpen(event: React.FormEvent) {
    event.preventDefault()
    if (!uri.trim()) return

    setLoading(true)
    setError('')

    try {
      if (dataset) {
        await closeDataset(dataset.dataset_id).catch(() => undefined)
      }

      const info = await openDataset(uri.trim())

      setDataset(info)
      setOffset(0)
      setFilters([])
      setRows([])
      setVisibleColumns(info.columns.map(column => column.name))
      setTotalRows(info.num_rows)
      setMapSnapshot(null)
      setMapPageFeatures([])
      setMapSnapshotSeed([])
      setTab('data')
    } catch (err) {
      setError(String(err))
    } finally {
      setLoading(false)
    }
  }

  async function loadPage(
    info: DatasetInfo,
    pageOffset: number,
    pageLimit: number,
    pageColumns: string[],
  ) {
    const requestId = ++pageRequestId.current

    const pageSpatialSource = detectSpatialSource(info)
    // Numeric coordinate pairs are cheap enough to keep with the table page so
    // Switch to map can paint those rows immediately. Large WKB geometry stays
    // out of the table projection unless the user already chose to display it.
    const requiredSpatialColumns = pageSpatialSource?.kind === 'coordinates'
      ? spatialColumnNames(pageSpatialSource)
      : pageSpatialSource && pageColumns.includes(pageSpatialSource.geometry.name)
        ? [pageSpatialSource.geometry.name]
        : []
    const requestColumns = [...new Set([...pageColumns, ...requiredSpatialColumns])]

    if (requestColumns.length === 0) {
      setRows([])
      setMapPageFeatures([])
      setLoading(false)
      return
    }

    setLoading(true)
    setError('')

    try {
      const nextRows: PlainRow[] = []
      const nextMapFeatures: Feature[] = []
      let rowsRead = 0
      const spatialSet = new Set(requiredSpatialColumns)
      const mapPropertyColumns = pageColumns.filter(name => !spatialSet.has(name)).slice(0, 8)

      for await (
        const batch of pageBatches(info.dataset_id, {
          columns: requestColumns,
          offset: pageOffset,
          limit: pageLimit,
          filters,
        })
      ) {
        nextRows.push(...batchRows(batch, pageLimit - nextRows.length))
        if (pageSpatialSource) {
          appendSpatialBatchFeatures(
            batch,
            pageSpatialSource,
            mapPropertyColumns,
            nextMapFeatures,
            pageOffset + rowsRead,
          )
        }
        rowsRead += batch.numRows
        if (nextRows.length >= pageLimit) break
      }

      if (requestId === pageRequestId.current) {
        setRows(nextRows)
        setMapPageFeatures(nextMapFeatures)
      }
    } catch (err) {
      if (requestId === pageRequestId.current) {
        setMapPageFeatures([])
        setError(String(err))
      }
    } finally {
      if (requestId === pageRequestId.current) setLoading(false)
    }
  }

  function switchToMap() {
    if (!dataset || !spatialSource || totalRows == null || countLoading) return

    setMapSnapshotSeed(mapPageFeatures.slice())
    setMapSnapshot({
      id: ++mapSnapshotId.current,
      filters: filters.map(filter => ({ ...filter })),
      expectedRows: totalRows,
    })
    setTab('map')
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div>
          <h1>Parquet Viewer</h1>
        </div>
        <div className="topbar-actions">
          {dataset && (
            <div className="dataset-pill">
              {dataset.num_rows.toLocaleString()} rows · {dataset.num_row_groups} row groups
            </div>
          )}
          <button
            type="button"
            className="theme-toggle secondary"
            aria-label={`Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`}
            aria-pressed={theme === 'dark'}
            onClick={() => setTheme(current => current === 'dark' ? 'light' : 'dark')}
          >
            <span aria-hidden="true">{theme === 'dark' ? '☾' : '☀'}</span>
            {theme === 'dark' ? 'Dark' : 'Light'}
          </button>
        </div>
      </header>

      <form className="open-bar" onSubmit={onOpen}>
        <input
          value={uri}
          onChange={event => setUri(event.target.value)}
          placeholder="https://…/dataset.parquet"
          aria-label="Parquet URL"
        />
        <button disabled={loading || !uri.trim()}>
          {loading ? 'Working…' : 'Open Parquet'}
        </button>
      </form>

      {error && <div className="error-banner">{error}</div>}

      {!dataset ? (
        <main className="landing">
          <div className="landing-card">
            <span className="eyebrow">Parquet + GeoParquet</span>
            <h2>Paste a Parquet URL.</h2>
            <p>
              The Rust backend range-reads Parquet. Table data arrives as Arrow IPC;
              coordinate pairs and geometry columns can be explored on an interactive map or globe.
            </p>
          </div>
        </main>
      ) : (
        <main className="workspace">
          <aside className="sidebar">
            <div className="side-section">
              <span className="section-label">Dataset</span>
              <div className="uri" title={dataset.uri}>
                {dataset.name || filename(dataset.uri)}
              </div>
              <div className="muted mono">{dataset.dataset_id.slice(0, 12)}</div>
            </div>

            <div className="side-section stats-grid">
              <Metric label="Rows" value={dataset.num_rows.toLocaleString()} />
              <Metric label="Columns" value={String(dataset.columns.length)} />
              <Metric label="Row groups" value={String(dataset.num_row_groups)} />
              <Metric
                label="Spatial"
                value={spatialSource ? spatialSource.kind : 'none'}
              />
            </div>

            {spatialSource && (
              <div className="side-section">
                <span className="section-label">Spatial source</span>
                {spatialSource.kind === 'coordinates' ? (
                  <dl className="metadata-list">
                    <dt>Type</dt><dd>Coordinates</dd>
                    <dt>Longitude</dt><dd>{spatialSource.longitude}</dd>
                    <dt>Latitude</dt><dd>{spatialSource.latitude}</dd>
                  </dl>
                ) : (
                  <dl className="metadata-list">
                    <dt>Column</dt><dd>{spatialSource.geometry.name}</dd>
                    <dt>Type</dt><dd>{spatialSource.geometry.logical_type}</dd>
                    <dt>CRS</dt><dd>{spatialSource.geometry.crs}</dd>
                    <dt>Statistics</dt>
                    <dd>
                      {spatialSource.geometry.row_groups_with_bbox}/
                      {spatialSource.geometry.row_groups_total}
                    </dd>
                  </dl>
                )}
              </div>
            )}

            <div className="side-section">
              <span className="section-label">Export</span>
              <div className="button-stack">
                <button
                  className="secondary"
                  onClick={() => void downloadSubset(
                    dataset.dataset_id,
                    'parquet',
                    undefined,
                    filters,
                  )}
                >
                  Download Parquet
                </button>
                <button
                  className="secondary"
                  onClick={() => void downloadSubset(
                    dataset.dataset_id,
                    'arrow',
                    undefined,
                    filters,
                  )}
                >
                  Download Arrow
                </button>
              </div>
            </div>
          </aside>

          <section className="content">
            <nav className="tabs">
              <TabButton active={tab === 'data'} onClick={() => setTab('data')}>
                Data
              </TabButton>
              <TabButton
                active={tab === 'map'}
                disabled={!spatialSource || !mapSnapshot}
                onClick={() => setTab('map')}
              >
                Map
              </TabButton>
              <TabButton active={tab === 'schema'} onClick={() => setTab('schema')}>
                Schema
              </TabButton>
            </nav>

            {tab === 'data' && (
              <section className="pane">
                <FilterBar
                  columns={dataset.columns}
                  filters={filters}
                  disabled={loading}
                  onApply={next => {
                    setOffset(0)
                    setFilters(next)
                  }}
                />

                <div className="pane-toolbar">
                  <div className="pager">
                    <button
                      className="secondary"
                      disabled={offset === 0 || loading || visibleColumns.length === 0}
                      onClick={() => setOffset(Math.max(0, offset - limit))}
                    >
                      Previous
                    </button>
                    <span>
                      {visibleColumns.length === 0
                        ? `No columns selected · ${countLoading || totalRows == null ? 'counting…' : totalRows.toLocaleString()}${filters.length ? ' matching' : ' rows'}`
                        : `${rows.length ? `${(offset + 1).toLocaleString()}–${(offset + rows.length).toLocaleString()}` : '0 rows'} of ${countLoading || totalRows == null ? 'counting…' : totalRows.toLocaleString()}${filters.length ? ' matching' : ''}`}
                    </span>
                    <button
                      className="secondary"
                      disabled={
                        loading
                        || countLoading
                        || visibleColumns.length === 0
                        || totalRows == null
                        || offset + rows.length >= totalRows
                      }
                      onClick={() => setOffset(offset + limit)}
                    >
                      Next
                    </button>
                  </div>

                  <div className="table-controls">
                    {spatialSource && (
                      <button
                        type="button"
                        className="map-switch-button"
                        disabled={loading || countLoading || totalRows == null}
                        onClick={switchToMap}
                        title="Freeze the current filtered result and render the complete spatial selection on the map"
                      >
                        {countLoading
                          ? 'Counting…'
                          : mapSnapshot && sameFilters(mapSnapshot.filters, filters)
                            ? 'Refresh map snapshot'
                            : 'Switch to map'}
                      </button>
                    )}
                    <ColumnPicker
                      columns={dataset.columns}
                      visibleColumns={visibleColumns}
                      disabled={loading}
                      onChange={next => {
                        setOffset(0)
                        setVisibleColumns(next)
                      }}
                    />
                    <label>
                      Rows/page{' '}
                      <select
                        value={limit}
                        disabled={loading}
                        onChange={event => {
                          setOffset(0)
                          setLimit(Number(event.target.value))
                        }}
                      >
                        <option>250</option>
                        <option>1000</option>
                        <option>5000</option>
                        <option>10000</option>
                      </select>
                    </label>
                  </div>
                </div>

                <DataTable columns={visibleColumns} rows={rows} />
              </section>
            )}

            {spatialSource && mapSnapshot && (
              <section
                className={`pane map-pane${tab === 'map' ? '' : ' pane-hidden'}`}
                aria-hidden={tab !== 'map'}
              >
                <MapPanel
                  dataset={dataset}
                  source={spatialSource}
                  snapshot={mapSnapshot}
                  seedFeatures={mapSnapshotSeed}
                  theme={theme}
                  active={tab === 'map'}
                />
              </section>
            )}

            {tab === 'schema' && (
              <section className="pane schema-pane">
                <div className="schema-summary">
                  <div>
                    <span className="section-label">GeoParquet</span>
                    <strong>
                      {dataset.geo_parquet?.version ?? 'native geospatial Parquet / none'}
                    </strong>
                  </div>
                  <div>
                    <span className="section-label">V2 checks</span>
                    <strong>
                      {dataset.geo_parquet?.v2_metadata_checks_passed
                        ? 'Passed'
                        : 'Not confirmed'}
                    </strong>
                  </div>
                  <div>
                    <span className="section-label">Exact spatial</span>
                    <strong>
                      {dataset.capabilities.spatial_exact
                        ? 'Enabled'
                        : 'Geometry path not exact'}
                    </strong>
                  </div>
                </div>

                {dataset.geo_parquet?.warnings.length ? (
                  <div className="warning-box">
                    <strong>Metadata warnings</strong>
                    {dataset.geo_parquet.warnings.map(warning => (
                      <div key={warning}>{warning}</div>
                    ))}
                  </div>
                ) : null}

                <div className="schema-table">
                  <div className="schema-row schema-head">
                    <span>Name</span>
                    <span>Arrow type</span>
                    <span>Parquet physical</span>
                    <span>Nullable</span>
                  </div>
                  {dataset.columns.map(column => (
                    <div className="schema-row" key={column.name}>
                      <span>{column.name}</span>
                      <span className="mono">{column.arrow_type}</span>
                      <span>{column.parquet_physical_type ?? '—'}</span>
                      <span>{column.nullable ? 'yes' : 'no'}</span>
                    </div>
                  ))}
                </div>
              </section>
            )}
          </section>
        </main>
      )}
    </div>
  )
}

function TabButton({
  active,
  disabled,
  onClick,
  children,
}: {
  active: boolean
  disabled?: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      className={active ? 'active' : ''}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <span className="metric-value">{value}</span>
      <span className="metric-label">{label}</span>
    </div>
  )
}

function getInitialTheme(): Theme {
  const saved = window.localStorage.getItem('parquet-viewer-theme')
  if (saved === 'light' || saved === 'dark') return saved
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

function sameFilters(left: FilterClause[], right: FilterClause[]): boolean {
  return JSON.stringify(left) === JSON.stringify(right)
}

function filename(uri: string) {
  try {
    return new URL(uri).pathname.split('/').filter(Boolean).pop() ?? uri
  } catch {
    return uri
  }
}
