import { useEffect, useMemo, useState } from 'react'
import type { ColumnInfo, FilterClause, FilterOp } from './types'

type DraftFilter = {
  id: number
  column: string
  op: FilterOp
  value: string
}

const NULL_OPS = new Set<FilterOp>(['is_null', 'is_not_null'])
let nextId = 1

export function FilterBar({
  columns,
  filters,
  disabled,
  onApply,
}: {
  columns: ColumnInfo[]
  filters: FilterClause[]
  disabled?: boolean
  onApply: (filters: FilterClause[]) => void
}) {
  const byName = useMemo(() => new Map(columns.map(column => [column.name, column])), [columns])
  const [drafts, setDrafts] = useState<DraftFilter[]>(() => filters.map(toDraft))
  const [error, setError] = useState('')

  useEffect(() => {
    setDrafts(filters.map(toDraft))
  }, [filters])

  function addFilter() {
    const column = columns[0]
    if (!column) return
    setDrafts(current => [...current, {
      id: nextId++,
      column: column.name,
      op: defaultOperator(column),
      value: '',
    }])
  }

  function updateDraft(id: number, patch: Partial<DraftFilter>) {
    setDrafts(current => current.map(draft => {
      if (draft.id !== id) return draft
      const next = { ...draft, ...patch }
      if (patch.column) {
        const column = byName.get(patch.column)
        if (column) {
          next.op = defaultOperator(column)
          next.value = ''
        }
      }
      return next
    }))
  }

  function apply() {
    try {
      const next = drafts.map(draft => compileDraft(draft, byName))
      setError('')
      onApply(next)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  function clear() {
    setDrafts([])
    setError('')
    onApply([])
  }

  return (
    <div className="filter-bar">
      <div className="filter-bar-head">
        <div>
          <strong>Filters</strong>
          {filters.length > 0 && <span className="filter-count">{filters.length} active</span>}
        </div>
        <div className="filter-actions">
          <button className="secondary compact" type="button" disabled={disabled} onClick={addFilter}>+ Add filter</button>
          {(drafts.length > 0 || filters.length > 0) && <button className="secondary compact" type="button" disabled={disabled} onClick={clear}>Clear</button>}
          <button className="compact" type="button" disabled={disabled || (drafts.length === 0 && filters.length === 0)} onClick={apply}>Apply</button>
        </div>
      </div>

      {drafts.length > 0 && <div className="filter-list">
        {drafts.map(draft => {
          const column = byName.get(draft.column)
          const operators = column ? operatorsFor(column) : ['eq'] as FilterOp[]
          const needsValue = !NULL_OPS.has(draft.op)
          const booleanColumn = column?.arrow_type === 'Boolean'
          return (
            <div className="filter-row" key={draft.id}>
              <select
                aria-label="Filter column"
                value={draft.column}
                disabled={disabled}
                onChange={event => updateDraft(draft.id, { column: event.target.value })}
              >
                {columns.map(option => <option key={option.name} value={option.name}>{option.name}</option>)}
              </select>

              <select
                aria-label="Filter operator"
                value={draft.op}
                disabled={disabled}
                onChange={event => updateDraft(draft.id, { op: event.target.value as FilterOp })}
              >
                {operators.map(op => <option key={op} value={op}>{operatorLabel(op)}</option>)}
              </select>

              {needsValue && (booleanColumn ? (
                <select
                  aria-label="Filter value"
                  value={draft.value || 'true'}
                  disabled={disabled}
                  onChange={event => updateDraft(draft.id, { value: event.target.value })}
                >
                  <option value="true">true</option>
                  <option value="false">false</option>
                </select>
              ) : (
                <input
                  aria-label="Filter value"
                  value={draft.value}
                  disabled={disabled}
                  placeholder={valuePlaceholder(column)}
                  onChange={event => updateDraft(draft.id, { value: event.target.value })}
                  onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); apply() } }}
                />
              ))}

              <button
                className="icon-button"
                type="button"
                aria-label="Remove filter"
                disabled={disabled}
                onClick={() => setDrafts(current => current.filter(item => item.id !== draft.id))}
              >
                ×
              </button>
            </div>
          )
        })}
      </div>}
      {error && <div className="filter-error">{error}</div>}
    </div>
  )
}

function toDraft(filter: FilterClause): DraftFilter {
  return {
    id: nextId++,
    column: filter.column,
    op: filter.op,
    value: filter.value == null ? '' : String(filter.value),
  }
}

function compileDraft(draft: DraftFilter, columns: Map<string, ColumnInfo>): FilterClause {
  const column = columns.get(draft.column)
  if (!column) throw new Error(`Unknown column: ${draft.column}`)
  if (NULL_OPS.has(draft.op)) return { column: draft.column, op: draft.op, value: null }
  if (!draft.value.trim()) throw new Error(`Enter a value for ${draft.column}`)

  const value: string | boolean = column.arrow_type === 'Boolean'
    ? draft.value !== 'false'
    : draft.value
  return { column: draft.column, op: draft.op, value }
}

function operatorsFor(column: ColumnInfo): FilterOp[] {
  const type = column.arrow_type
  if (type === 'Boolean') return ['eq', 'neq', 'is_null', 'is_not_null']
  if (isStringType(type)) return ['eq', 'neq', 'contains', 'is_null', 'is_not_null']
  if (isOrderedType(type)) return ['eq', 'neq', 'lt', 'lte', 'gt', 'gte', 'is_null', 'is_not_null']
  return ['eq', 'neq', 'is_null', 'is_not_null']
}

function defaultOperator(_column: ColumnInfo): FilterOp {
  return 'eq'
}

function isStringType(type: string): boolean {
  return type === 'Utf8' || type === 'LargeUtf8' || type === 'Utf8View'
}

function isOrderedType(type: string): boolean {
  return /^(U?Int|Float|Decimal|Date|Time|Timestamp)/.test(type)
}

function valuePlaceholder(column?: ColumnInfo): string {
  if (!column) return 'value'
  if (column.arrow_type.startsWith('Timestamp')) return 'timestamp or raw value'
  if (isOrderedType(column.arrow_type)) return 'number'
  return 'value'
}

function operatorLabel(op: FilterOp): string {
  return ({
    eq: '=',
    neq: '≠',
    lt: '<',
    lte: '≤',
    gt: '>',
    gte: '≥',
    contains: 'contains',
    is_null: 'is null',
    is_not_null: 'is not null',
  } satisfies Record<FilterOp, string>)[op]
}
