# EDITO deployment

Recommended topology:

```text
EDITO Static Page / frontend container
    React + Arrow JS + MapLibre
              │
              │ /api
              ▼
EDITO Generic Service
    parquet-viewer-server
    axum + Arrow/Parquet-RS
```

For fastest initial deployment, the supplied frontend Docker image can reverse-proxy `/api` to the Rust service. If EDITO Static Pages are preferred, build `src/frontend` with `VITE_API_URL` pointing to the public Rust API URL.

## Backend environment

```text
PV_CORS_ORIGINS=https://your-viewer.example
PV_ALLOWED_SOURCE_SCHEMES=https,http
PV_ALLOWED_REMOTE_HOSTS=
PV_ALLOW_PRIVATE_NETWORKS=false
PV_MAX_PAGE_SIZE=10000
PV_MAX_SPATIAL_FEATURES=100000
PV_BATCH_SIZE=8192
PV_MAX_OPEN_DATASETS=512
PV_DATASET_IDLE_TIMEOUT_SECONDS=3600
```

For a public service, strongly consider an explicit `PV_ALLOWED_REMOTE_HOSTS` policy or EDITO egress policy. Presigned HTTPS object URLs are preferable to accepting embedded credentials.

## Dataset handle lifetime

Opened datasets are process-local, lightweight handles containing the validated URI and viewer metadata, not cached Parquet contents. `PV_DATASET_IDLE_TIMEOUT_SECONDS` controls how long an unused handle stays registered (default `3600`; `0` disables expiration). A periodic task cleans abandoned handles, and each successful dataset-handle lookup refreshes the idle timestamp.

## Scaling

Dataset handles are currently process-local metadata. Start with one backend replica. A server restart intentionally invalidates all existing IDs. Before horizontal scaling, use sticky routing, a shared registry, or change handles to signed stateless source tokens; stateless tokens are attractive because file contents are not cached in the process.

## Health

```text
GET /api/v1/health
```
