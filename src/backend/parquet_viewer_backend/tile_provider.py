from __future__ import annotations

import httpx

from parquet_viewer_backend.contracts import TileProvider


class NoopTileProvider(TileProvider):
    async def get_tile(
        self,
        z: int,
        x: int,
        y: int,
        dataset: str,
        geom_column: str,
        where: str | None,
    ) -> tuple[int, bytes, str]:
        return 501, b"", "application/x-protobuf"


class HttpTileProvider(TileProvider):
    def __init__(self, base_url: str) -> None:
        self.base_url = base_url.rstrip("/")

    async def get_tile(
        self,
        z: int,
        x: int,
        y: int,
        dataset: str,
        geom_column: str,
        where: str | None,
    ) -> tuple[int, bytes, str]:
        params: dict[str, str] = {"dataset": dataset, "geom_column": geom_column}
        if where:
            params["where"] = where

        url = f"{self.base_url}/tiles/{z}/{x}/{y}.mvt"
        async with httpx.AsyncClient(timeout=20) as client:
            response = await client.get(url, params=params)

        content_type = response.headers.get("content-type", "application/x-protobuf")
        return response.status_code, response.content, content_type
