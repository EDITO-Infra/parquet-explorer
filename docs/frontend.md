# Frontend

## Role

The frontend is the browser workspace for opening a dataset, exploring rows, inspecting schema/statistics, viewing the current result on a map, and seeing request progress/diagnostics. See [Installation and local setup](installation.md) for current tool requirements.

Parquet reading and physical-layout interpretation remain backend responsibilities.

## Source organization

The frontend is organized by ownership rather than by page:

```text
src/frontend/src/
├── App.tsx              shared workspace coordination
├── main.tsx             browser entry point
├── types.ts             shared frontend/API types
├── lib/                 shared transport/data helpers
└── features/
    ├── dataset/         open/source/sidebar UI
    ├── table/           filters, projection, paging, table
    ├── schema/          schema/statistics presentation
    ├── map/             spatial conversion + MapLibre
    └── diagnostics/     trace lifecycle + progress UI
```

`App.tsx` should coordinate state that genuinely crosses features. Feature-specific UI and logic should stay in the owning feature folder.

## Shared data flow

The common table path is:

```text
UI state
  -> lib/api.ts
  -> Rust API
  -> Arrow IPC stream
  -> Arrow JS RecordBatch
  -> browser-friendly rows
  -> table
```

`lib/api.ts` is the frontend transport boundary. React components should generally use its typed helpers instead of introducing scattered `fetch()` calls.

## Dataset IDs

The frontend does not create dataset IDs. `openDataset()` receives `DatasetInfo` from the backend and `App.tsx` stores it in state. Subsequent requests use `dataset.dataset_id`.

The ID is temporary and process-local. Opening another URL best-effort closes the previous handle. If a handle expires or the backend restarts, the source must be opened again.

## State ownership

Keep state at the lowest level that needs to share it:

- active dataset, filters, paging, complete-result cache, and map coordination belong at the workspace level when multiple features need them;
- local interaction state stays inside the relevant feature;
- transport and Arrow conversion stay outside React components.

The aim is to avoid both a giant `App.tsx` and a large number of tiny components with no clear responsibility.

## Table

The table feature owns filter/projection controls, paging, and row rendering. Unfiltered browsing uses backend pages. Filtered results (and explicit **All**) are streamed once into a browser cache and table paging is then local, with a maximum visible page size of 500,000 rows. Column selection still controls the backend projection.

## Map

The map feature owns spatial-source selection, WKB/coordinate conversion, and MapLibre interaction. Unfiltered Map uses the current table page; filtered or explicit All modes use the shared complete-result cache. Ordinary map interaction does not re-query Parquet.

## Diagnostics

The diagnostics feature combines backend trace events with browser-side measurements. It must preserve the distinction between:

- milestones (`@ elapsed time`);
- measured/derived operation durations.

Browser timings cover work the backend cannot see, such as response-byte arrival, Arrow JS decoding, row materialization, and WKB decoding.

## Analysis UI

The backend analysis API and typed frontend API helpers exist, but there is currently no dedicated interactive Analysis view.

**TODO:** add the Analysis UI under `features/analysis/`. Keep Parquet interpretation in Rust; the frontend should request and present analysis results rather than reproduce the calculations.

Avoid documenting the planned layout/components in more detail until that UI exists.

## Frontend rules worth preserving

- Keep Parquet analysis logic out of React.
- Keep shared HTTP/Arrow transport behind `lib/api.ts`.
- Stream row results where practical.
- Avoid forcing geometry into ordinary unfiltered table-page reads unless it is visible or Map needs that page.
- Ignore stale asynchronous results when newer requests supersede them.
- Split code around coherent responsibilities, not arbitrary line counts.
