from fastapi import FastAPI

from parquet_viewer_backend.api import router
from parquet_viewer_backend.config import settings

app = FastAPI(title=settings.app_name)
app.include_router(router)
