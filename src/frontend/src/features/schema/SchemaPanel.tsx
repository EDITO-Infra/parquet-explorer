/**
 * Schema-tab presentation.
 *
 * This view renders metadata already returned when the dataset was opened. It
 * intentionally does not start deeper Parquet analysis requests; those belong
 * in the separate Analysis feature/API so merely opening Schema stays cheap.
 */
import type { DatasetInfo } from '../../types'

export function SchemaPanel({
  dataset,
  spatialAvailable,
}: {
  dataset: DatasetInfo
  spatialAvailable: boolean
}) {
  return (
    <section className="pane schema-pane">
      <div className="schema-intro">
        <div>
          <span className="eyebrow">File overview</span>
          <h2>Schema &amp; statistics</h2>
          <p>Structure, storage layout, and spatial metadata for this file.</p>
        </div>
      </div>

      <div className="schema-summary schema-stats">
        <StatCard label="Rows" value={dataset.num_rows.toLocaleString()} help="Total rows reported by the file metadata" />
        <StatCard label="Fields" value={String(dataset.columns.length)} help="Columns available to query" />
        <StatCard label="Row groups" value={String(dataset.num_row_groups)} help="Parquet storage groups used for reads" />
        <StatCard label="Spatial" value={spatialAvailable ? 'Available' : 'Not detected'} help={spatialAvailable ? 'Map view can be created' : 'No geometry or coordinate pair detected'} />
      </div>

      {dataset.geo_parquet && (
        <div className="schema-summary technical-stats">
          <StatCard label="GeoParquet" value={dataset.geo_parquet.version} help={`Primary field: ${dataset.geo_parquet.primary_column}`} />
          <StatCard label="Metadata checks" value={dataset.geo_parquet.v2_metadata_checks_passed ? 'Passed' : 'Review'} help="GeoParquet metadata validation" />
          <StatCard label="Exact spatial filter" value={dataset.capabilities.spatial_exact ? 'Enabled' : 'Not yet'} help="Current map query capability" />
        </div>
      )}

      {dataset.geo_parquet?.warnings.length ? (
        <div className="warning-box">
          <strong>Metadata notes</strong>
          {dataset.geo_parquet.warnings.map(warning => (
            <div key={warning}>{warning}</div>
          ))}
        </div>
      ) : null}

      <div className="schema-section-head">
        <div>
          <h3>Fields</h3>
          <p>The logical and physical representation of each column.</p>
        </div>
      </div>
      <div className="schema-table">
        <div className="schema-row schema-head">
          <span>Field</span>
          <span>Data type</span>
          <span>Parquet storage</span>
          <span>Optional</span>
        </div>
        {dataset.columns.map(column => (
          <div className="schema-row" key={column.name}>
            <span>{column.name}</span>
            <span className="mono">{column.arrow_type}</span>
            <span>{column.parquet_physical_type ?? '—'}</span>
            <span>{column.nullable ? 'Yes' : 'No'}</span>
          </div>
        ))}
      </div>
    </section>
  )
}

function StatCard({ label, value, help }: { label: string; value: string; help: string }) {
  return (
    <div className="stat-card">
      <span className="section-label">{label}</span>
      <strong>{value}</strong>
      <p>{help}</p>
    </div>
  )
}
