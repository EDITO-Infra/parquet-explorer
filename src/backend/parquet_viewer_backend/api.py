from fastapi import APIRouter, HTTPException, Query, Response
import logging
from parquet_viewer_backend.config import settings
from parquet_viewer_backend.deps import catalog, query_engine, tile_provider
from parquet_viewer_backend.models import (
    DatasetRef,
    ErrorResponse,
    QueryRequest,
    QueryResponse,
    RegisterDatasetRequest,
    SchemaResponse,
)
from parquet_viewer_backend.sql_guard import SqlGuardError

router = APIRouter()


@router.get("/health")
def health() -> dict[str, str]:
    return {"status": "ok", "env": settings.app_env}


@router.get("/api/datasets", response_model=list[DatasetRef])
def list_datasets() -> list[DatasetRef]:
    return catalog.list()


@router.post("/api/datasets", response_model=DatasetRef)
def register_dataset(payload: RegisterDatasetRequest) -> DatasetRef:
    ref = DatasetRef(id=payload.id, uri=payload.uri)
    return catalog.register(ref)


@router.get(
    "/api/schema",
    response_model=SchemaResponse,
    responses={404: {"model": ErrorResponse}},
)
def get_schema(dataset: str = Query(..., min_length=1)) -> SchemaResponse:
    try:
        ref = catalog.get(dataset)
    except KeyError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    return query_engine.schema(ref)


@router.get(
    "/api/preview",
    response_model=QueryResponse,
    responses={404: {"model": ErrorResponse}},
)
def get_preview(
    dataset: str = Query(..., min_length=1),
    limit: int = Query(default=100, ge=1, le=10_000),
) -> QueryResponse:
    try:
        ref = catalog.get(dataset)
    except KeyError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    return query_engine.preview(ref, min(limit, settings.max_rows))


@router.post(
    "/api/query",
    response_model=QueryResponse,
    responses={400: {"model": ErrorResponse}, 404: {"model": ErrorResponse}},
)
def run_query(dataset: str = Query(..., min_length=1), request: QueryRequest | None = None) -> QueryResponse:
    if request is None:
        raise HTTPException(status_code=400, detail="Missing request body")

    try:
        ref = catalog.get(dataset)
    except KeyError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc

    try:
        return query_engine.query(ref, request)
    except SqlGuardError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


@router.get("/api/geo/eligible")
def is_geo_eligible(
    dataset: str = Query(..., min_length=1),
    geom_column: str | None = Query(default=None),
):
    try:
        schema = get_schema(dataset)
    except HTTPException:
        raise
    logging.debug("Checking geo eligibility for dataset %s with schema: %s", dataset, schema)
    # Auto-detect BLOB column if none specified
    if geom_column is None:
        blob_cols = [
            c for c in schema.columns
            if c.type.upper() in {"BLOB", "BYTEA", "VARBINARY"}
        ]
        if not blob_cols:
            logging.debug("No binary columns found in dataset %s schema, not eligible for geo features", dataset)
            return {"eligible": False, "reason": "No binary geometry column found"}
        geom_column = blob_cols[0].name

    column = next((c for c in schema.columns if c.name == geom_column), None)
    if column is None:
        logging.debug("Specified geometry column %s not found in dataset %s schema", geom_column, dataset)
        return {"eligible": False, "reason": f"Missing column: {geom_column}"}

    if column.type.upper() not in {"BLOB", "BYTEA", "VARBINARY"}:
        logging.debug("Specified geometry column %s in dataset %s is not binary type (found type %s)", geom_column, dataset, column.type)
        return {
            "eligible": False,
            "reason": f"Column {geom_column} is not binary WKB (type={column.type})",
        }
    logging.info("Dataset %s is eligible for geo features with geometry column %s", dataset, geom_column)
    return {
        "eligible": True,
        "reason": f"WKB geometry column found: {geom_column}",
        "geom_column": geom_column,
    }


@router.get("/tiles/{z}/{x}/{y}.mvt")
async def get_tile(
    z: int,
    x: int,
    y: int,
    dataset: str = Query(..., min_length=1),
    geom_column: str = Query(default="geom", min_length=1),
    where: str | None = Query(default=None),
) -> Response:
    status, payload, content_type = await tile_provider.get_tile(
        z=z,
        x=x,
        y=y,
        dataset=dataset,
        geom_column=geom_column,
        where=where,
    )

    if status == 501:
        raise HTTPException(
            status_code=501,
            detail="Tile service is not configured. Set PV_TILE_SERVICE_URL.",
        )

    if status >= 400:
        raise HTTPException(status_code=status, detail="Tile provider error")

    return Response(content=payload, media_type=content_type)
