//! Conservative row-group pruning from Parquet footer statistics.
//!
//! This module may only discard a row group when the cached metadata proves the
//! filter cannot match. Unknown, missing, or inexact statistics retain the row
//! group and exact `RowFilter` evaluation remains authoritative.

use anyhow::{Result, anyhow};
use arrow::{
    array::{Array, BooleanArray},
    compute::kernels::cmp,
};
use parquet::arrow::arrow_reader::statistics::StatisticsConverter;

use crate::model::{FilterClause, FilterOp};

use super::{super::dataset::OpenedDataset, filter::json_scalar};

#[derive(Debug)]
pub(crate) struct PruningResult {
    pub(crate) row_groups: Vec<usize>,
    pub(crate) filters_pruning_row_groups: usize,
}

pub(crate) fn prune_row_groups(
    dataset: &OpenedDataset,
    filters: &[FilterClause],
) -> Result<PruningResult> {
    let metadata = dataset.reader_metadata.metadata().as_ref();
    let row_group_count = metadata.num_row_groups();
    if filters.is_empty() || row_group_count == 0 {
        return Ok(PruningResult {
            row_groups: (0..row_group_count).collect(),
            filters_pruning_row_groups: 0,
        });
    }

    let parquet_schema = metadata.file_metadata().schema_descr();
    let arrow_schema = dataset.reader_metadata.schema().as_ref();
    let mut keep = vec![true; row_group_count];
    let mut filters_pruning_row_groups = 0usize;

    for clause in filters {
        if matches!(clause.op, FilterOp::Contains) {
            continue;
        }

        let converter =
            match StatisticsConverter::try_new(&clause.column, arrow_schema, parquet_schema) {
                Ok(converter) => converter.with_missing_null_counts_as_zero(false),
                // Exact filter compilation reports unknown/unsupported columns. For
                // pruning, inability to interpret statistics must stay conservative.
                Err(_) => continue,
            };

        let rejected = match clause.op {
            FilterOp::IsNull => match prune_is_null(&converter, metadata) {
                Ok(rejected) => rejected,
                Err(_) => continue,
            },
            FilterOp::IsNotNull => match prune_is_not_null(&converter, metadata) {
                Ok(rejected) => rejected,
                Err(_) => continue,
            },
            FilterOp::Eq
            | FilterOp::Neq
            | FilterOp::Lt
            | FilterOp::Lte
            | FilterOp::Gt
            | FilterOp::Gte => {
                if clause.value.is_null() {
                    continue;
                }
                let scalar = json_scalar(&clause.value, converter.arrow_field().data_type())
                    .map_err(|error| {
                        anyhow!(
                            "invalid filter value for '{}' ({:?}): {error}",
                            clause.column,
                            converter.arrow_field().data_type()
                        )
                    })?;
                match prune_comparison(&converter, metadata, clause.op, &scalar) {
                    Ok(rejected) => rejected,
                    Err(_) => continue,
                }
            }
            FilterOp::Contains => unreachable!(),
        };

        if rejected.iter().any(|value| *value) {
            filters_pruning_row_groups += 1;
        }
        for (index, reject) in rejected.into_iter().enumerate() {
            if reject {
                keep[index] = false;
            }
        }
    }

    Ok(PruningResult {
        row_groups: keep
            .into_iter()
            .enumerate()
            .filter_map(|(index, keep)| keep.then_some(index))
            .collect(),
        filters_pruning_row_groups,
    })
}

fn prune_comparison(
    converter: &StatisticsConverter<'_>,
    metadata: &parquet::file::metadata::ParquetMetaData,
    op: FilterOp,
    scalar: &arrow::array::Scalar<arrow::array::ArrayRef>,
) -> Result<Vec<bool>> {
    let mins = converter.row_group_mins(metadata.row_groups().iter())?;
    let maxes = converter.row_group_maxes(metadata.row_groups().iter())?;
    let min_exact = converter.row_group_is_min_value_exact(metadata.row_groups().iter())?;
    let max_exact = converter.row_group_is_max_value_exact(metadata.row_groups().iter())?;

    let min_gt = cmp::gt(&mins, scalar)?;
    let min_gte = cmp::gt_eq(&mins, scalar)?;
    let max_lt = cmp::lt(&maxes, scalar)?;
    let max_lte = cmp::lt_eq(&maxes, scalar)?;
    let min_eq = cmp::eq(&mins, scalar)?;
    let max_eq = cmp::eq(&maxes, scalar)?;

    let mut rejected = vec![false; metadata.num_row_groups()];
    for (index, is_rejected) in rejected.iter_mut().enumerate() {
        let exact_min = bool_value(&min_exact, index).unwrap_or(false);
        let exact_max = bool_value(&max_exact, index).unwrap_or(false);
        *is_rejected = match op {
            FilterOp::Eq => {
                (exact_min && bool_value(&min_gt, index).unwrap_or(false))
                    || (exact_max && bool_value(&max_lt, index).unwrap_or(false))
            }
            FilterOp::Neq => {
                exact_min
                    && exact_max
                    && bool_value(&min_eq, index).unwrap_or(false)
                    && bool_value(&max_eq, index).unwrap_or(false)
            }
            FilterOp::Lt => exact_min && bool_value(&min_gte, index).unwrap_or(false),
            FilterOp::Lte => exact_min && bool_value(&min_gt, index).unwrap_or(false),
            FilterOp::Gt => exact_max && bool_value(&max_lte, index).unwrap_or(false),
            FilterOp::Gte => exact_max && bool_value(&max_lt, index).unwrap_or(false),
            FilterOp::Contains | FilterOp::IsNull | FilterOp::IsNotNull => false,
        };
    }
    Ok(rejected)
}

fn prune_is_null(
    converter: &StatisticsConverter<'_>,
    metadata: &parquet::file::metadata::ParquetMetaData,
) -> Result<Vec<bool>> {
    let null_counts = converter.row_group_null_counts(metadata.row_groups().iter())?;
    Ok((0..metadata.num_row_groups())
        .map(|index| null_counts.is_valid(index) && null_counts.value(index) == 0)
        .collect())
}

fn prune_is_not_null(
    converter: &StatisticsConverter<'_>,
    metadata: &parquet::file::metadata::ParquetMetaData,
) -> Result<Vec<bool>> {
    let null_counts = converter.row_group_null_counts(metadata.row_groups().iter())?;
    Ok(metadata
        .row_groups()
        .iter()
        .enumerate()
        .map(|(index, row_group)| {
            null_counts.is_valid(index)
                && null_counts.value(index)
                    == u64::try_from(row_group.num_rows()).unwrap_or(u64::MAX)
        })
        .collect())
}

fn bool_value(array: &BooleanArray, index: usize) -> Option<bool> {
    array.is_valid(index).then(|| array.value(index))
}
