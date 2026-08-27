import { useEffect, useMemo, useState } from 'react'
import type { RecordBatch } from 'apache-arrow'
import { batchRows, type PlainRow } from './arrow'
import { closeDataset, downloadSubset, openDataset, pageBatches } from './api'
import { DataTable } from './DataTable'
import { MapPanel } from './MapPanel'
import type { DatasetInfo } from './types'

type Tab = 'data' | 'map' | 'schema'

export default function App() {
  const [uri, setUri] = useState('')
  const [dataset, setDataset] = useState<DatasetInfo | null>(null)
  const [tab, setTab] = useState<Tab>('data')
  const [offset, setOffset] = useState(0)
  const [limit, setLimit] = useState(1000)
  const [rows, setRows] = useState<PlainRow[]>([])
  const [columns, setColumns] = useState<string[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const primaryGeo = useMemo(() => dataset?.geo_columns.find(column => column.is_primary) ?? dataset?.geo_columns[0], [dataset])

  useEffect(() => {
    if (!dataset) return
    void loadPage(dataset, offset, limit)
  }, [dataset?.dataset_id, offset, limit])

  async function onOpen(event: React.FormEvent) {
    event.preventDefault()
    if (!uri.trim()) return
    setLoading(true); setError('')
    try {
      if (dataset) await closeDataset(dataset.dataset_id).catch(() => undefined)
      const info = await openDataset(uri.trim())
      setDataset(info); setOffset(0); setTab(info.geo_columns.length ? 'map' : 'data')
    } catch (err) { setError(String(err)) }
    finally { setLoading(false) }
  }

  async function loadPage(info: DatasetInfo, pageOffset: number, pageLimit: number) {
    setLoading(true); setError('')
    try {
      const nextRows: PlainRow[] = []
      let firstBatch: RecordBatch | null = null
      for await (const batch of pageBatches(info.dataset_id, { offset: pageOffset, limit: pageLimit })) {
        firstBatch ??= batch
        nextRows.push(...batchRows(batch, pageLimit - nextRows.length))
        if (nextRows.length >= pageLimit) break
      }
      setRows(nextRows)
      setColumns(firstBatch?.schema.fields.map(field => field.name) ?? info.columns.map(column => column.name))
    } catch (err) { setError(String(err)) }
    finally { setLoading(false) }
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div>
          <h1>Parquet Globe</h1>
          <p>Native Rust GeoParquet 2 explorer · Arrow IPC · globe viewport queries</p>
        </div>
        {dataset && <div className="dataset-pill">{dataset.num_rows.toLocaleString()} rows · {dataset.num_row_groups} row groups</div>}
      </header>

      <form className="open-bar" onSubmit={onOpen}>
        <input value={uri} onChange={event => setUri(event.target.value)} placeholder="https://…/dataset.parquet" aria-label="Parquet URL" />
        <button disabled={loading || !uri.trim()}>{loading ? 'Working…' : 'Open Parquet'}</button>
      </form>

      {error && <div className="error-banner">{error}</div>}

      {!dataset ? (
        <main className="landing">
          <div className="landing-card">
            <span className="eyebrow">GeoParquet 2.0</span>
            <h2>Paste a Parquet URL.</h2>
            <p>The backend range-reads metadata and row groups. Table data arrives as Arrow IPC; geospatial files open directly on a globe.</p>
          </div>
        </main>
      ) : (
        <main className="workspace">
          <aside className="sidebar">
            <div className="side-section">
              <span className="section-label">Dataset</span>
              <div className="uri" title={dataset.uri}>{dataset.name || filename(dataset.uri)}</div>
              <div className="muted mono">{dataset.dataset_id.slice(0, 12)}</div>
            </div>
            <div className="side-section stats-grid">
              <Metric label="Rows" value={dataset.num_rows.toLocaleString()} />
              <Metric label="Columns" value={String(dataset.columns.length)} />
              <Metric label="Row groups" value={String(dataset.num_row_groups)} />
              <Metric label="Geo columns" value={String(dataset.geo_columns.length)} />
            </div>
            {primaryGeo && <div className="side-section">
              <span className="section-label">Spatial</span>
              <dl className="metadata-list">
                <dt>Column</dt><dd>{primaryGeo.name}</dd>
                <dt>Type</dt><dd>{primaryGeo.logical_type}</dd>
                <dt>CRS</dt><dd>{primaryGeo.crs}</dd>
                <dt>Statistics</dt><dd>{primaryGeo.row_groups_with_bbox}/{primaryGeo.row_groups_total}</dd>
              </dl>
            </div>}
            <div className="side-section">
              <span className="section-label">Export</span>
              <div className="button-stack">
                <button className="secondary" onClick={() => void downloadSubset(dataset.dataset_id, 'parquet')}>Download Parquet</button>
                <button className="secondary" onClick={() => void downloadSubset(dataset.dataset_id, 'arrow')}>Download Arrow</button>
              </div>
            </div>
          </aside>

          <section className="content">
            <nav className="tabs">
              <TabButton active={tab === 'data'} onClick={() => setTab('data')}>Data</TabButton>
              <TabButton active={tab === 'map'} disabled={!dataset.geo_columns.length} onClick={() => setTab('map')}>Globe</TabButton>
              <TabButton active={tab === 'schema'} onClick={() => setTab('schema')}>Schema</TabButton>
            </nav>

            {tab === 'data' && <section className="pane">
              <div className="pane-toolbar">
                <div className="pager">
                  <button className="secondary" disabled={offset === 0 || loading} onClick={() => setOffset(Math.max(0, offset - limit))}>Previous</button>
                  <span>{offset.toLocaleString()}–{Math.min(offset + rows.length, dataset.num_rows).toLocaleString()}</span>
                  <button className="secondary" disabled={offset + limit >= dataset.num_rows || loading} onClick={() => setOffset(offset + limit)}>Next</button>
                </div>
                <label>Rows <select value={limit} onChange={event => { setOffset(0); setLimit(Number(event.target.value)) }}><option>250</option><option>1000</option><option>5000</option><option>10000</option></select></label>
              </div>
              <DataTable columns={columns} rows={rows} />
            </section>}

            {tab === 'map' && <section className="pane map-pane"><MapPanel dataset={dataset} /></section>}

            {tab === 'schema' && <section className="pane schema-pane">
              <div className="schema-summary">
                <div><span className="section-label">GeoParquet</span><strong>{dataset.geo_parquet?.version ?? 'native geospatial Parquet / none'}</strong></div>
                <div><span className="section-label">V2 checks</span><strong>{dataset.geo_parquet?.v2_metadata_checks_passed ? 'Passed' : 'Not confirmed'}</strong></div>
                <div><span className="section-label">Exact spatial</span><strong>{dataset.capabilities.spatial_exact ? 'Enabled' : 'Pending GeoRust kernel'}</strong></div>
              </div>
              {dataset.geo_parquet?.warnings.length ? <div className="warning-box"><strong>Metadata warnings</strong>{dataset.geo_parquet.warnings.map(warning => <div key={warning}>{warning}</div>)}</div> : null}
              <div className="schema-table">
                <div className="schema-row schema-head"><span>Name</span><span>Arrow type</span><span>Parquet physical</span><span>Nullable</span></div>
                {dataset.columns.map(column => <div className="schema-row" key={column.name}><span>{column.name}</span><span className="mono">{column.arrow_type}</span><span>{column.parquet_physical_type ?? '—'}</span><span>{column.nullable ? 'yes' : 'no'}</span></div>)}
              </div>
            </section>}
          </section>
        </main>
      )}
    </div>
  )
}

function TabButton({ active, disabled, onClick, children }: { active: boolean; disabled?: boolean; onClick: () => void; children: React.ReactNode }) {
  return <button className={active ? 'active' : ''} disabled={disabled} onClick={onClick}>{children}</button>
}
function Metric({ label, value }: { label: string; value: string }) { return <div><span className="metric-value">{value}</span><span className="metric-label">{label}</span></div> }
function filename(uri: string) { try { return new URL(uri).pathname.split('/').filter(Boolean).pop() ?? uri } catch { return uri } }
