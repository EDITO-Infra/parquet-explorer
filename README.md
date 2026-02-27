# parquet-viewer

Interactive Parquet explorer service for EDITO.

## v1 goals

- Explore Parquet datasets (S3 or local mounted storage).
- Run read-only SQL with guardrails through DuckDB.
- Show schema and preview quickly.
- Optional map tab for WKB geometry datasets.
- Keep component boundaries stable so query/tile services can be swapped later.

## Architecture (v1)

- Frontend: React + Vite + TypeScript + MapLibre GL JS
- Query API: Python + FastAPI + DuckDB
- Tile API: separate service endpoint (Rust/Go/Python), pluggable via config

This repo includes a Rust tile service scaffold at `src/tile-rust`.

The frontend only talks to stable HTTP APIs:

- Query API (`/api/*`) for datasets/schema/query/preview.
- Tile API (`/tiles/*`) for map vector tiles.

This means backend pieces can be replaced later without frontend rewrite.

## Why not embed DuckDB UI directly?

DuckDB UI is not designed as a drop-in embeddable app for this multi-tenant service shape.
This project needs:

- S3 + mounted storage controls
- strict query guardrails
- stable API contracts
- map integration with WKB and external tile engines

## Geo policy

- Map tab requires a valid WKB geometry column.
- Non-geo Parquet is fully supported in table/query mode.

## Backend interfaces (for migration)

The backend is designed around replaceable contracts:

- `DatasetCatalog`: resolve dataset id -> physical location.
- `QueryEngine`: execute safe, read-only SQL against resolved datasets.
- `TileProvider`: produce/proxy vector tiles.

You can keep the same API and swap implementations:

- Python DuckDB -> Rust/Go query runtime
- Python tile fallback -> dedicated Rust/Go tile service

## Quick start (backend)

1) Install deps

`pip install -e .[dev]`

2) Start API

`uvicorn parquet_viewer_backend.main:app --host 0.0.0.0 --port 8080`

3) Open API docs

`http://localhost:8080/docs`

## Quick start (frontend)

1) Install frontend deps

`cd src/frontend && npm install`

2) Start dev UI

`npm run dev`

The dev server proxies `/api`, `/health`, and `/tiles` to `http://localhost:8080`.

## Quick start (rust tile service)

1) Install Rust deps

`make install-rust`

2) Start service

`make dev-rust-tile`

3) Point backend to tile service

`export PV_TILE_SERVICE_URL=http://localhost:8090`

## Key configuration

- `PV_MAX_ROWS` (default: `10000`)
- `PV_QUERY_TIMEOUT_S` (default: `30`)
- `PV_DUCKDB_MEMORY_LIMIT` (default: `1GB`)
- `PV_DUCKDB_THREADS` (default: `4`)
- `PV_TILE_SERVICE_URL` (default: empty; if empty, tiles endpoint returns `501`)

## Docker deployment (complete stack)

All three services (backend, frontend, tile-rust) run in containers, networked together.

### Quick Docker start

1) Build all images

`make docker-build`

2) Start all services

`make docker-up`

3) Access services

- Frontend: `http://localhost:3000`
- Backend API: `http://localhost:8080`
- Backend docs: `http://localhost:8080/docs`
- Tile service: `http://localhost:8090/health`

4) View logs

`make docker-logs`

5) Stop services

`make docker-down`

6) Clean up

`make clean-docker`

### Docker images

- **Backend** ([Dockerfile](Dockerfile)): Python 3.11 slim + FastAPI + DuckDB. Port 8080. Healthcheck included.
- **Frontend** ([Dockerfile.frontend](Dockerfile.frontend)): Node 20 Alpine build, served with `serve`. Port 3000. Healthcheck included.
- **Tile service** ([Dockerfile.tile-rust](Dockerfile.tile-rust)): Rust build, small Debian runtime. Port 8090. Healthcheck included.

### Docker environment

The `docker-compose.yml` includes default environment variables. Override in `.env` file or via command line:

```
PV_APP_ENV=production
PV_MAX_ROWS=10000
PV_QUERY_TIMEOUT_S=30
PV_DUCKDB_MEMORY_LIMIT=2GB
PV_DUCKDB_THREADS=4
PV_TILE_SERVICE_URL=http://tile-service:8090
```

All services are networked and can reach each other by container name (e.g., `http://backend:8080` from frontend).

## Deployment notes

- For EDITO shared service, set conservative limits and autoscaling.
- For self-deployed user instances, recommend larger CPU/memory for heavy datasets.
- A Helm chart can expose these limits as values for per-deployment tuning.

## Makefile shortcuts

- `make install` or `make install-all` installs Python + Node + Rust deps.
- `make dev-backend` starts FastAPI API.
- `make dev-frontend` starts Vite UI.
- `make dev-rust-tile` starts Rust tile service.

See [docs/architecture.md](docs/architecture.md), [docs/api.md](docs/api.md), and [DEPLOY.md](DEPLOY.md).
