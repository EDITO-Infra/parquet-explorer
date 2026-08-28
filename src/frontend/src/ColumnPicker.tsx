import type { ColumnInfo } from './types'

export function ColumnPicker({
  columns,
  visibleColumns,
  disabled,
  onChange,
}: {
  columns: ColumnInfo[]
  visibleColumns: string[]
  disabled?: boolean
  onChange: (columns: string[]) => void
}) {
  const visible = new Set(visibleColumns)

  function toggle(column: string, checked: boolean) {
    if (checked) {
      onChange(columns.map(item => item.name).filter(name => name === column || visible.has(name)))
    } else {
      onChange(visibleColumns.filter(name => name !== column))
    }
  }

  return (
    <details className="column-picker">
      <summary>
        Columns <span>{visibleColumns.length}/{columns.length}</span>
      </summary>
      <div className="column-picker-panel">
        <div className="column-picker-actions">
          <button
            className="compact secondary"
            type="button"
            disabled={disabled || visibleColumns.length === columns.length}
            onClick={() => onChange(columns.map(column => column.name))}
          >
            Show all
          </button>
          <button
            className="compact secondary"
            type="button"
            disabled={disabled || visibleColumns.length === 0}
            onClick={() => onChange([])}
          >
            Hide all
          </button>
        </div>
        <div className="column-checklist">
          {columns.map(column => (
            <label key={column.name} title={column.arrow_type}>
              <input
                type="checkbox"
                checked={visible.has(column.name)}
                disabled={disabled}
                onChange={event => toggle(column.name, event.target.checked)}
              />
              <span>{column.name}</span>
            </label>
          ))}
        </div>
      </div>
    </details>
  )
}
