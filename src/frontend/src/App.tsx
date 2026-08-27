import { useEffect, useMemo, useState } from 'react'
import type { RecordBatch } from 'apache-arrow'
import { batchRows, type PlainRow } from './arrow'
import { closeDataset, downloadSubset, openDataset, pageBatches } from './api'
import { DataTable } from './DataTable'
import { FilterBar } from './FilterBar'
import { MapPanel } from './MapPanel'
import { detectSpatialSource } from './spatial'
import type { DatasetInfo, FilterClause } from './types'

type Tab = 'data' | 'map' | 'schema'

export default function App() {
  const [uri, setUri] = useState('')
  const [dataset, setDataset] = useState<DatasetInfo | null>(null)
  const [tab, setTab] = useState<Tab>('data')
  const [offset, setOffset] = useState(0)
  const [limit, setLimit] = useState(1000)
  const [rows, setRows] = useState<PlainRow[]>([])
  const [columns, setColumns] = useState<string[]>([])
  const [filters, setFilters] = useState<FilterClause[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const spatialSource = useMemo(
    () => dataset ? detectSpatialSource(dataset) : undefined,
    [dataset],
  )

  useEffect(() => {
    if (!dataset) return
    void loadPage(dataset, offset, limit)
  }, [dataset?.dataset_id, offset, limit, filters])

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
      setTab(detectSpatialSource(info) ? 'map' : 'data')
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
  ) {
    setLoading(true)
    setError('')

    try {
      const nextRows: PlainRow[] = []
      let firstBatch: RecordBatch | null = null

      for await (
        const batch of pageBatches(info.dataset_id, {
          offset: pageOffset,
          limit: pageLimit,
          filters,
        })
      ) {
        firstBatch ??= batch
        nextRows.push(...batchRows(batch, pageLimit - nextRows.length))
        if (nextRows.length >= pageLimit) break
      }

      setRows(nextRows)
      setColumns(
        firstBatch?.schema.fields.map(field => field.name)
        ?? info.columns.map(column => column.name),
      )
    } catch (err) {
      setError(String(err))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div>
          <h1>Parquet Globe</h1>
          <p>Native Rust Parquet explorer · Arrow IPC · table + spatial viewport queries</p>
        </div>
        {dataset && (
          <div className="dataset-pill">
            {dataset.num_rows.toLocaleString()} rows · {dataset.num_row_groups} row groups
          </div>
        )}
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
              coordinate pairs and geometry columns can be explored on the globe.
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
                disabled={!spatialSource}
                onClick={() => setTab('map')}
              >
                Globe
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
                      disabled={offset === 0 || loading}
                      onClick={() => setOffset(Math.max(0, offset - limit))}
                    >
                      Previous
                    </button>
                    <span>
                      {rows.length
                        ? `${(offset + 1).toLocaleString()}–${(offset + rows.length).toLocaleString()}`
                        : '0 rows'}
                      {filters.length ? ' matching' : ''}
                    </span>
                    <button
                      className="secondary"
                      disabled={loading || (
                        filters.length
                          ? rows.length < limit
                          : offset + rows.length >= dataset.num_rows
                      )}
                      onClick={() => setOffset(offset + limit)}
                    >
                      Next
                    </button>
                  </div>

                  <label>
                    Rows{' '}
                    <select
                      value={limit}
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

                <DataTable columns={columns} rows={rows} />
              </section>
            )}

            {tab === 'map' && spatialSource && (
              <section className="pane map-pane">
                <MapPanel
                  dataset={dataset}
                  source={spatialSource}
                  filters={filters}
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

function filename(uri: string) {
  try {
    return new URL(uri).pathname.split('/').filter(Boolean).pop() ?? uri
  } catch {
    return uri
  }
}
