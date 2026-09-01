/**
 * Table-tab presentation and interaction surface.
 *
 * Page size is always a display limit (maximum 10,000 rows). Unfiltered data
 * can stay backend-paged or explicitly switch to All. Filtered results are
 * always streamed once and paged locally from the browser cache.
 */
import type { PlainRow } from '../../lib/arrow'
import type { DatasetInfo, FilterClause } from '../../types'
import { ColumnPicker } from './ColumnPicker'
import { DataTable } from './DataTable'
import { FilterBar } from './FilterBar'

interface TablePanelProps {
  dataset: DatasetInfo
  rows: PlainRow[]
  filters: FilterClause[]
  visibleColumns: string[]
  offset: number
  limit: number
  totalRows: number | null
  loadedRows: number
  loading: boolean
  fullResult: boolean
  fullResultComplete: boolean
  loadAll: boolean
  onApplyFilters: (filters: FilterClause[]) => void
  onOffsetChange: (offset: number) => void
  onVisibleColumnsChange: (columns: string[]) => void
  onLimitChange: (limit: number) => void
  onLoadAllChange: (loadAll: boolean) => void
}

export function TablePanel({
  dataset,
  rows,
  filters,
  visibleColumns,
  offset,
  limit,
  totalRows,
  loadedRows,
  loading,
  fullResult,
  fullResultComplete,
  loadAll,
  onApplyFilters,
  onOffsetChange,
  onVisibleColumnsChange,
  onLimitChange,
  onLoadAllChange,
}: TablePanelProps) {
  const localAvailableRows = fullResult ? loadedRows : totalRows ?? 0
  const canGoNext = fullResult
    ? offset + limit < localAvailableRows
    : totalRows != null && offset + rows.length < totalRows

  return (
    <section className="pane">
      <FilterBar
        columns={dataset.columns}
        filters={filters}
        disabled={false}
        onApply={onApplyFilters}
      />

      <div className="pane-toolbar">
        <div className="pager">
          <button
            className="secondary"
            disabled={offset === 0 || visibleColumns.length === 0}
            onClick={() => onOffsetChange(Math.max(0, offset - limit))}
          >
            Previous
          </button>
          <span>{pagerLabel({
            rows,
            filters,
            visibleColumns,
            offset,
            totalRows,
            loadedRows,
            loading,
            fullResult,
            fullResultComplete,
          })}</span>
          <button
            className="secondary"
            disabled={visibleColumns.length === 0 || !canGoNext}
            onClick={() => onOffsetChange(offset + limit)}
          >
            Next
          </button>
        </div>

        <div className="table-controls">
          {filters.length > 0 ? (
            <span className="result-scope-note" title="The matching result is streamed once; table pages are local.">
              All matching rows
            </span>
          ) : (
            <label title="Paged reads only the current table page. All deliberately streams the complete dataset into the browser.">
              Result{' '}
              <select
                value={loadAll ? 'all' : 'paged'}
                onChange={event => onLoadAllChange(event.target.value === 'all')}
              >
                <option value="paged">Paged</option>
                <option value="all">All</option>
              </select>
            </label>
          )}

          <ColumnPicker
            columns={dataset.columns}
            visibleColumns={visibleColumns}
            disabled={false}
            onChange={onVisibleColumnsChange}
          />

          <label>
            Rows per page{' '}
            <select
              value={limit}
              onChange={event => onLimitChange(Number(event.target.value))}
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
  )
}

function pagerLabel({
  rows,
  filters,
  visibleColumns,
  offset,
  totalRows,
  loadedRows,
  loading,
  fullResult,
  fullResultComplete,
}: {
  rows: PlainRow[]
  filters: FilterClause[]
  visibleColumns: string[]
  offset: number
  totalRows: number | null
  loadedRows: number
  loading: boolean
  fullResult: boolean
  fullResultComplete: boolean
}): string {
  const qualifier = filters.length > 0 ? ' matching' : ''

  if (visibleColumns.length === 0) {
    if (fullResult && !fullResultComplete) {
      return `${loadedRows.toLocaleString()}${qualifier} rows loaded${loading ? '…' : ''}`
    }
    return `${(totalRows ?? loadedRows).toLocaleString()}${qualifier} rows`
  }

  const range = rows.length
    ? `${(offset + 1).toLocaleString()}–${(offset + rows.length).toLocaleString()}`
    : '0 rows'

  if (fullResult && !fullResultComplete) {
    return `${range} · ${loadedRows.toLocaleString()}${qualifier} rows loaded${loading ? '…' : ''}`
  }

  return `${range} of ${(totalRows ?? loadedRows).toLocaleString()}${qualifier}`
}
