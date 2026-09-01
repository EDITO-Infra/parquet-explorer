import { useEffect, useMemo, useRef, useState } from 'react'
import type { Feature } from 'geojson'
import { batchRows, type PlainRow } from './arrow'
import { closeDataset, countRows, downloadSubset, getTrace, openDataset, pageBatches } from './api'
import { ColumnPicker } from './ColumnPicker'
import { DataTable } from './DataTable'
import { FilterBar } from './FilterBar'
import { MapPanel, type MapActivityUpdate } from './MapPanel'
import { ProgressPanel, type ActivityState } from './ProgressPanel'
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
  const [opening, setOpening] = useState(false)
  const [error, setError] = useState('')
  const [theme, setTheme] = useState<Theme>(getInitialTheme)
  const [mapSnapshot, setMapSnapshot] = useState<MapSnapshotSpec | null>(null)
  const [mapPageFeatures, setMapPageFeatures] = useState<Feature[]>([])
  const [mapSnapshotSeed, setMapSnapshotSeed] = useState<Feature[]>([])
  const [activity, setActivity] = useState<ActivityState | null>(null)
  const pageRequestId = useRef(0)
  const mapSnapshotId = useRef(0)
  const activeTraceId = useRef<string | null>(null)

  const spatialSource = useMemo(
    () => dataset ? detectSpatialSource(dataset) : undefined,
    [dataset],
  )

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    window.localStorage.setItem('parquet-viewer-theme', theme)
  }, [theme])

  useEffect(() => {
    if (!activity || activity.status !== 'running') return
    const timer = window.setInterval(() => {
      setActivity(current => current?.status === 'running'
        ? { ...current, elapsedMs: performance.now() - current.startedAt }
        : current)
    }, 100)
    return () => window.clearInterval(timer)
  }, [activity?.traceId, activity?.status])

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

  function beginActivity(traceId: string, title: string, current: string) {
    activeTraceId.current = traceId
    const startedAt = performance.now()
    setActivity({
      traceId,
      title,
      current,
      status: 'running',
      startedAt,
      elapsedMs: 0,
      clientEvents: [],
    })
    void monitorTrace(traceId)
  }

  function addClientActivity(
    traceId: string,
    message: string,
    detail?: string,
    progress?: number,
    durationMs?: number,
  ) {
    setActivity(current => {
      if (!current || current.traceId !== traceId) return current
      const elapsedMs = performance.now() - current.startedAt
      return {
        ...current,
        current: message,
        progress: progress ?? current.progress,
        elapsedMs,
        clientEvents: [...current.clientEvents, {
          message,
          detail,
          elapsedMs,
          durationMs,
          source: 'Browser',
        }],
      }
    })
  }

  function finishActivity(traceId: string, message: string, detail?: string) {
    setActivity(current => {
      if (!current || current.traceId !== traceId) return current
      const elapsedMs = performance.now() - current.startedAt
      return {
        ...current,
        current: message,
        status: 'complete',
        progress: 1,
        elapsedMs,
        clientEvents: [...current.clientEvents, {
          message,
          detail,
          elapsedMs,
          source: 'Browser',
        }],
      }
    })
  }

  function failActivity(traceId: string, message: string) {
    setActivity(current => {
      if (!current || current.traceId !== traceId) return current
      return {
        ...current,
        current: message,
        status: 'error',
        elapsedMs: performance.now() - current.startedAt,
      }
    })
  }

  async function monitorTrace(traceId: string) {
    let seen = false
    for (let attempt = 0; attempt < 900; attempt += 1) {
      if (activeTraceId.current !== traceId) return
      try {
        const trace = await getTrace(traceId)
        if (trace) {
          seen = true
          setActivity(current => {
            if (!current || current.traceId !== traceId) return current
            const last = trace.events.at(-1)
            return {
              ...current,
              trace,
              current: trace.status === 'running' && last ? last.message : current.current,
              elapsedMs: Math.max(current.elapsedMs, trace.elapsed_ms),
            }
          })
          if (trace.status !== 'running') return
        }
      } catch {
        // Diagnostics must never interrupt the actual query.
      }
      if (!seen && attempt >= 40) return
      await delay(120)
    }
  }

  async function onOpen(event: React.FormEvent) {
    event.preventDefault()
    if (!uri.trim()) return

    const traceId = makeTraceId('open')
    beginActivity(traceId, 'Opening file', 'Checking source')
    setOpening(true)
    setError('')

    try {
      if (dataset) {
        await closeDataset(dataset.dataset_id).catch(() => undefined)
      }

      const info = await openDataset(uri.trim(), undefined, traceId)
      addClientActivity(traceId, 'Preparing the viewer', `${info.columns.length} fields found`)

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
      finishActivity(traceId, 'File opened', `${info.num_rows.toLocaleString()} rows ready to explore`)
    } catch (err) {
      failActivity(traceId, 'Could not open file')
      setError(String(err))
    } finally {
      setOpening(false)
    }
  }

  async function loadPage(
    info: DatasetInfo,
    pageOffset: number,
    pageLimit: number,
    pageColumns: string[],
  ) {
    const requestId = ++pageRequestId.current
    const traceId = makeTraceId('page')

    const pageSpatialSource = detectSpatialSource(info)
    // Keep coordinate pairs available for a fast map preview. Large WKB geometry
    // stays out of the table projection unless it is already visible.
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

    beginActivity(traceId, 'Loading rows', `Fetching rows ${(pageOffset + 1).toLocaleString()}–${(pageOffset + pageLimit).toLocaleString()}`)
    setLoading(true)
    setError('')

    try {
      const nextRows: PlainRow[] = []
      const nextMapFeatures: Feature[] = []
      let rowsRead = 0
      const spatialSet = new Set(requiredSpatialColumns)
      const mapPropertyColumns = pageColumns.filter(name => !spatialSet.has(name)).slice(0, 8)

      for await (
        const batch of pageBatches(
          info.dataset_id,
          {
            trace_id: traceId,
            columns: requestColumns,
            offset: pageOffset,
            limit: pageLimit,
            filters,
          },
          diagnostic => addClientActivity(
            traceId,
            diagnostic.message,
            diagnostic.detail,
            undefined,
            diagnostic.durationMs,
          ),
        )
      ) {
        const tableDecodeStarted = performance.now()
        nextRows.push(...batchRows(batch, pageLimit - nextRows.length))
        addClientActivity(
          traceId,
          'Building table rows',
          `${Math.min(pageLimit, rowsRead + batch.numRows).toLocaleString()} of ${pageLimit.toLocaleString()} requested`,
          Math.min(0.94, (rowsRead + batch.numRows) / Math.max(1, pageLimit)),
          performance.now() - tableDecodeStarted,
        )

        if (pageSpatialSource) {
          const featureCountBefore = nextMapFeatures.length
          const geometryStarted = performance.now()
          appendSpatialBatchFeatures(
            batch,
            pageSpatialSource,
            mapPropertyColumns,
            nextMapFeatures,
            pageOffset + rowsRead,
          )
          addClientActivity(
            traceId,
            pageSpatialSource.kind === 'geometry' ? 'Decoding WKB geometries' : 'Building coordinate geometries',
            `${(nextMapFeatures.length - featureCountBefore).toLocaleString()} features from ${batch.numRows.toLocaleString()} rows`,
            Math.min(0.98, (rowsRead + batch.numRows) / Math.max(1, pageLimit)),
            performance.now() - geometryStarted,
          )
        }
        rowsRead += batch.numRows
        if (nextRows.length >= pageLimit) break
      }

      if (pageSpatialSource && nextMapFeatures.length > 0) {
        addClientActivity(
          traceId,
          'Preparing map preview',
          `${nextMapFeatures.length.toLocaleString()} geometries ready`,
          0.98,
        )
      }

      if (requestId === pageRequestId.current) {
        setRows(nextRows)
        setMapPageFeatures(nextMapFeatures)
        finishActivity(
          traceId,
          'Rows ready',
          `${nextRows.length.toLocaleString()} rows loaded into the table`,
        )
      }
    } catch (err) {
      if (requestId === pageRequestId.current) {
        setMapPageFeatures([])
        failActivity(traceId, 'Loading rows failed')
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

  function handleMapActivity(update: MapActivityUpdate) {
    if (update.type === 'start') {
      beginActivity(update.traceId, 'Loading map', update.message)
      return
    }
    if (update.type === 'progress') {
      addClientActivity(update.traceId, update.message, update.detail, update.progress, update.durationMs)
      return
    }
    if (update.type === 'complete') {
      finishActivity(update.traceId, update.message, update.detail)
      return
    }
    failActivity(update.traceId, update.message)
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-block">
          <div className="brand-mark" aria-hidden="true">P</div>
          <div>
            <h1>Parquet Viewer</h1>
            <p>Open, inspect, filter and map Parquet files.</p>
          </div>
        </div>
        <div className="topbar-actions">
          {dataset && (
            <div className="dataset-pill" title={dataset.uri}>
              {dataset.name || filename(dataset.uri)}
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
        <div className="open-input-wrap">
          <span className="open-input-label">Parquet URL</span>
          <input
            value={uri}
            onChange={event => setUri(event.target.value)}
            placeholder="https://example.com/data.parquet"
            aria-label="Parquet URL"
          />
        </div>
        <button disabled={opening || !uri.trim()}>
          {opening ? 'Opening…' : 'Open file'}
        </button>
      </form>

      {error && <div className="error-banner">{error}</div>}
      <ProgressPanel activity={activity} />

      {!dataset ? (
        <main className="landing">
          <div className="landing-card">
            <span className="eyebrow">Parquet + GeoParquet</span>
            <h2>Explore a Parquet file without downloading it first.</h2>
            <p>
              Paste a file URL to preview rows, filter values, inspect the schema,
              and map spatial data when it is available.
            </p>
            <div className="landing-features" aria-label="Viewer features">
              <span>Preview rows</span>
              <span>Filter data</span>
              <span>Inspect fields</span>
              <span>Map geometry</span>
            </div>
          </div>
        </main>
      ) : (
        <main className="workspace">
          <aside className="sidebar">
            <div className="side-section dataset-summary">
              <span className="section-label">Current file</span>
              <div className="uri" title={dataset.uri}>
                {dataset.name || filename(dataset.uri)}
              </div>
              <div className="muted source-uri" title={dataset.uri}>{dataset.uri}</div>
            </div>

            <details className="side-section file-details">
              <summary>File details</summary>
              <div className="stats-grid compact-stats">
                <Metric label="Rows" value={dataset.num_rows.toLocaleString()} />
                <Metric label="Fields" value={String(dataset.columns.length)} />
                <Metric label="Row groups" value={String(dataset.num_row_groups)} />
                <Metric label="Spatial" value={spatialSource ? 'Yes' : 'No'} />
              </div>

              {spatialSource && (
                <div className="spatial-details">
                  <span className="section-label">Spatial data</span>
                  {spatialSource.kind === 'coordinates' ? (
                    <dl className="metadata-list">
                      <dt>Type</dt><dd>Coordinates</dd>
                      <dt>Longitude</dt><dd>{spatialSource.longitude}</dd>
                      <dt>Latitude</dt><dd>{spatialSource.latitude}</dd>
                    </dl>
                  ) : (
                    <dl className="metadata-list">
                      <dt>Field</dt><dd>{spatialSource.geometry.name}</dd>
                      <dt>Type</dt><dd>{spatialSource.geometry.logical_type}</dd>
                      <dt>CRS</dt><dd>{spatialSource.geometry.crs}</dd>
                      <dt>Bbox stats</dt>
                      <dd>
                        {spatialSource.geometry.row_groups_with_bbox}/
                        {spatialSource.geometry.row_groups_total} row groups
                      </dd>
                    </dl>
                  )}
                </div>
              )}
            </details>

            <div className="side-section">
              <span className="section-label">Download</span>
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
                  Save as Parquet
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
                  Save as Arrow
                </button>
              </div>
            </div>
          </aside>

          <section className="content">
            <nav className="tabs">
              <TabButton active={tab === 'data'} onClick={() => setTab('data')}>
                Table
              </TabButton>
              <TabButton
                active={tab === 'map'}
                disabled={!spatialSource || !mapSnapshot}
                onClick={() => setTab('map')}
              >
                Map
              </TabButton>
              <TabButton active={tab === 'schema'} onClick={() => setTab('schema')}>
                Schema &amp; stats
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
                        title="Create a map from the current filtered result"
                      >
                        {countLoading
                          ? 'Counting…'
                          : mapSnapshot && sameFilters(mapSnapshot.filters, filters)
                            ? 'Refresh map'
                            : 'View on map'}
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
                      Rows per page{' '}
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
                  onActivity={handleMapActivity}
                />
              </section>
            )}

            {tab === 'schema' && (
              <section className="pane schema-pane">
                <div className="schema-intro">
                  <div>
                    <span className="eyebrow">File overview</span>
                    <h2>Schema &amp; statistics</h2>
                    <p>Structure, storage layout, and spatial metadata for this file.</p>
                  </div>
                </div>

                <div className="schema-summary schema-stats">
                  <StatCard label="Rows" value={dataset.num_rows.toLocaleString()} help="Total rows reported by the file metadata" />
                  <StatCard label="Fields" value={String(dataset.columns.length)} help="Columns available to query" />
                  <StatCard label="Row groups" value={String(dataset.num_row_groups)} help="Parquet storage groups used for reads" />
                  <StatCard label="Spatial" value={spatialSource ? 'Available' : 'Not detected'} help={spatialSource ? 'Map view can be created' : 'No geometry or coordinate pair detected'} />
                </div>

                {dataset.geo_parquet && (
                  <div className="schema-summary technical-stats">
                    <StatCard label="GeoParquet" value={dataset.geo_parquet.version} help={`Primary field: ${dataset.geo_parquet.primary_column}`} />
                    <StatCard label="Metadata checks" value={dataset.geo_parquet.v2_metadata_checks_passed ? 'Passed' : 'Review'} help="GeoParquet metadata validation" />
                    <StatCard label="Exact spatial filter" value={dataset.capabilities.spatial_exact ? 'Enabled' : 'Not yet'} help="Current map query capability" />
                  </div>
                )}

                {dataset.geo_parquet?.warnings.length ? (
                  <div className="warning-box">
                    <strong>Metadata notes</strong>
                    {dataset.geo_parquet.warnings.map(warning => (
                      <div key={warning}>{warning}</div>
                    ))}
                  </div>
                ) : null}

                <div className="schema-section-head">
                  <div>
                    <h3>Fields</h3>
                    <p>The logical and physical representation of each column.</p>
                  </div>
                </div>
                <div className="schema-table">
                  <div className="schema-row schema-head">
                    <span>Field</span>
                    <span>Data type</span>
                    <span>Parquet storage</span>
                    <span>Optional</span>
                  </div>
                  {dataset.columns.map(column => (
                    <div className="schema-row" key={column.name}>
                      <span>{column.name}</span>
                      <span className="mono">{column.arrow_type}</span>
                      <span>{column.parquet_physical_type ?? '—'}</span>
                      <span>{column.nullable ? 'Yes' : 'No'}</span>
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

function StatCard({ label, value, help }: { label: string; value: string; help: string }) {
  return (
    <div className="stat-card">
      <span className="section-label">{label}</span>
      <strong>{value}</strong>
      <p>{help}</p>
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

function makeTraceId(prefix: string) {
  const random = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`
  return `${prefix}-${random}`
}

function delay(ms: number) {
  return new Promise(resolve => window.setTimeout(resolve, ms))
}
