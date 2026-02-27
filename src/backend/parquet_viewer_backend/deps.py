from parquet_viewer_backend.catalog import InMemoryDatasetCatalog
from parquet_viewer_backend.config import settings
from parquet_viewer_backend.contracts import DatasetCatalog, QueryEngine, TileProvider
from parquet_viewer_backend.duckdb_engine import DuckDbQueryEngine
from parquet_viewer_backend.tile_provider import HttpTileProvider, NoopTileProvider

catalog: DatasetCatalog = InMemoryDatasetCatalog()
query_engine: QueryEngine = DuckDbQueryEngine()

if settings.tile_service_url:
    tile_provider: TileProvider = HttpTileProvider(settings.tile_service_url)
else:
    tile_provider = NoopTileProvider()
