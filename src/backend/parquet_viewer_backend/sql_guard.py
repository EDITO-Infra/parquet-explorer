import re

READONLY_FORBIDDEN = re.compile(
    r"\b(INSERT|UPDATE|DELETE|CREATE|DROP|ALTER|TRUNCATE|ATTACH|DETACH|COPY|EXPORT|IMPORT|INSTALL|LOAD|CALL)\b",
    flags=re.IGNORECASE,
)


class SqlGuardError(ValueError):
    pass


def validate_read_only_sql(sql: str) -> str:
    normalized = sql.strip()
    if not normalized:
        raise SqlGuardError("SQL must not be empty")

    if ";" in normalized[:-1]:
        raise SqlGuardError("Only a single SQL statement is allowed")

    if READONLY_FORBIDDEN.search(normalized):
        raise SqlGuardError("Only read-only SQL is allowed")

    if not normalized.upper().startswith(("SELECT", "WITH", "SHOW", "DESCRIBE", "EXPLAIN")):
        raise SqlGuardError("SQL must start with SELECT/WITH/SHOW/DESCRIBE/EXPLAIN")

    return normalized.rstrip(";")
