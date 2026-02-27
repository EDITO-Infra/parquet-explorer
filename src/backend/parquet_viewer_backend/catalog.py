from parquet_viewer_backend.contracts import DatasetCatalog
from parquet_viewer_backend.models import DatasetRef


class InMemoryDatasetCatalog(DatasetCatalog):
    def __init__(self) -> None:
        self._items: dict[str, DatasetRef] = {}

    def register(self, ref: DatasetRef) -> DatasetRef:
        self._items[ref.id] = ref
        return ref

    def get(self, dataset_id: str) -> DatasetRef:
        try:
            return self._items[dataset_id]
        except KeyError as exc:
            raise KeyError(f"Unknown dataset: {dataset_id}") from exc

    def list(self) -> list[DatasetRef]:
        return sorted(self._items.values(), key=lambda d: d.id)
