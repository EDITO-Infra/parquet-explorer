//! Projection planning shared by table, spatial, and export queries.

use std::collections::HashSet;

use anyhow::{Result, bail};
use parquet::arrow::ProjectionMask;

use super::super::source::ReaderBuilder;

pub(crate) fn apply_projection(
    mut builder: ReaderBuilder,
    columns: Option<&[String]>,
) -> Result<ReaderBuilder> {
    let Some(columns) = columns else {
        return Ok(builder);
    };
    if columns.is_empty() {
        return Ok(builder);
    }

    let schema = builder.metadata().file_metadata().schema_descr();
    let known_roots: HashSet<&str> = builder
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().as_str())
        .collect();
    for column in columns {
        let root = column.split('.').next().unwrap_or(column);
        if !known_roots.contains(root) {
            bail!("unknown column: {column}");
        }
    }
    let mask = ProjectionMask::columns(schema, columns.iter().map(String::as_str));
    builder = builder.with_projection(mask);
    Ok(builder)
}
