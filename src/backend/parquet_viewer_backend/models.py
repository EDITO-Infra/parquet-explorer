from typing import Any, Literal

from pydantic import BaseModel, Field


class DatasetRef(BaseModel):
    id: str = Field(min_length=1, max_length=128)
    uri: str = Field(min_length=1, max_length=4096)
    format: Literal["parquet"] = "parquet"


class RegisterDatasetRequest(BaseModel):
    id: str = Field(min_length=1, max_length=128)
    uri: str = Field(min_length=1, max_length=4096)


class QueryRequest(BaseModel):
    sql: str = Field(min_length=1, max_length=50000)
    limit: int = Field(default=1000, ge=1, le=200000)
    offset: int = Field(default=0, ge=0)


class QueryResponse(BaseModel):
    columns: list[str]
    rows: list[list[Any]]
    row_count: int
    truncated: bool


class ColumnSchema(BaseModel):
    name: str
    type: str


class SchemaResponse(BaseModel):
    dataset_id: str
    columns: list[ColumnSchema]


class ErrorResponse(BaseModel):
    detail: str
