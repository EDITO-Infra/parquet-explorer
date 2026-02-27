from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", env_prefix="PV_", extra="ignore")

    app_name: str = "parquet-viewer"
    app_env: str = "dev"

    max_rows: int = Field(default=10_000, ge=1, le=200_000)
    query_timeout_s: int = Field(default=30, ge=1, le=600)

    duckdb_memory_limit: str = "1GB"
    duckdb_threads: int = Field(default=4, ge=1, le=64)

    tile_service_url: str = ""


settings = Settings()
