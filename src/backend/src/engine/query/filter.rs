//! Translation of API filter clauses into Parquet-RS `RowFilter` predicates.
//!
//! Filters are pushed into the native reader so predicate columns can be read
//! separately from the output projection and non-matching rows are discarded
//! before Arrow batches are returned to the browser.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use arrow::{
    array::{ArrayRef, BooleanArray, Float64Array, Int64Array, Scalar, StringArray, UInt64Array},
    compute::kernels::{cmp, comparison},
    compute::{cast, is_not_null, is_null},
    datatypes::DataType,
};
use parquet::arrow::{
    ProjectionMask,
    arrow_reader::{ArrowPredicate, ArrowPredicateFn, RowFilter},
};
use serde_json::Value;

use crate::model::{FilterClause, FilterOp};

use super::super::source::ReaderBuilder;

/// Compile API filter clauses into Parquet-RS row predicates.
///
/// Each clause gets its own one-column projection. Parquet-RS evaluates these
/// predicates during decoding and applies the final output projection only
/// after filtering, so a filter column does not need to be present in the
/// table's requested columns.
/// Compile API filter clauses into Parquet-RS row predicates.
///
/// The function validates referenced columns/types first and then attaches a
/// `RowFilter` to the reader builder. No browser-side filtering is required for
/// the supported operators.
pub fn apply_filters(
    mut builder: ReaderBuilder,
    filters: &[FilterClause],
) -> Result<ReaderBuilder> {
    if filters.is_empty() {
        return Ok(builder);
    }

    let parquet_schema = builder.metadata().file_metadata().schema_descr_ptr();
    let arrow_schema = Arc::clone(builder.schema());
    let mut predicates: Vec<Box<dyn ArrowPredicate>> = Vec::with_capacity(filters.len());

    for clause in filters {
        let field = arrow_schema
            .field_with_name(&clause.column)
            .map_err(|_| anyhow!("unknown filter column: {}", clause.column))?;
        let data_type = field.data_type().clone();

        let leaf_index = parquet_schema
            .columns()
            .iter()
            .position(|column| column.path().string() == clause.column)
            .ok_or_else(|| {
                anyhow!(
                    "filter column must currently be a top-level Parquet leaf: {}",
                    clause.column
                )
            })?;

        validate_operator(clause.op, &data_type)?;
        let scalar = if needs_value(clause.op) {
            if clause.value.is_null() {
                bail!(
                    "filter {:?} on '{}' requires a value",
                    clause.op,
                    clause.column
                );
            }
            Some(json_scalar(&clause.value, &data_type).map_err(|error| {
                anyhow!(
                    "invalid filter value for '{}' ({data_type:?}): {error}",
                    clause.column
                )
            })?)
        } else {
            None
        };

        let projection = ProjectionMask::leaves(&parquet_schema, [leaf_index]);
        let op = clause.op;
        let predicate = ArrowPredicateFn::new(projection, move |batch| {
            let column = batch.column(0);
            match op {
                FilterOp::Eq => cmp::eq(column, scalar.as_ref().expect("validated scalar")),
                FilterOp::Neq => cmp::neq(column, scalar.as_ref().expect("validated scalar")),
                FilterOp::Lt => cmp::lt(column, scalar.as_ref().expect("validated scalar")),
                FilterOp::Lte => cmp::lt_eq(column, scalar.as_ref().expect("validated scalar")),
                FilterOp::Gt => cmp::gt(column, scalar.as_ref().expect("validated scalar")),
                FilterOp::Gte => cmp::gt_eq(column, scalar.as_ref().expect("validated scalar")),
                FilterOp::Contains => {
                    comparison::contains(column, scalar.as_ref().expect("validated scalar"))
                }
                FilterOp::IsNull => is_null(column.as_ref()),
                FilterOp::IsNotNull => is_not_null(column.as_ref()),
            }
        });
        predicates.push(Box::new(predicate));
    }

    builder = builder.with_row_filter(RowFilter::new(predicates));
    Ok(builder)
}

fn needs_value(op: FilterOp) -> bool {
    !matches!(op, FilterOp::IsNull | FilterOp::IsNotNull)
}

fn validate_operator(op: FilterOp, data_type: &DataType) -> Result<()> {
    if matches!(op, FilterOp::Contains)
        && !matches!(
            data_type,
            DataType::Utf8
                | DataType::LargeUtf8
                | DataType::Utf8View
                | DataType::Binary
                | DataType::LargeBinary
                | DataType::BinaryView
        )
    {
        bail!("contains filter is only supported for string/binary columns, not {data_type:?}");
    }

    if data_type.is_nested() && !matches!(op, FilterOp::IsNull | FilterOp::IsNotNull) {
        bail!("filter operator {op:?} is not supported for nested type {data_type:?}");
    }
    Ok(())
}

/// Convert a JSON request value to a one-element Arrow scalar matching the
/// Parquet reader's Arrow type. Arrow's cast kernels handle narrowing numeric
/// types and temporal parsing, keeping request parsing independent of each
/// concrete Arrow primitive type.
pub(super) fn json_scalar(value: &Value, data_type: &DataType) -> Result<Scalar<ArrayRef>, String> {
    let source: ArrayRef = match value {
        Value::String(value) => Arc::new(StringArray::from(vec![Some(value.as_str())])),
        Value::Bool(value) => Arc::new(BooleanArray::from(vec![Some(*value)])),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Arc::new(Int64Array::from(vec![Some(value)]))
            } else if let Some(value) = value.as_u64() {
                Arc::new(UInt64Array::from(vec![Some(value)]))
            } else if let Some(value) = value.as_f64() {
                Arc::new(Float64Array::from(vec![Some(value)]))
            } else {
                return Err("number cannot be represented by Arrow".to_string());
            }
        }
        Value::Null => return Err("null is only valid with is_null/is_not_null".to_string()),
        other => return Err(format!("unsupported JSON filter value: {other}")),
    };

    let casted = if source.data_type() == data_type {
        source
    } else {
        cast(source.as_ref(), data_type).map_err(|error| error.to_string())?
    };
    Ok(Scalar::new(casted))
}
