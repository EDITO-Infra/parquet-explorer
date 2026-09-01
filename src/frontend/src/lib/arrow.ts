/**
 * Apache Arrow -> table-friendly JavaScript conversion helpers.
 *
 * Complete filtered results are kept as Arrow RecordBatches in the browser.
 * Only the currently visible table page is materialized into plain JS rows.
 */
import type { RecordBatch } from 'apache-arrow'

export type PlainRow = Record<string, unknown>

export function batchRows(
  batch: RecordBatch,
  maxRows = Number.POSITIVE_INFINITY,
  columns?: string[],
): PlainRow[] {
  return batchRowsRange(batch, 0, maxRows, columns)
}

/** Convert a row window from one RecordBatch into display-friendly objects. */
export function batchRowsRange(
  batch: RecordBatch,
  startRow: number,
  maxRows: number,
  columns?: string[],
): PlainRow[] {
  const rows: PlainRow[] = []
  const first = Math.max(0, Math.min(batch.numRows, startRow))
  const count = Math.max(0, Math.min(batch.numRows - first, maxRows))
  const selected = columns ? new Set(columns) : undefined

  for (let rowIndex = first; rowIndex < first + count; rowIndex += 1) {
    const row: PlainRow = {}
    for (let columnIndex = 0; columnIndex < batch.numCols; columnIndex += 1) {
      const field = batch.schema.fields[columnIndex]
      if (selected && !selected.has(field.name)) continue
      const vector = batch.getChildAt(columnIndex)
      row[field.name] = plainValue(vector?.get(rowIndex))
    }
    rows.push(row)
  }
  return rows
}

export function plainValue(value: unknown): unknown {
  if (value == null) return null
  if (typeof value === 'bigint') return value.toString()
  if (value instanceof Date) return value.toISOString()
  if (value instanceof Uint8Array) return `<binary ${value.byteLength} bytes>`
  if (Array.isArray(value)) return value.map(plainValue)
  if (typeof value === 'object') {
    const maybe = value as { toJSON?: () => unknown; toString?: () => string }
    if (typeof maybe.toJSON === 'function') return maybe.toJSON()
    if (typeof maybe.toString === 'function') {
      const text = maybe.toString()
      if (text !== '[object Object]') return text
    }
  }
  return value
}
