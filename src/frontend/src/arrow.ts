import type { RecordBatch } from 'apache-arrow'

export type PlainRow = Record<string, unknown>

export function batchRows(batch: RecordBatch, maxRows = Number.POSITIVE_INFINITY): PlainRow[] {
  const rows: PlainRow[] = []
  const count = Math.min(batch.numRows, maxRows)
  for (let rowIndex = 0; rowIndex < count; rowIndex += 1) {
    const row: PlainRow = {}
    for (let columnIndex = 0; columnIndex < batch.numCols; columnIndex += 1) {
      const field = batch.schema.fields[columnIndex]
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
