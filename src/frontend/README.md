# Frontend developer guide

The frontend is a thin React/TypeScript explorer over the Rust Parquet engine. For the project-specific architecture, see [`../../docs/frontend.md`](../../docs/frontend.md).

## Source map

```text
src/
├── main.tsx        browser entry point
├── App.tsx         shared workspace state / cross-feature coordination
├── types.ts        shared contracts
├── lib/            HTTP and Arrow helpers shared by features
└── features/       UI and helpers grouped by user-facing feature
```

Current features:

- `dataset/` — source/open/sidebar UI;
- `table/` — filters, projection, paging, row rendering;
- `schema/` — schema/statistics presentation;
- `map/` — spatial conversion and MapLibre interaction;
- `diagnostics/` — progress UI and trace lifecycle.

The backend analysis API is already available, but the interactive Analysis feature is not yet implemented.

**TODO:** add it under `features/analysis/` and consume the existing typed API helpers rather than duplicating Parquet analysis logic in the browser.

## Ownership rule

Keep cross-feature state in `App.tsx`, feature-local behavior in its feature folder, and shared transport/Arrow conversion in `lib/`. Avoid adding large feature-specific blocks back into `App.tsx`.

`dataset_id` is returned by the Rust backend when a source is opened. It is a temporary handle, not a stable identifier; see [`../../docs/architecture.md`](../../docs/architecture.md#dataset-handles).
