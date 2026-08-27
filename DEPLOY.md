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
PV_MAX_PAGE_SIZE=25000
PV_MAX_SPATIAL_FEATURES=100000
PV_BATCH_SIZE=8192
PV_MAX_OPEN_DATASETS=512
```

For a public service, strongly consider an explicit `PV_ALLOWED_REMOTE_HOSTS` policy or EDITO egress policy. Presigned HTTPS object URLs are preferable to accepting embedded credentials.

## Scaling

Dataset handles are currently process-local metadata. Start with one backend replica. Before horizontal scaling, change handles to signed stateless source tokens or use a shared registry; stateless tokens are preferred because file contents are not cached in the process.

## Health

```text
GET /api/v1/health
```
