# Deployment Guide

## Local development with Docker

The quickest way to test locally with all three services:

```bash
make docker-build
make docker-up
```

Then open `http://localhost:3000` in your browser.

## Production deployment

### Helm chart (Kubernetes)

For EDITO platform users deploying to Kubernetes:

```bash
# Add the chart repo (when available)
helm repo add parquet-viewer https://charts.edito.io
helm repo update

# Install with defaults
helm install my-release parquet-viewer/parquet-viewer

# Or with custom values
helm install my-release parquet-viewer/parquet-viewer -f custom-values.yaml
```

**Key Helm values to customize:**

```yaml
backend:
  resources:
    requests:
      memory: "1Gi"
      cpu: "500m"
    limits:
      memory: "4Gi"
      cpu: "2000m"
  env:
    PV_DUCKDB_MEMORY_LIMIT: "2GB"
    PV_DUCKDB_THREADS: "4"

frontend:
  replicas: 2

tileService:
  replicas: 2
  resources:
    requests:
      memory: "256Mi"
      cpu: "250m"
    limits:
      memory: "1Gi"
      cpu: "1000m"

persistence:
  enabled: true
  size: "20Gi"
  storageClass: "standard"
```

### Docker Compose (single machine)

For testing or small deployments on a single Docker host:

```bash
docker-compose -f docker-compose.yml up -d
```

With custom env file:

```bash
docker-compose --env-file .env.prod up -d
```

### Building and pushing images

To build and push images to your registry:

```bash
# Set registry URL
export REGISTRY=gcr.io/my-project

# Build
docker-compose build

# Tag
docker tag parquet-viewer-backend $REGISTRY/parquet-viewer-backend:latest
docker tag parquet-viewer-frontend $REGISTRY/parquet-viewer-frontend:latest
docker tag parquet-viewer-tile-service $REGISTRY/parquet-viewer-tile-service:latest

# Push
docker push $REGISTRY/parquet-viewer-backend:latest
docker push $REGISTRY/parquet-viewer-frontend:latest
docker push $REGISTRY/parquet-viewer-tile-service:latest
```

## Storage setup

### S3 (recommended for cloud)

The backend reads Parquet directly from S3 via signed URLs or IAM roles.

```bash
# With AWS credentials in pod (Kubernetes)
kubectl create secret generic aws-credentials \
  --from-literal=AWS_ACCESS_KEY_ID=... \
  --from-literal=AWS_SECRET_ACCESS_KEY=...
```

### Local mounted storage

Mount a directory with Parquet files:

```bash
# Docker Compose
volumes:
  - ./data:/data

# Then register: s3://my-bucket/ or /data/path/to/dataset.parquet
```

## Configuration

### Environment variables

All runtime config is via `PV_*` env vars:

- `PV_APP_ENV`: `dev` or `production`
- `PV_MAX_ROWS`: maximum result set size (default: 10000)
- `PV_QUERY_TIMEOUT_S`: query timeout in seconds (default: 30)
- `PV_DUCKDB_MEMORY_LIMIT`: DuckDB memory limit (default: 1GB)
- `PV_DUCKDB_THREADS`: DuckDB thread pool size (default: 4)
- `PV_TILE_SERVICE_URL`: tile service endpoint (e.g., `http://tile-service:8090`)

### Security considerations

- The `sql_guard` module enforces read-only queries. Never disable.
- Set `PV_TILE_SERVICE_URL` to an internal endpoint only if filtering tiles server-side.
- Use network policies to restrict backend-to-S3 access.
- Consider rate-limiting on the frontend or load balancer.

## Monitoring and health checks

All containers have built-in health checks:

```bash
# Backend
curl http://localhost:8080/health

# Frontend
curl http://localhost:3000

# Tile service
curl http://localhost:8090/health
```

Logs:

```bash
# Docker Compose
docker-compose logs -f backend
docker-compose logs -f frontend
docker-compose logs -f tile-service
```
