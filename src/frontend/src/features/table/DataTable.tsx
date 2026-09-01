/**
 * Presentation-only table for already-decoded rows.
 *
 * Network access and Arrow decoding happen elsewhere. Keeping this component
 * focused on presentation makes table behavior independent of the Parquet and
 * Arrow transport details.
 */
import { useMemo, useState } from 'react'
import type { PlainRow } from '../../lib/arrow'

const ROW_HEIGHT = 34
const VIEWPORT_HEIGHT = 520
const OVERSCAN = 12

const MIN_COLUMN_WIDTH = 120
const MAX_COLUMN_WIDTH = 360
const APPROX_CHAR_WIDTH = 7.2
const CELL_HORIZONTAL_PADDING = 22
const WIDTH_SAMPLE_ROWS = 250

export function DataTable({ columns, rows }: { columns: string[]; rows: PlainRow[] }) {
  const [scrollTop, setScrollTop] = useState(0)

  const window = useMemo(() => {
    const visible = Math.ceil(VIEWPORT_HEIGHT / ROW_HEIGHT)
    const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN)
    const end = Math.min(rows.length, start + visible + OVERSCAN * 2)
    return { start, end }
  }, [scrollTop, rows.length])

  // Calculate column widths once for the whole table, then reuse the exact same
  // grid template for the header and every row. This keeps flexible widths while
  // guaranteeing that header cells and body cells remain aligned.
  const columnWidths = useMemo(() => {
    const sample = rows.slice(0, WIDTH_SAMPLE_ROWS)

    return columns.map(column => {
      let longest = column.length

      for (const row of sample) {
        longest = Math.max(longest, stringify(row[column]).length)
      }

      return Math.max(
        MIN_COLUMN_WIDTH,
        Math.min(MAX_COLUMN_WIDTH, Math.ceil(longest * APPROX_CHAR_WIDTH + CELL_HORIZONTAL_PADDING)),
      )
    })
  }, [columns, rows])

  const gridTemplateColumns = useMemo(
    () => columnWidths.map(width => `${width}px`).join(' '),
    [columnWidths],
  )

  const tableWidth = useMemo(
    () => columnWidths.reduce((total, width) => total + width, 0),
    [columnWidths],
  )

  if (!columns.length) return <div className="empty-state">No rows loaded.</div>

  return (
    <div className="virtual-table" onScroll={event => setScrollTop(event.currentTarget.scrollTop)}>
      <div
        className="table-grid table-head"
        style={{ gridTemplateColumns, width: tableWidth }}
      >
        {columns.map(column => (
          <div key={column} className="cell head-cell" title={column}>
            {column}
          </div>
        ))}
      </div>

      <div
        style={{
          height: rows.length * ROW_HEIGHT,
          position: 'relative',
          width: tableWidth,
        }}
      >
        {rows.slice(window.start, window.end).map((row, localIndex) => {
          const index = window.start + localIndex

          return (
            <div
              key={index}
              className="table-grid table-row"
              style={{
                gridTemplateColumns,
                position: 'absolute',
                top: index * ROW_HEIGHT,
                left: 0,
                width: tableWidth,
                height: ROW_HEIGHT,
              }}
            >
              {columns.map(column => (
                <div key={column} className="cell" title={stringify(row[column])}>
                  {stringify(row[column])}
                </div>
              ))}
            </div>
          )
        })}
      </div>
    </div>
  )
}

function stringify(value: unknown): string {
  if (value == null) return ''
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  try {
    return JSON.stringify(value)
  } catch {
    return String(value)
  }
}
