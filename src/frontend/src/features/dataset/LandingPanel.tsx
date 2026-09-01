/** Empty-state explanation shown before a dataset has been opened. */
export function LandingPanel() {
  return (
    <main className="landing">
      <div className="landing-card">
        <span className="eyebrow">Parquet + GeoParquet</span>
        <h2>Explore a Parquet file without downloading it first.</h2>
        <p>
          Paste a file URL to preview rows, filter values, inspect the schema,
          and map spatial data when it is available.
        </p>
        <div className="landing-features" aria-label="Viewer features">
          <span>Preview rows</span>
          <span>Filter data</span>
          <span>Inspect fields</span>
          <span>Map geometry</span>
        </div>
      </div>
    </main>
  )
}
