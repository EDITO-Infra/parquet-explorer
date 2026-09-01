/**
 * Complete Table-tab presentation and interaction surface.
 *
 * This component owns table-specific controls (filters, paging, projection and
 * rows-per-page selection). It does not fetch Parquet data itself: App owns the
 * shared query state and passes callbacks/state down. That keeps the table UI
 * reusable while preserving one clear coordinator for cross-feature effects
 * such as creating a map snapshot from the current filters.
 */
import type { PlainRow } from '../../lib/arrow'
import type { DatasetInfo, FilterClause, MapSnapshotSpec } from '../../types'
import type { SpatialSource } from '../map/spatial'
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
  loading: boolean
  countLoading: boolean
  spatialSource?: SpatialSource
  mapSnapshot: MapSnapshotSpec | null
  onApplyFilters: (filters: FilterClause[]) => void
  onOffsetChange: (offset: number) => void
  onVisibleColumnsChange: (columns: string[]) => void
  onLimitChange: (limit: number) => void
  onViewMap: () => void
}

export function TablePanel({
  dataset,
  rows,
  filters,
  visibleColumns,
  offset,
  limit,
  totalRows,
  loading,
  countLoading,
  spatialSource,
  mapSnapshot,
  onApplyFilters,
  onOffsetChange,
  onVisibleColumnsChange,
  onLimitChange,
  onViewMap,
}: TablePanelProps) {
  return (
    <section className="pane">
      <FilterBar
        columns={dataset.columns}
        filters={filters}
        disabled={loading}
        onApply={onApplyFilters}
      />

      <div className="pane-toolbar">
        <div className="pager">
          <button
            className="secondary"
            disabled={offset === 0 || loading || visibleColumns.length === 0}
            onClick={() => onOffsetChange(Math.max(0, offset - limit))}
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
            onClick={() => onOffsetChange(offset + limit)}
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
              onClick={onViewMap}
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
            onChange={onVisibleColumnsChange}
          />
          <label>
            Rows per page{' '}
            <select
              value={limit}
              disabled={loading}
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

function sameFilters(left: FilterClause[], right: FilterClause[]): boolean {
  return JSON.stringify(left) === JSON.stringify(right)
}
