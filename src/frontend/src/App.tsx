/**
 * Top-level viewer coordinator.
 *
 * App owns state shared across Table, Map, Schema and diagnostics. Unfiltered
 * browsing remains backend-paged. Once filters are active (or the user
 * explicitly chooses All), one complete Arrow result is streamed into the
 * browser and both Table and Map read from that same cached result.
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent, ReactNode } from 'react'
import type { Feature } from 'geojson'
import type { RecordBatch } from 'apache-arrow'
import { batchRows, batchRowsRange, type PlainRow } from './lib/arrow'
import { closeDataset, openDataset, pageBatches, resultBatches } from './lib/api'
import { DatasetSidebar } from './features/dataset/DatasetSidebar'
import { LandingPanel } from './features/dataset/LandingPanel'
import { OpenFileBar } from './features/dataset/OpenFileBar'
import { ProgressPanel } from './features/diagnostics/ProgressPanel'
import { useActivityTrace } from './features/diagnostics/useActivityTrace'
import { MapPanel } from './features/map/MapPanel'
import { detectSpatialSource } from './features/map/spatial'
import {
  appendSpatialBatchFeatures,
  selectMapPropertyColumns,
  spatialColumnNames,
} from './features/map/spatialFeatures'
import { SchemaPanel } from './features/schema/SchemaPanel'
import { TablePanel } from './features/table/TablePanel'
import type { DatasetInfo, FilterClause } from './types'

type Tab = 'data' | 'map' | 'schema'
type Theme = 'light' | 'dark'

export default function App() {
  const [uri, setUri] = useState('')
  const [dataset, setDataset] = useState<DatasetInfo | null>(null)
  const [tab, setTab] = useState<Tab>('data')
  const [offset, setOffset] = useState(0)
  const [limit, setLimit] = useState(1000)
  const [pageRows, setPageRows] = useState<PlainRow[]>([])
  const [visibleColumns, setVisibleColumns] = useState<string[]>([])
  const [filters, setFilters] = useState<FilterClause[]>([])
  const [loadAll, setLoadAll] = useState(false)
  const [loading, setLoading] = useState(false)
  const [opening, setOpening] = useState(false)
  const [error, setError] = useState('')
  const [theme, setTheme] = useState<Theme>(getInitialTheme)

  // The canonical complete-result cache stays in Arrow form. Table pages are
  // materialized only when displayed, and Map decodes these same batches only
  // when the map is actually opened. This avoids duplicating the whole result
  // into JavaScript row objects up front.
  const fullBatchesRef = useRef<RecordBatch[]>([])
  const fullMapFeaturesRef = useRef<Feature[]>([])
  const fullMapDecodedBatchCountRef = useRef(0)
  const fullMapDecodedRowsRef = useRef(0)
  const fullMapLastPublishedRowsRef = useRef(0)
  const fullMapLastDiagnosticAtRef = useRef(0)
  const fullResultTraceIdRef = useRef<string | null>(null)
  const [fullRowsLoaded, setFullRowsLoaded] = useState(0)
  const [fullResultComplete, setFullResultComplete] = useState(false)
  const [fullMapRevision, setFullMapRevision] = useState(0)

  const [mapPageFeatures, setMapPageFeatures] = useState<Feature[]>([])
  const [mapPageLoading, setMapPageLoading] = useState(false)
  const [mapPageKey, setMapPageKey] = useState<string | null>(null)
  const [mapPageRevision, setMapPageRevision] = useState(0)

  // Request IDs are monotonic guards against stale async results. Abort signals
  // stop obsolete full-result/page streams as soon as query state changes.
  const pageRequestId = useRef(0)
  const resultRequestId = useRef(0)
  const mapPageRequestId = useRef(0)

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

  const fullResultActive = filters.length > 0 || loadAll
  const fullPageRowsAvailable = Math.max(0, Math.min(limit, fullRowsLoaded - offset))
  const displayedRows = useMemo(
    () => fullResultActive
      ? rowsFromCachedBatches(fullBatchesRef.current, offset, limit, visibleColumns)
      : pageRows,
    [fullResultActive, fullPageRowsAvailable, offset, limit, visibleColumns, pageRows],
  )
  const totalRows = fullResultActive
    ? (fullResultComplete ? fullRowsLoaded : null)
    : dataset?.num_rows ?? null

  const currentPageKey = dataset
    ? `${dataset.dataset_id}:${offset}:${limit}:${visibleColumns.join('|')}`
    : ''

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    window.localStorage.setItem('parquet-explorer-theme', theme)
  }, [theme])

  // Drop a complete-result cache as soon as the viewer returns to ordinary
  // unfiltered paging. Arrow buffers can be large, so stale filtered/All data
  // should not remain retained behind the scenes.
  useEffect(() => {
    if (fullResultActive) return
    fullBatchesRef.current = []
    fullMapFeaturesRef.current = []
    fullMapDecodedBatchCountRef.current = 0
    fullMapDecodedRowsRef.current = 0
    fullMapLastPublishedRowsRef.current = 0
    fullMapLastDiagnosticAtRef.current = 0
    fullResultTraceIdRef.current = null
    setFullRowsLoaded(0)
    setFullResultComplete(false)
    setFullMapRevision(value => value + 1)
  }, [dataset?.dataset_id, fullResultActive])

  // Ordinary unfiltered exploration stays server-paged. Changing page or page
  // size therefore issues one bounded /page request.
  useEffect(() => {
    if (!dataset || fullResultActive) return
    const controller = new AbortController()
    void loadPage(dataset, offset, limit, visibleColumns, controller.signal)
    return () => controller.abort()
  }, [dataset?.dataset_id, fullResultActive, offset, limit, visibleColumns])

  // A filtered result (or explicit All) is streamed once. Local table paging
  // does not appear in this dependency list, so Previous/Next never re-query
  // Parquet while this cache is active.
  useEffect(() => {
    if (!dataset || !fullResultActive) return
    const controller = new AbortController()
    void loadFullResult(dataset, visibleColumns, controller.signal)
    return () => controller.abort()
  }, [dataset?.dataset_id, fullResultActive, filters, visibleColumns])

  // In ordinary unfiltered paging, coordinate columns are usually already part
  // of the page query. A large WKB geometry column is deliberately fetched only
  // when the user actually opens Map, avoiding hidden geometry I/O in Table.
  useEffect(() => {
    if (
      !dataset
      || !spatialSource
      || fullResultActive
      || tab !== 'map'
      || mapPageKey === currentPageKey
    ) return

    const controller = new AbortController()
    void loadCurrentPageMap(dataset, controller.signal)
    return () => controller.abort()
  }, [dataset?.dataset_id, spatialSource, fullResultActive, tab, currentPageKey, mapPageKey])

  // Full filtered/All results keep Arrow batches as the shared cache. Decode
  // spatial features only while Map is open, and only for batches not already
  // processed. This preserves one backend query without forcing WKB -> GeoJSON
  // work on users who stay in Table.
  useEffect(() => {
    if (!dataset || !spatialSource || !fullResultActive || tab !== 'map') return

    const batches = fullBatchesRef.current
    const firstBatch = fullMapDecodedBatchCountRef.current
    if (firstBatch >= batches.length) {
      // Completion can arrive after the last batch was decoded but before the
      // 50k publish threshold was reached. Always publish that final tail.
      if (
        fullResultComplete
        && fullMapLastPublishedRowsRef.current !== fullMapDecodedRowsRef.current
      ) {
        fullMapLastPublishedRowsRef.current = fullMapDecodedRowsRef.current
        setFullMapRevision(value => value + 1)
      }
      return
    }

    const spatialSet = new Set(spatialColumnNames(spatialSource))
    const propertyColumns = visibleColumns.filter(name => !spatialSet.has(name)).slice(0, 8)
    const started = performance.now()
    for (let index = firstBatch; index < batches.length; index += 1) {
      const batch = batches[index]
      appendSpatialBatchFeatures(
        batch,
        spatialSource,
        propertyColumns,
        fullMapFeaturesRef.current,
        fullMapDecodedRowsRef.current,
      )
      fullMapDecodedRowsRef.current += batch.numRows
      fullMapDecodedBatchCountRef.current = index + 1
    }

    const decodeMs = performance.now() - started
    const decodedRows = fullMapDecodedRowsRef.current
    const shouldPublish = fullMapLastPublishedRowsRef.current === 0
      || decodedRows - fullMapLastPublishedRowsRef.current >= 50_000
      || (fullResultComplete && fullMapDecodedBatchCountRef.current === batches.length)

    if (shouldPublish) {
      fullMapLastPublishedRowsRef.current = decodedRows
      setFullMapRevision(value => value + 1)
    }

    const traceId = fullResultTraceIdRef.current
    const now = performance.now()
    if (
      traceId
      && (fullMapLastDiagnosticAtRef.current === 0
        || now - fullMapLastDiagnosticAtRef.current >= 1_000
        || fullResultComplete)
    ) {
      addClientActivity(
        traceId,
        spatialSource.kind === 'geometry' ? 'Decoding WKB geometries' : 'Building coordinate geometries',
        `${fullMapFeaturesRef.current.length.toLocaleString()} features from ${decodedRows.toLocaleString()} cached rows`,
        undefined,
        decodeMs,
      )
      fullMapLastDiagnosticAtRef.current = now
    }
  }, [dataset?.dataset_id, spatialSource, fullResultActive, tab, fullRowsLoaded, fullResultComplete, visibleColumns])

  /** Open a URL and reset all dataset-dependent browser state. */
  async function onOpen(event: FormEvent) {
    event.preventDefault()
    if (!uri.trim()) return

    const traceId = makeTraceId('open')
    beginActivity(traceId, 'Opening file', 'Checking source')
    setOpening(true)
    setError('')

    try {
      if (dataset) {
        // Dataset IDs are temporary backend handles. Explicit close is a fast
        // cleanup path; backend idle expiry handles browser crashes/refreshes.
        await closeDataset(dataset.dataset_id).catch(() => undefined)
      }

      const info = await openDataset(uri.trim(), undefined, traceId)
      addClientActivity(traceId, 'Preparing the viewer', `${info.columns.length} fields found`)

      fullBatchesRef.current = []
      fullMapFeaturesRef.current = []
      fullMapDecodedBatchCountRef.current = 0
      fullMapDecodedRowsRef.current = 0
      fullMapLastPublishedRowsRef.current = 0
      fullMapLastDiagnosticAtRef.current = 0
      fullResultTraceIdRef.current = null
      setDataset(info)
      setOffset(0)
      setFilters([])
      setLoadAll(false)
      setPageRows([])
      setVisibleColumns(info.columns.map(column => column.name))
      setFullRowsLoaded(0)
      setFullResultComplete(false)
      setFullMapRevision(0)
      setMapPageFeatures([])
      setMapPageKey(null)
      setMapPageRevision(0)
      setTab('data')
      finishActivity(traceId, 'File opened', `${info.num_rows.toLocaleString()} rows ready to explore`)
    } catch (err) {
      failActivity(traceId, 'Could not open file')
      setError(String(err))
    } finally {
      setOpening(false)
    }
  }

  /** Stream one bounded unfiltered table page from the backend. */
  async function loadPage(
    info: DatasetInfo,
    pageOffset: number,
    pageLimit: number,
    pageColumns: string[],
    signal: AbortSignal,
  ) {
    const requestId = ++pageRequestId.current
    const traceId = makeTraceId('page')
    const pageSpatialSource = detectSpatialSource(info)

    // Coordinates are cheap enough to keep with the current page so Map can
    // open instantly. WKB is included here only when already visible in Table.
    const requiredSpatialColumns = pageSpatialSource?.kind === 'coordinates'
      ? spatialColumnNames(pageSpatialSource)
      : pageSpatialSource && pageColumns.includes(pageSpatialSource.geometry.name)
        ? [pageSpatialSource.geometry.name]
        : []
    const requestColumns = [...new Set([...pageColumns, ...requiredSpatialColumns])]

    setMapPageFeatures([])
    setMapPageKey(null)

    if (requestColumns.length === 0) {
      setPageRows([])
      setLoading(false)
      return
    }

    beginActivity(
      traceId,
      'Loading rows',
      `Fetching rows ${(pageOffset + 1).toLocaleString()}–${(pageOffset + pageLimit).toLocaleString()}`,
    )
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
            filters: [],
          },
          diagnostic => addClientActivity(
            traceId,
            diagnostic.message,
            diagnostic.detail,
            undefined,
            diagnostic.durationMs,
          ),
          signal,
        )
      ) {
        const tableDecodeStarted = performance.now()
        nextRows.push(...batchRows(batch, pageLimit - nextRows.length, pageColumns))
        addClientActivity(
          traceId,
          'Building table rows',
          `${Math.min(pageLimit, rowsRead + batch.numRows).toLocaleString()} of ${pageLimit.toLocaleString()} requested`,
          Math.min(0.94, (rowsRead + batch.numRows) / Math.max(1, pageLimit)),
          performance.now() - tableDecodeStarted,
        )

        if (pageSpatialSource && requiredSpatialColumns.length > 0) {
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

      if (signal.aborted || requestId !== pageRequestId.current) return

      setPageRows(nextRows)
      if (pageSpatialSource && requiredSpatialColumns.length > 0) {
        setMapPageFeatures(nextMapFeatures)
        setMapPageKey(currentPageKeyFor(info.dataset_id, pageOffset, pageLimit, pageColumns))
        setMapPageRevision(value => value + 1)
      }
      finishActivity(traceId, 'Rows ready', `${nextRows.length.toLocaleString()} rows loaded into the table`)
    } catch (err) {
      if (signal.aborted) return
      if (requestId === pageRequestId.current) {
        setMapPageFeatures([])
        setMapPageKey(null)
        failActivity(traceId, 'Loading rows failed')
        setError(String(err))
      }
    } finally {
      if (!signal.aborted && requestId === pageRequestId.current) setLoading(false)
    }
  }

  /**
   * Stream the complete current result once and keep it in the browser.
   *
   * Filters automatically use this path. The explicit All option uses the same
   * path with no filters. Table Previous/Next only slices this cache afterward.
   */
  async function loadFullResult(
    info: DatasetInfo,
    tableColumns: string[],
    signal: AbortSignal,
  ) {
    const requestId = ++resultRequestId.current
    const traceId = makeTraceId(filters.length ? 'filter' : 'all')
    const resultSpatialSource = detectSpatialSource(info)
    const spatialColumns = resultSpatialSource ? spatialColumnNames(resultSpatialSource) : []
    const requested = [...new Set([...tableColumns, ...spatialColumns])]
    const requestColumns = requested.length > 0
      ? requested
      : info.columns.slice(0, 1).map(column => column.name)

    fullBatchesRef.current = []
    fullMapFeaturesRef.current = []
    fullMapDecodedBatchCountRef.current = 0
    fullMapDecodedRowsRef.current = 0
    fullMapLastPublishedRowsRef.current = 0
    fullMapLastDiagnosticAtRef.current = 0
    fullResultTraceIdRef.current = traceId
    setPageRows([])
    setMapPageFeatures([])
    setMapPageKey(null)
    setFullRowsLoaded(0)
    setFullResultComplete(false)
    setFullMapRevision(value => value + 1)
    setLoading(true)
    setError('')

    beginActivity(
      traceId,
      filters.length ? 'Loading filtered result' : 'Loading all rows',
      filters.length ? 'Streaming all matching rows once' : 'Streaming the complete dataset',
    )

    try {
      let rowsRead = 0
      let lastProgressAt = 0

      for await (
        const batch of resultBatches(
          info.dataset_id,
          {
            trace_id: traceId,
            columns: requestColumns,
            filters,
          },
          diagnostic => addClientActivity(
            traceId,
            diagnostic.message,
            diagnostic.detail,
            undefined,
            diagnostic.durationMs,
          ),
          signal,
        )
      ) {
        fullBatchesRef.current.push(batch)
        rowsRead += batch.numRows
        if (signal.aborted || requestId !== resultRequestId.current) return

        // A row counter is enough to refresh local table paging. The Arrow
        // batches themselves stay canonical until a visible page is rendered.
        setFullRowsLoaded(rowsRead)

        const now = performance.now()
        if (lastProgressAt === 0 || now - lastProgressAt >= 1_000) {
          addClientActivity(
            traceId,
            'Caching Arrow result',
            `${rowsRead.toLocaleString()} rows · ${fullBatchesRef.current.length.toLocaleString()} batches`,
          )
          lastProgressAt = now
        }
      }

      if (signal.aborted || requestId !== resultRequestId.current) return

      setFullRowsLoaded(rowsRead)
      setFullResultComplete(true)
      finishActivity(
        traceId,
        filters.length ? 'Filtered result ready' : 'All rows loaded',
        `${rowsRead.toLocaleString()} rows cached as Arrow batches`,
      )
    } catch (err) {
      if (signal.aborted) return
      if (requestId === resultRequestId.current) {
        failActivity(traceId, filters.length ? 'Filtered result failed' : 'Loading all rows failed')
        setError(String(err))
      }
    } finally {
      if (!signal.aborted && requestId === resultRequestId.current) setLoading(false)
    }
  }

  /** Fetch only the current unfiltered page's spatial data when Map needs it. */
  async function loadCurrentPageMap(info: DatasetInfo, signal: AbortSignal) {
    if (!spatialSource) return
    const requestId = ++mapPageRequestId.current
    const traceId = makeTraceId('map-page')
    const propertyColumns = selectMapPropertyColumns(info, spatialSource, visibleColumns)
    const columns = [...new Set([...spatialColumnNames(spatialSource), ...propertyColumns])]
    const features: Feature[] = []
    let rowsRead = 0

    setMapPageLoading(true)
    beginActivity(traceId, 'Loading map page', `Rows ${(offset + 1).toLocaleString()}–${(offset + limit).toLocaleString()}`)

    try {
      for await (
        const batch of pageBatches(
          info.dataset_id,
          { trace_id: traceId, columns, offset, limit, filters: [] },
          diagnostic => addClientActivity(
            traceId,
            diagnostic.message,
            diagnostic.detail,
            undefined,
            diagnostic.durationMs,
          ),
          signal,
        )
      ) {
        const geometryStarted = performance.now()
        appendSpatialBatchFeatures(batch, spatialSource, propertyColumns, features, offset + rowsRead)
        rowsRead += batch.numRows
        addClientActivity(
          traceId,
          spatialSource.kind === 'geometry' ? 'Decoding WKB geometries' : 'Building coordinate geometries',
          `${features.length.toLocaleString()} features from ${rowsRead.toLocaleString()} rows`,
          Math.min(0.98, rowsRead / Math.max(1, limit)),
          performance.now() - geometryStarted,
        )
      }

      if (signal.aborted || requestId !== mapPageRequestId.current) return
      setMapPageFeatures(features)
      setMapPageKey(currentPageKey)
      setMapPageRevision(value => value + 1)
      finishActivity(traceId, 'Map page ready', `${features.length.toLocaleString()} geometries loaded`)
    } catch (err) {
      if (signal.aborted) return
      if (requestId === mapPageRequestId.current) {
        failActivity(traceId, 'Map page failed')
        setError(String(err))
      }
    } finally {
      if (!signal.aborted && requestId === mapPageRequestId.current) setMapPageLoading(false)
    }
  }

  const mapFeatures = fullResultActive ? fullMapFeaturesRef.current : mapPageFeatures
  const mapRevision = fullResultActive ? fullMapRevision : mapPageRevision
  const fullMapComplete = fullResultComplete
    && fullMapDecodedBatchCountRef.current === fullBatchesRef.current.length
  const mapRowsLoaded = fullResultActive ? fullMapDecodedRowsRef.current : pageRows.length
  const mapLoading = fullResultActive ? (loading || !fullMapComplete) : mapPageLoading
  const mapComplete = fullResultActive ? fullMapComplete : mapPageKey === currentPageKey
  const mapScopeLabel = filters.length > 0
    ? 'Filtered result'
    : loadAll
      ? 'All rows'
      : `Current page · ${(offset + 1).toLocaleString()}–${Math.min(offset + limit, dataset?.num_rows ?? offset + limit).toLocaleString()}`
  const mapDataKey = dataset
    ? `${dataset.dataset_id}:${fullResultActive ? `full:${JSON.stringify(filters)}:${loadAll}` : currentPageKey}`
    : ''

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-block">
          <div className="brand-mark" aria-hidden="true">P</div>
          <div>
            <h1>Parquet Explorer</h1>
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
          <DatasetSidebar dataset={dataset} spatialSource={spatialSource} filters={filters} />

          <section className="content">
            <nav className="tabs">
              <TabButton active={tab === 'data'} onClick={() => setTab('data')}>Table</TabButton>
              <TabButton
                active={tab === 'map'}
                disabled={!spatialSource}
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
                rows={displayedRows}
                filters={filters}
                visibleColumns={visibleColumns}
                offset={offset}
                limit={limit}
                totalRows={totalRows}
                loadedRows={fullResultActive ? fullRowsLoaded : pageRows.length}
                loading={loading}
                fullResult={fullResultActive}
                fullResultComplete={fullResultComplete}
                loadAll={loadAll}
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
                onLoadAllChange={next => {
                  setOffset(0)
                  setLoadAll(next)
                }}
              />
            )}

            {spatialSource && (
              <section
                className={`pane map-pane${tab === 'map' ? '' : ' pane-hidden'}`}
                aria-hidden={tab !== 'map'}
              >
                <MapPanel
                  dataset={dataset}
                  source={spatialSource}
                  features={mapFeatures}
                  rowsLoaded={mapRowsLoaded}
                  scopeLabel={mapScopeLabel}
                  dataKey={mapDataKey}
                  dataRevision={mapRevision}
                  loading={mapLoading}
                  complete={mapComplete}
                  theme={theme}
                  active={tab === 'map'}
                />
              </section>
            )}

            {tab === 'schema' && (
              <SchemaPanel dataset={dataset} spatialAvailable={Boolean(spatialSource)} />
            )}
          </section>
        </main>
      )}

      <footer className="project-footer">
        <div className="project-footer-logos" aria-label="Project attribution">
          <a href="https://www.edito.eu/" target="_blank" rel="noreferrer">
            <img
              className="edito-logo"
              src="https://www.edito.eu/wp-content/uploads/2022/08/EDITO_Short_Logo_1.6.svg"
              alt="Powered by EDITO"
            />
          </a>
          <a
            href="https://www.vliz.be/"
            target="_blank"
            rel="noreferrer"
            aria-label="Flanders Marine Institute (VLIZ)"
          >
            <img src="/vliz-logo.png" className="footer-logo-vliz" alt="VLIZ" />
          </a>
        </div>
      </footer>
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
    <button className={active ? 'active' : ''} disabled={disabled} onClick={onClick}>
      {children}
    </button>
  )
}

function getInitialTheme(): Theme {
  const saved = window.localStorage.getItem('parquet-explorer-theme')
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

function rowsFromCachedBatches(
  batches: RecordBatch[],
  offset: number,
  limit: number,
  columns: string[],
): PlainRow[] {
  const rows: PlainRow[] = []
  let skip = offset
  let remaining = limit

  for (const batch of batches) {
    if (remaining <= 0) break
    if (skip >= batch.numRows) {
      skip -= batch.numRows
      continue
    }

    const next = batchRowsRange(batch, skip, remaining, columns)
    rows.push(...next)
    remaining -= next.length
    skip = 0
  }

  return rows
}

function currentPageKeyFor(datasetId: string, offset: number, limit: number, columns: string[]) {
  return `${datasetId}:${offset}:${limit}:${columns.join('|')}`
}
