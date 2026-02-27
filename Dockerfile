# Multi-stage backend service Dockerfile
# Backend: Python + FastAPI + DuckDB

FROM python:3.11-slim as backend

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

WORKDIR /app

COPY pyproject.toml README.md ./
COPY src/backend ./src/backend

RUN pip install --no-cache-dir -e .

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD python -c "import httpx; httpx.get('http://localhost:8080/health')"

CMD ["uvicorn", "parquet_viewer_backend.main:app", "--host", "0.0.0.0", "--port", "8080"]
