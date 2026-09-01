/**
 * Top-level viewer coordinator.
 *
 * App owns state that must be shared across the main viewer areas (dataset,
 * filters, paging, selected columns, map snapshot and diagnostics) and
 * coordinates the major workflows that connect UI actions to the API.
 *
 * It deliberately does NOT implement Parquet parsing, Arrow IPC decoding, trace
 * polling, or large feature-specific view trees. Those concerns live under
 * `lib/` and `features/`. App remains the one place where state shared by Table,
 * Map, Schema and future Analysis views is coordinated.
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent, ReactNode } from 'react'
import type { Feature } from 'geojson'
import { batchRows, type PlainRow } from './lib/arrow'
import { closeDataset, countRows, openDataset, pageBatches } from './lib/api'
import { DatasetSidebar } from './features/dataset/DatasetSidebar'
import { LandingPanel } from './features/dataset/LandingPanel'
import { OpenFileBar } from './features/dataset/OpenFileBar'
import { ProgressPanel } from './features/diagnostics/ProgressPanel'
import { useActivityTrace } from './features/diagnostics/useActivityTrace'
import { MapPanel, type MapActivityUpdate } from './features/map/MapPanel'
import { detectSpatialSource } from './features/map/spatial'
import { appendSpatialBatchFeatures, spatialColumnNames } from './features/map/spatialFeatures'
import { SchemaPanel } from './features/schema/SchemaPanel'
import { TablePanel } from './features/table/TablePanel'
import type { DatasetInfo, FilterClause, MapSnapshotSpec } from './types'

type Tab = 'data' | 'map' | 'schema'
type Theme = 'light' | 'dark'

export default function App() {
  // Shared workspace state. These values affect more than one child component,
  // so App is the lowest sensible common owner for them.
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
  // Request IDs are monotonic guards against stale async results. A valid old
  // response must not overwrite state produced by a newer user action.
  const pageRequestId = useRef(0)
  const mapSnapshotId = useRef(0)

  // All viewer features report progress through one shared trace lifecycle.
  const {
    activity,
    beginActivity,
    addClientActivity,
    finishActivity,
    failActivity,
  } = useActivityTrace()

  const spatialSource = useMemo(
    () => dataset ? detectSpatialSource(dataset) : undefined,
    [dataset],
  )

  // Synchronize React theme state with the document and persisted preference.
  useEffect(() => {
    document.documentElement.dataset.theme = theme
    window.localStorage.setItem('parquet-viewer-theme', theme)
  }, [theme])

  // Table queries are derived from dataset + paging + filters + projection.
  // Changing any of those inputs invalidates the current page and triggers a read.
  useEffect(() => {
    if (!dataset) return
    void loadPage(dataset, offset, limit, visibleColumns)
  }, [dataset?.dataset_id, offset, limit, filters, visibleColumns])

  // The unfiltered row count comes from footer metadata. Filtered counts require
  // a backend query and are kept separate from page loading.
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

  /** Open a URL and reset all dataset-dependent UI state around the new handle. */
  async function onOpen(event: FormEvent) {
    event.preventDefault()
    if (!uri.trim()) return

    const traceId = makeTraceId('open')
    beginActivity(traceId, 'Opening file', 'Checking source')
    setOpening(true)
    setError('')

    try {
      if (dataset) {
        // Dataset IDs are temporary server-side handles. Close the previous one
        // when changing URLs, but keep this best-effort: the backend idle TTL
        // also cleans it up if the browser disappears or this request fails.
        await closeDataset(dataset.dataset_id).catch(() => undefined)
      }

      // `/datasets/open` generates a fresh process-local handle. Store the full
      // DatasetInfo because every later feature uses `dataset.dataset_id`.
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

  /**
   * Stream one table page as Arrow batches.
   *
   * The backend projection follows visibleColumns, while extra spatial columns
   * may be requested only when they enable the lightweight map preview.
   */
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

  /** Freeze the current filter/count state into a map snapshot specification. */
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

      <OpenFileBar
        uri={uri}
        opening={opening}
        onUriChange={setUri}
        onOpen={onOpen}
      />

      {error && <div className="error-banner">{error}</div>}
      <ProgressPanel activity={activity} />

      {!dataset ? (
        <LandingPanel />
      ) : (
        <main className="workspace">
          <DatasetSidebar
            dataset={dataset}
            spatialSource={spatialSource}
            filters={filters}
          />

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
              <TablePanel
                dataset={dataset}
                rows={rows}
                filters={filters}
                visibleColumns={visibleColumns}
                offset={offset}
                limit={limit}
                totalRows={totalRows}
                loading={loading}
                countLoading={countLoading}
                spatialSource={spatialSource}
                mapSnapshot={mapSnapshot}
                onApplyFilters={next => {
                  setOffset(0)
                  setFilters(next)
                }}
                onOffsetChange={setOffset}
                onVisibleColumnsChange={next => {
                  setOffset(0)
                  setVisibleColumns(next)
                }}
                onLimitChange={next => {
                  setOffset(0)
                  setLimit(next)
                }}
                onViewMap={switchToMap}
              />
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
              <SchemaPanel dataset={dataset} spatialAvailable={Boolean(spatialSource)} />
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
  children: ReactNode
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

function getInitialTheme(): Theme {
  const saved = window.localStorage.getItem('parquet-viewer-theme')
  if (saved === 'light' || saved === 'dark') return saved
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
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

