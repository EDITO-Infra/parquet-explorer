import { useMemo, useState } from 'react'
import type { PlainRow } from './arrow'

const ROW_HEIGHT = 34
const VIEWPORT_HEIGHT = 520
const OVERSCAN = 12

export function DataTable({
  columns,
  rows,
}: {
  columns: string[]
  rows: PlainRow[]
}) {
  const [scrollTop, setScrollTop] = useState(0)

  const window = useMemo(() => {
    const visible = Math.ceil(VIEWPORT_HEIGHT / ROW_HEIGHT)
    const start = Math.max(
      0,
      Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN,
    )
    const end = Math.min(
      rows.length,
      start + visible + OVERSCAN * 2,
    )

    return { start, end }
  }, [scrollTop, rows.length])

  if (!columns.length) {
    return <div className="empty-state">No rows loaded.</div>
  }

  const columnWidth = 180
  const tableWidth = columns.length * columnWidth
  const gridTemplateColumns =
    `repeat(${columns.length}, ${columnWidth}px)`

  return (
    <div
      className="virtual-table"
      onScroll={event =>
        setScrollTop(event.currentTarget.scrollTop)
      }
    >
      <div
        className="table-grid table-head"
        style={{
          gridTemplateColumns,
          width: tableWidth,
        }}
      >
        {columns.map(column => (
          <div
            key={column}
            className="cell head-cell"
            title={column}
          >
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
        {rows
          .slice(window.start, window.end)
          .map((row, localIndex) => {
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
                  <div
                    key={column}
                    className="cell"
                    title={stringify(row[column])}
                  >
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
  try { return JSON.stringify(value) } catch { return String(value) }
}
