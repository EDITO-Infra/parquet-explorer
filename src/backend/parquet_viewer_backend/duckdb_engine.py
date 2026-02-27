from __future__ import annotations
import base64
import duckdb

from parquet_viewer_backend.config import settings
from parquet_viewer_backend.contracts import QueryEngine
from parquet_viewer_backend.models import ColumnSchema, DatasetRef, QueryRequest, QueryResponse, SchemaResponse
from parquet_viewer_backend.sql_guard import validate_read_only_sql


class DuckDbQueryEngine(QueryEngine):
    def _connect(self) -> duckdb.DuckDBPyConnection:
        con = duckdb.connect(database=":memory:")
        con.execute("SET enable_progress_bar=false")
        con.execute(f"SET memory_limit='{settings.duckdb_memory_limit}'")
        con.execute(f"SET threads={settings.duckdb_threads}")
        # con.execute(f"SET statement_timeout='{settings.query_timeout_s}s'")
        try:
            con.execute("INSTALL httpfs")
        except Exception:
            pass
        con.execute("LOAD httpfs")
        return con

    @staticmethod
    def _safe_dataset_sql(uri: str) -> str:
        return uri.replace("'", "''")

    def _table_expr(self, ref: DatasetRef) -> str:
        uri = self._safe_dataset_sql(ref.uri)
        return f"read_parquet('{uri}', union_by_name=true)"

    def schema(self, ref: DatasetRef) -> SchemaResponse:
        con = self._connect()
        try:
            sql = f"DESCRIBE SELECT * FROM {self._table_expr(ref)} LIMIT 0"
            result = con.execute(sql).fetchall()
            columns = [ColumnSchema(name=row[0], type=row[1]) for row in result]
            return SchemaResponse(dataset_id=ref.id, columns=columns)
        finally:
            con.close()

    def preview(self, ref: DatasetRef, limit: int) -> QueryResponse:
        request = QueryRequest(sql="SELECT * FROM __dataset", limit=limit, offset=0)
        return self.query(ref, request)

    def _safe_value(self, v):
        if isinstance(v, (bytes, bytearray)):
            return base64.b64encode(v).decode("ascii")
        return v
    def query(self, ref: DatasetRef, request: QueryRequest) -> QueryResponse:
        sql = validate_read_only_sql(request.sql)
        capped_limit = min(request.limit, settings.max_rows)

        con = self._connect()
        try:
            wrapped = (
                f"WITH __dataset AS (SELECT * FROM {self._table_expr(ref)}) "
                f"SELECT * FROM ({sql}) q LIMIT {capped_limit + 1} OFFSET {request.offset}"
            )
            cursor = con.execute(wrapped)
            records = cursor.fetchall()
            columns = [desc[0] for desc in cursor.description]

            truncated = len(records) > capped_limit
            if truncated:
                records = records[:capped_limit]

            return QueryResponse(
                columns=columns,
                rows=[[self._safe_value(v) for v in r] for r in records],
                row_count=len(records),
                truncated=truncated,
            )
        finally:
            con.close()
