from abc import ABC, abstractmethod

from parquet_viewer_backend.models import (
    DatasetRef,
    QueryRequest,
    QueryResponse,
    SchemaResponse,
)


class DatasetCatalog(ABC):
    @abstractmethod
    def register(self, ref: DatasetRef) -> DatasetRef:
        raise NotImplementedError

    @abstractmethod
    def get(self, dataset_id: str) -> DatasetRef:
        raise NotImplementedError

    @abstractmethod
    def list(self) -> list[DatasetRef]:
        raise NotImplementedError


class QueryEngine(ABC):
    @abstractmethod
    def schema(self, ref: DatasetRef) -> SchemaResponse:
        raise NotImplementedError

    @abstractmethod
    def preview(self, ref: DatasetRef, limit: int) -> QueryResponse:
        raise NotImplementedError

    @abstractmethod
    def query(self, ref: DatasetRef, request: QueryRequest) -> QueryResponse:
        raise NotImplementedError


class TileProvider(ABC):
    @abstractmethod
    async def get_tile(
        self,
        z: int,
        x: int,
        y: int,
        dataset: str,
        geom_column: str,
        where: str | None,
    ) -> tuple[int, bytes, str]:
        raise NotImplementedError
