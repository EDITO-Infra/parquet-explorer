# Frontend

## Role of the frontend

The frontend is a React application responsible for interaction, presentation, browser-side decoding, and map rendering. It should treat the Rust service as the source of truth for Parquet semantics and physical-layout analysis.

The browser receives two main forms of data:

- JSON for dataset metadata, analysis, counts, and diagnostics;
- Arrow IPC streams for rows.

## Main state

`App.tsx` currently coordinates the high-level viewer state:

- active dataset handle and URI;
- active tab;
- table offset/limit;
- visible columns;
- filter clauses;
- loaded rows;
- map snapshot state;
- progress/diagnostic activity;
- application theme.

The exact component split may evolve. The stable idea is that table state and map snapshot state are related but not identical.

## Opening a dataset

When the user opens a URL:

```text
form submit
  ↓
create client trace ID
  ↓
POST /datasets/open
  ↓
receive DatasetInfo
  ↓
initialize columns/count/spatial metadata
  ↓
load first table page
```

Opening should remain metadata-oriented. The UI should become useful quickly without waiting for a large table or geometry read.

## Table data flow

Table rows are streamed as Arrow IPC rather than returned as JSON objects.

```text
page request
  ↓
fetch streaming response
  ↓
Arrow JS stream decoder
  ↓
RecordBatch
  ↓
plain rows for table presentation
```

The frontend API helpers live in `api.ts`; Arrow-specific conversion helpers live in `arrow.ts`. UI components should generally consume typed application data rather than implement HTTP/Arrow protocol details themselves.

### Visible columns affect backend work

The selected table columns are sent as the query projection. This is an important performance behavior, not just a display preference: hiding an expensive column can avoid reading that Parquet column chunk.

Spatial coordinate columns may be added to a request when needed for a fast map preview. Large WKB geometry is intentionally not forced into every ordinary table request.

## Map model

The map is a **frozen snapshot**, not a live viewport query system.

When the user chooses to view the current result on the map, the application captures the current filter state and streams the complete matching spatial result once. Panning, zooming, changing basemap, or switching projection does not issue new Parquet queries.

This keeps map interaction predictable and makes the relationship between table filters and map content explicit.

The browser is responsible for converting spatial values into renderable GeoJSON features. WKB parsing occurs in browser code (`wkb.ts` / `spatialFeatures.ts`), while MapLibre handles visual rendering and interaction.

## Progress and diagnostics

The progress surface merges two independent timelines:

### Backend events

Polled from `/diagnostics/{trace_id}` and produced close to the engine/storage operations.

Examples:

- metadata storage reads;
- Parquet reader milestones;
- object-store bytes/time;
- Arrow IPC encoding;
- response-channel backpressure.

### Browser events

Measured directly with `performance.now()` around browser work.

Examples:

- waiting for headers/response bytes;
- Arrow stream decoding;
- constructing table rows;
- WKB geometry decoding.

The UI must preserve the distinction between a **milestone timestamp** and a **measured duration**. Do not infer a duration merely because one event appeared 60 seconds after another.

Diagnostics are secondary to the data request. Polling failures are ignored so a temporary trace problem cannot break normal viewing.

## Analysis UI

Typed frontend helpers exist for the read-only analysis API even if the presentation continues to evolve.

The intended frontend rule is:

> React decides how to display analysis; Rust decides what the Parquet layout means.

For example, the frontend may render a storage-by-column chart, but the calculation of compressed bytes per Parquet leaf column should come from `/analysis/columns`, not be recreated from unrelated metadata in JavaScript.

A future Analysis tab can progressively disclose detail:

```text
Summary
  row groups / sizes / compression / index coverage

Storage by column
  geometry 96%
  name 2%
  ...

Query cost
  estimated bytes for current table projection/filter

Findings
  advisory explanations

Technical details
  row groups / page indexes
```

## Component responsibilities

Current files roughly map to these concerns:

| File | Responsibility |
|---|---|
| `App.tsx` | high-level state and workflows |
| `api.ts` | HTTP requests and streamed API access |
| `types.ts` | frontend API/application types |
| `arrow.ts` | Arrow batch/row conversion |
| `DataTable.tsx` | table presentation |
| `ColumnPicker.tsx` | projection/visibility controls |
| `FilterBar.tsx` | user filter construction |
| `ProgressPanel.tsx` | user progress + expandable diagnostics |
| `MapPanel.tsx` | MapLibre map interaction/rendering |
| `spatialFeatures.ts` | rows/batches to map features |
| `wkb.ts` | WKB parsing |
| `spatial.ts` / `geo.ts` | spatial source/metadata helpers |

This table describes intent, not a requirement to preserve the current component boundaries forever.

## Frontend performance principles

A few principles are more durable than implementation details:

- Stream rows; do not wait for a whole large result before decoding.
- Keep projections narrow when possible.
- Do not decode WKB unless geometry is actually needed.
- Do not turn map interaction into repeated remote Parquet scans without an explicit design change.
- Attribute time to the correct layer before optimizing it.
- Keep technical diagnostics available, but present normal users with plain-language progress first.

## Error and stale-request handling

Interactive applications can have overlapping requests—for example, a user changes page or projection before an earlier response completes. The frontend uses request/trace IDs to avoid applying stale results to newer state.

When extending a workflow, consider both success and obsolescence: a response may be valid but no longer relevant to the current UI state.

## Adding a new backend capability to the UI

A clean path is:

1. define/confirm the backend API model;
2. add matching TypeScript types in `types.ts`;
3. add a small typed function in `api.ts`;
4. keep protocol/parsing concerns there;
5. let a component consume the typed result.

Avoid embedding ad-hoc `fetch()` calls and Parquet-specific interpretation across multiple components.
