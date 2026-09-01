/**
 * Left-hand controls and lightweight metadata for the currently open dataset.
 *
 * This panel deliberately shows only information already known from the open
 * response. Deep physical-layout analysis belongs to the Analysis feature so
 * expanding "File details" cannot accidentally trigger expensive remote I/O.
 */
import { downloadSubset } from '../../lib/api'
import type { DatasetInfo, FilterClause } from '../../types'
import type { SpatialSource } from '../map/spatial'

export function DatasetSidebar({
  dataset,
  spatialSource,
  filters,
}: {
  dataset: DatasetInfo
  spatialSource?: SpatialSource
  filters: FilterClause[]
}) {
  return (
    <aside className="sidebar">
      <div className="side-section dataset-summary">
        <span className="section-label">Current file</span>
        <div className="uri" title={dataset.uri}>
          {dataset.name || filename(dataset.uri)}
        </div>
        <div className="muted source-uri" title={dataset.uri}>{dataset.uri}</div>
      </div>

      <details className="side-section file-details">
        <summary>File details</summary>
        <div className="stats-grid compact-stats">
          <Metric label="Rows" value={dataset.num_rows.toLocaleString()} />
          <Metric label="Fields" value={String(dataset.columns.length)} />
          <Metric label="Row groups" value={String(dataset.num_row_groups)} />
          <Metric label="Spatial" value={spatialSource ? 'Yes' : 'No'} />
        </div>

        {spatialSource && (
          <div className="spatial-details">
            <span className="section-label">Spatial data</span>
            {spatialSource.kind === 'coordinates' ? (
              <dl className="metadata-list">
                <dt>Type</dt><dd>Coordinates</dd>
                <dt>Longitude</dt><dd>{spatialSource.longitude}</dd>
                <dt>Latitude</dt><dd>{spatialSource.latitude}</dd>
              </dl>
            ) : (
              <dl className="metadata-list">
                <dt>Field</dt><dd>{spatialSource.geometry.name}</dd>
                <dt>Type</dt><dd>{spatialSource.geometry.logical_type}</dd>
                <dt>CRS</dt><dd>{spatialSource.geometry.crs}</dd>
                <dt>Bbox stats</dt>
                <dd>
                  {spatialSource.geometry.row_groups_with_bbox}/
                  {spatialSource.geometry.row_groups_total} row groups
                </dd>
              </dl>
            )}
          </div>
        )}
      </details>

      <div className="side-section">
        <span className="section-label">Download</span>
        <div className="button-stack">
          <button
            className="secondary"
            onClick={() => void downloadSubset(dataset.dataset_id, 'parquet', undefined, filters)}
          >
            Save as Parquet
          </button>
          <button
            className="secondary"
            onClick={() => void downloadSubset(dataset.dataset_id, 'arrow', undefined, filters)}
          >
            Save as Arrow
          </button>
        </div>
      </div>
    </aside>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <span className="metric-value">{value}</span>
      <span className="metric-label">{label}</span>
    </div>
  )
}

function filename(uri: string) {
  try {
    return new URL(uri).pathname.split('/').filter(Boolean).pop() ?? uri
  } catch {
    return uri
  }
}
