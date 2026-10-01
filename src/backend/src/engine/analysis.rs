//! Read-only Parquet physical-layout analysis.
//!
//! This module answers questions such as:
//! - How large are the row groups and individual column chunks?
//! - Which columns dominate compressed storage?
//! - Does the file expose row-group statistics or page indexes?
//! - How many compressed bytes is a viewer-style query likely to touch?
//!
//! None of these functions mutate or rewrite the source Parquet object. The
//! viewer intentionally stays an explorer; a separate optimizer can reuse the
//! response models or move this module into a shared crate later.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};
use parquet::{
    arrow::arrow_reader::ArrowReaderOptions,
    file::{
        metadata::{PageIndexPolicy, ParquetMetaData},
        page_index::column_index::ColumnIndexMetaData,
    },
};

use crate::model::{
    AnalysisFinding, AnalysisRecommendationsResponse, AnalysisSummaryResponse, ColumnAnalysis,
    ColumnPagesAnalysis, ColumnsAnalysisResponse, PageAnalysis, PagesAnalysisQuery,
    PagesAnalysisResponse, QueryCostContributor, QueryCostRequest, QueryCostResponse,
    RowGroupAnalysis, RowGroupColumnAnalysis, RowGroupsAnalysisResponse,
};

use super::{CoreEngine, dataset::OpenedDataset, query::prune_query_row_groups};

const MIB: u64 = 1024 * 1024;
const LARGE_ROW_GROUP_BYTES: u64 = 128 * MIB;
const MEDIUM_ROW_GROUP_BYTES: u64 = 64 * MIB;

/// Build a cheap file-level layout summary from ordinary Parquet footer metadata.
fn summary(dataset: &OpenedDataset) -> Result<AnalysisSummaryResponse> {
    let dataset_id = dataset.info.dataset_id.as_str();
    let top_level_columns = dataset.info.columns.len();
    let metadata = dataset.reader_metadata.metadata().as_ref();
    let leaf_columns = metadata.file_metadata().schema_descr().num_columns();

    let mut compressed = 0u64;
    let mut uncompressed = 0u64;
    let mut largest_row_group = 0u64;
    let mut row_groups_with_sorting = 0usize;
    let mut chunks_with_statistics = 0usize;
    let mut chunks_total = 0usize;
    let mut column_index_declared = false;
    let mut offset_index_declared = false;

    for row_group in metadata.row_groups() {
        let row_group_compressed = nonnegative_i64(row_group.compressed_size());
        compressed = compressed.saturating_add(row_group_compressed);
        uncompressed = uncompressed.saturating_add(nonnegative_i64(row_group.total_byte_size()));
        largest_row_group = largest_row_group.max(row_group_compressed);
        row_groups_with_sorting += if row_group.sorting_columns().is_some() {
            1
        } else {
            0
        };

        for column in row_group.columns() {
            chunks_total += 1;
            chunks_with_statistics += if column.statistics().is_some() { 1 } else { 0 };
            column_index_declared |= column.column_index_offset().is_some();
            offset_index_declared |= column.offset_index_offset().is_some();
        }
    }

    let row_group_count = metadata.num_row_groups();
    let rows = metadata.file_metadata().num_rows();

    Ok(AnalysisSummaryResponse {
        dataset_id: dataset_id.to_string(),
        analysis_source: "parquet_footer",
        rows,
        columns: top_level_columns,
        leaf_columns,
        row_groups: row_group_count,
        compressed_data_bytes: compressed,
        uncompressed_data_bytes: uncompressed,
        compression_ratio: ratio(uncompressed, compressed),
        average_row_group_rows: if row_group_count == 0 {
            None
        } else {
            Some(rows as f64 / row_group_count as f64)
        },
        average_row_group_compressed_bytes: if row_group_count == 0 {
            None
        } else {
            Some(compressed as f64 / row_group_count as f64)
        },
        largest_row_group_compressed_bytes: largest_row_group,
        row_groups_with_sorting_columns: row_groups_with_sorting,
        column_chunks_with_statistics: chunks_with_statistics,
        column_chunks_total: chunks_total,
        statistics_coverage: if chunks_total == 0 {
            None
        } else {
            Some(chunks_with_statistics as f64 / chunks_total as f64)
        },
        column_index_declared,
        offset_index_declared,
    })
}

/// Aggregate compressed/uncompressed size, encodings, codecs and statistics
/// coverage for every Parquet leaf column across all row groups.
fn columns(dataset: &OpenedDataset) -> Result<ColumnsAnalysisResponse> {
    Ok(columns_from_metadata(
        &dataset.info.dataset_id,
        dataset.reader_metadata.metadata().as_ref(),
    ))
}

fn columns_from_metadata(dataset_id: &str, metadata: &ParquetMetaData) -> ColumnsAnalysisResponse {
    let schema = metadata.file_metadata().schema_descr();
    let total_compressed = metadata.row_groups().iter().fold(0u64, |total, rg| {
        total.saturating_add(nonnegative_i64(rg.compressed_size()))
    });

    let mut output = Vec::with_capacity(schema.num_columns());
    for (leaf_index, descriptor) in schema.columns().iter().enumerate() {
        let mut compressed = 0u64;
        let mut uncompressed = 0u64;
        let mut values = 0u64;
        let mut codecs = BTreeSet::new();
        let mut encodings = BTreeSet::new();
        let mut statistics = 0usize;
        let mut geo_statistics = 0usize;
        let mut column_index_declared = false;
        let mut offset_index_declared = false;

        for row_group in metadata.row_groups() {
            let chunk = row_group.column(leaf_index);
            compressed = compressed.saturating_add(nonnegative_i64(chunk.compressed_size()));
            uncompressed = uncompressed.saturating_add(nonnegative_i64(chunk.uncompressed_size()));
            values = values.saturating_add(nonnegative_i64(chunk.num_values()));
            codecs.insert(format!("{:?}", chunk.compression()));
            encodings.extend(chunk.encodings().map(|encoding| format!("{encoding:?}")));
            statistics += if chunk.statistics().is_some() { 1 } else { 0 };
            geo_statistics += if chunk.geo_statistics().is_some() {
                1
            } else {
                0
            };
            column_index_declared |= chunk.column_index_offset().is_some();
            offset_index_declared |= chunk.offset_index_offset().is_some();
        }

        output.push(ColumnAnalysis {
            name: descriptor.path().string(),
            leaf_index,
            physical_type: format!("{:?}", descriptor.physical_type()),
            logical_type: descriptor
                .logical_type_ref()
                .map(|value| format!("{value:?}")),
            compressed_bytes: compressed,
            uncompressed_bytes: uncompressed,
            percent_of_compressed_data: percent(compressed, total_compressed),
            num_values: values,
            average_compressed_bytes_per_value: if values == 0 {
                None
            } else {
                Some(compressed as f64 / values as f64)
            },
            compression_codecs: codecs.into_iter().collect(),
            encodings: encodings.into_iter().collect(),
            row_groups_with_statistics: statistics,
            row_groups_with_geo_statistics: geo_statistics,
            row_groups_total: metadata.num_row_groups(),
            column_index_declared,
            offset_index_declared,
        });
    }

    ColumnsAnalysisResponse {
        dataset_id: dataset_id.to_string(),
        analysis_source: "parquet_footer",
        compressed_data_bytes: total_compressed,
        columns: output,
    }
}

/// Return row-group and per-column-chunk metadata without reading any data pages.
fn row_groups(dataset: &OpenedDataset) -> Result<RowGroupsAnalysisResponse> {
    let dataset_id = dataset.info.dataset_id.as_str();
    let metadata = dataset.reader_metadata.metadata().as_ref();
    let schema = metadata.file_metadata().schema_descr();

    let row_groups = metadata
        .row_groups()
        .iter()
        .enumerate()
        .map(|(index, row_group)| {
            let columns = row_group
                .columns()
                .iter()
                .enumerate()
                .map(|(leaf_index, chunk)| {
                    let (range_offset, range_length) = chunk.byte_range();
                    RowGroupColumnAnalysis {
                        name: schema.column(leaf_index).path().string(),
                        leaf_index,
                        compressed_bytes: nonnegative_i64(chunk.compressed_size()),
                        uncompressed_bytes: nonnegative_i64(chunk.uncompressed_size()),
                        num_values: nonnegative_i64(chunk.num_values()),
                        compression: format!("{:?}", chunk.compression()),
                        encodings: chunk
                            .encodings()
                            .map(|value| format!("{value:?}"))
                            .collect(),
                        statistics_available: chunk.statistics().is_some(),
                        geo_statistics_available: chunk.geo_statistics().is_some(),
                        data_page_offset: chunk.data_page_offset(),
                        dictionary_page_offset: chunk.dictionary_page_offset(),
                        byte_range_offset: range_offset,
                        byte_range_length: range_length,
                        column_index_declared: chunk.column_index_offset().is_some(),
                        offset_index_declared: chunk.offset_index_offset().is_some(),
                    }
                })
                .collect();

            RowGroupAnalysis {
                index,
                rows: row_group.num_rows(),
                compressed_bytes: nonnegative_i64(row_group.compressed_size()),
                uncompressed_bytes: nonnegative_i64(row_group.total_byte_size()),
                file_offset: row_group.file_offset(),
                sorting_columns_declared: row_group.sorting_columns().is_some(),
                columns,
            }
        })
        .collect();

    Ok(RowGroupsAnalysisResponse {
        dataset_id: dataset_id.to_string(),
        analysis_source: "parquet_footer",
        row_groups,
    })
}

/// Load the optional Parquet page indexes and expose page offsets/sizes.
///
/// Unlike the other analysis endpoints this can issue additional object-store
/// range reads. It still does not read or decode page payloads.
async fn pages(
    dataset: &OpenedDataset,
    query: PagesAnalysisQuery,
) -> Result<PagesAnalysisResponse> {
    let dataset_id = dataset.info.dataset_id.as_str();
    let options = ArrowReaderOptions::new().with_page_index_policy(PageIndexPolicy::Optional);
    let (builder, metrics) = dataset.source.builder_with_options(options).await?;
    let metadata = builder.metadata();
    let schema = metadata.file_metadata().schema_descr();

    if let Some(row_group) = query.row_group
        && row_group >= metadata.num_row_groups()
    {
        bail!("row_group {row_group} is out of range");
    }

    let selected_leaves = if let Some(column) = query.column.as_ref() {
        let selector = [column.clone()];
        resolve_leaf_indices(metadata, Some(&selector))?
    } else {
        resolve_leaf_indices(metadata, None)?
    };
    let offset_indexes = metadata.offset_index();
    let column_indexes = metadata.column_index();
    let mut output = Vec::new();

    for (row_group_index, row_group) in metadata.row_groups().iter().enumerate() {
        if query
            .row_group
            .is_some_and(|wanted| wanted != row_group_index)
        {
            continue;
        }

        for &leaf_index in &selected_leaves {
            let column_name = schema.column(leaf_index).path().string();
            let offset_index = offset_indexes
                .and_then(|groups| groups.get(row_group_index))
                .and_then(|columns| columns.get(leaf_index));
            let column_index_available = column_indexes
                .and_then(|groups| groups.get(row_group_index))
                .and_then(|columns| columns.get(leaf_index))
                .is_some_and(|index| !matches!(index, ColumnIndexMetaData::NONE));

            let mut pages = Vec::new();
            if let Some(offset_index) = offset_index {
                let locations = offset_index.page_locations();
                let unencoded = offset_index.unencoded_byte_array_data_bytes();
                for (page_index, location) in locations.iter().enumerate() {
                    let next_first_row = locations
                        .get(page_index + 1)
                        .map(|next| next.first_row_index)
                        .unwrap_or_else(|| row_group.num_rows());
                    pages.push(PageAnalysis {
                        page_index,
                        offset: location.offset,
                        compressed_page_size: location.compressed_page_size,
                        first_row_index: location.first_row_index,
                        row_count: next_first_row.saturating_sub(location.first_row_index),
                        unencoded_byte_array_data_bytes: unencoded
                            .and_then(|values| values.get(page_index))
                            .copied(),
                    });
                }
            }

            output.push(ColumnPagesAnalysis {
                row_group: row_group_index,
                column: column_name,
                leaf_index,
                column_index_available,
                offset_index_available: offset_index.is_some(),
                pages,
            });
        }
    }

    let io = metrics.snapshot();
    let page_indexes_available = output
        .iter()
        .any(|column| column.offset_index_available || column.column_index_available);
    let mut notes = Vec::new();
    if !page_indexes_available {
        notes.push("No Parquet page index was available for the selected columns. The viewer did not scan data pages to reconstruct one.".to_string());
    }
    notes.push("Storage counters cover metadata and page-index reads performed by this analysis request; data-page payloads are not read.".to_string());

    Ok(PagesAnalysisResponse {
        dataset_id: dataset_id.to_string(),
        analysis_source: "parquet_page_index",
        storage_calls: io.calls,
        storage_ranges: io.ranges,
        storage_bytes: io.returned_bytes,
        page_indexes_available,
        columns: output,
        notes,
    })
}

/// Estimate compressed Parquet bytes touched by a row-oriented viewer query.
///
/// For unfiltered queries, row groups are selected from offset/limit. For
/// filtered queries, the estimate is intentionally conservative because the
/// number/location of matches cannot be known from generic footer metadata.
fn query_cost(dataset: &OpenedDataset, request: QueryCostRequest) -> Result<QueryCostResponse> {
    let dataset_id = dataset.info.dataset_id.as_str();
    if request.limit == 0 {
        bail!("limit must be at least 1");
    }

    let metadata = dataset.reader_metadata.metadata().as_ref();
    let schema = metadata.file_metadata().schema_descr();

    let projected = resolve_leaf_indices(metadata, request.columns.as_deref())?;
    let mut read_leaves = projected.iter().copied().collect::<BTreeSet<_>>();

    // Parquet RowFilter evaluates predicate columns even when they are not part
    // of the final projection, so include them in the physical-read estimate.
    for filter in &request.filters {
        let selector = [filter.column.clone()];
        for leaf in resolve_leaf_indices(metadata, Some(&selector))? {
            read_leaves.insert(leaf);
        }
    }

    let filtered = !request.filters.is_empty();
    let row_groups = if filtered {
        prune_query_row_groups(dataset, &request.filters)?.row_groups
    } else {
        row_groups_for_window(metadata, request.offset, request.limit)
    };

    let mut by_column = BTreeMap::<String, u64>::new();
    let mut estimated = 0u64;
    for &row_group_index in &row_groups {
        let row_group = metadata.row_group(row_group_index);
        for &leaf_index in &read_leaves {
            let bytes = nonnegative_i64(row_group.column(leaf_index).compressed_size());
            estimated = estimated.saturating_add(bytes);
            let name = schema.column(leaf_index).path().string();
            let total = by_column.entry(name).or_default();
            *total = total.saturating_add(bytes);
        }
    }

    let mut contributors = by_column
        .into_iter()
        .map(|(column, compressed_bytes)| QueryCostContributor {
            column,
            compressed_bytes,
            percent_of_estimated_read: percent(compressed_bytes, estimated),
        })
        .collect::<Vec<_>>();
    contributors.sort_by_key(|column| std::cmp::Reverse(column.compressed_bytes));

    let mut warnings = vec![
        "Estimate uses compressed column-chunk sizes from Parquet metadata and does not execute the query.".to_string(),
        "Actual object-store bytes may differ when page indexes, caches, HTTP range coalescing, or predicate pushdown change what the reader fetches.".to_string(),
    ];
    if filtered {
        warnings.push("Filter estimates apply the same conservative row-group statistics pruning as query execution; exact row matches still require RowFilter evaluation.".to_string());
    }

    let column_chunks = read_leaves.len().saturating_mul(row_groups.len());

    Ok(QueryCostResponse {
        dataset_id: dataset_id.to_string(),
        analysis_source: "parquet_footer",
        estimate_kind: if filtered {
            "statistics_pruned_upper_bound"
        } else {
            "row_group_chunk_estimate"
        },
        requested_rows: request.limit,
        requested_columns: projected.len(),
        row_groups,
        column_chunks,
        estimated_compressed_bytes_read: estimated,
        contributors,
        warnings,
    })
}

/// Produce read-only findings aimed at interactive/browser access patterns.
fn recommendations(dataset: &OpenedDataset) -> Result<AnalysisRecommendationsResponse> {
    let dataset_id = dataset.info.dataset_id.as_str();
    let info = &dataset.info;
    let metadata = dataset.reader_metadata.metadata().as_ref();
    let columns = columns_from_metadata(dataset_id, metadata);
    let rg_count = metadata.num_row_groups();
    let total_compressed = columns.compressed_data_bytes;
    let average_rg = if rg_count == 0 {
        0
    } else {
        total_compressed / rg_count as u64
    };
    let mut findings = Vec::new();

    if average_rg >= LARGE_ROW_GROUP_BYTES {
        findings.push(AnalysisFinding {
            code: "LARGE_ROW_GROUPS",
            severity: "high",
            title: "Row groups are very large".to_string(),
            detail: format!("Average compressed row-group size is {} MiB. Small interactive queries can therefore touch much more data than they return.", average_rg / MIB),
            recommendation: Some("For interactive workloads, consider producing a separate file with smaller row groups in an external optimization workflow.".to_string()),
        });
    } else if average_rg >= MEDIUM_ROW_GROUP_BYTES {
        findings.push(AnalysisFinding {
            code: "LARGE_ROW_GROUPS",
            severity: "medium",
            title: "Row groups are relatively large".to_string(),
            detail: format!("Average compressed row-group size is {} MiB.", average_rg / MIB),
            recommendation: Some("If small-range reads are common, compare query read amplification before choosing a smaller row-group target.".to_string()),
        });
    }

    let geo_names = info
        .geo_columns
        .iter()
        .map(|column| column.name.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(geometry) = columns
        .columns
        .iter()
        .filter(|column| {
            geo_names.contains(column.name.as_str())
                || geo_names
                    .iter()
                    .any(|root| column.name.starts_with(&format!("{root}.")))
        })
        .max_by_key(|column| column.compressed_bytes)
        && geometry.percent_of_compressed_data >= 50.0
    {
        findings.push(AnalysisFinding {
                code: "GEOMETRY_DOMINATES_STORAGE",
                severity: if geometry.percent_of_compressed_data >= 80.0 { "high" } else { "medium" },
                title: "Geometry dominates compressed storage".to_string(),
                detail: format!("{} accounts for {:.1}% of compressed column data.", geometry.name, geometry.percent_of_compressed_data),
                recommendation: Some("Expect geometry-inclusive queries to be much more expensive than scalar-column previews; inspect query-cost and page layout before changing the file elsewhere.".to_string()),
        });
    }

    let declared_page_index = columns
        .columns
        .iter()
        .any(|column| column.offset_index_declared || column.column_index_declared);
    if !declared_page_index {
        findings.push(AnalysisFinding {
            code: "NO_PAGE_INDEX",
            severity: "medium",
            title: "No Parquet page index is declared".to_string(),
            detail: "The file does not advertise page-level offset/min-max indexes in its column-chunk metadata.".to_string(),
            recommendation: Some("Page indexes can improve page skipping for compatible readers; consider them when producing an interactive-access copy in a separate workflow.".to_string()),
        });
    }

    let chunks_total = metadata
        .row_groups()
        .iter()
        .map(|rg| rg.num_columns())
        .sum::<usize>();
    let chunks_with_stats = metadata
        .row_groups()
        .iter()
        .flat_map(|rg| rg.columns())
        .filter(|column| column.statistics().is_some())
        .count();
    if chunks_total > 0 && chunks_with_stats * 100 < chunks_total * 80 {
        findings.push(AnalysisFinding {
            code: "LOW_STATISTICS_COVERAGE",
            severity: "medium",
            title: "Row-group statistics coverage is limited".to_string(),
            detail: format!("Statistics are available on {chunks_with_stats} of {chunks_total} column chunks."),
            recommendation: Some("Statistics help readers prune work for filters. Preserve/write them when generating downstream optimized copies.".to_string()),
        });
    }

    let health = if findings.iter().any(|finding| finding.severity == "high") {
        "poor"
    } else if findings.iter().any(|finding| finding.severity == "medium") {
        "mixed"
    } else {
        "good"
    };

    Ok(AnalysisRecommendationsResponse {
        dataset_id: dataset_id.to_string(),
        analysis_source: "parquet_footer",
        interactive_read_health: health,
        findings,
    })
}

/// Resolve top-level or nested column selectors to Parquet leaf indices.
fn resolve_leaf_indices(
    metadata: &ParquetMetaData,
    columns: Option<&[String]>,
) -> Result<Vec<usize>> {
    let schema = metadata.file_metadata().schema_descr();
    let Some(columns) = columns else {
        return Ok((0..schema.num_columns()).collect());
    };
    if columns.is_empty() {
        return Ok((0..schema.num_columns()).collect());
    }

    let mut selected = BTreeSet::new();
    for requested in columns {
        let mut matched = false;
        for (leaf_index, descriptor) in schema.columns().iter().enumerate() {
            let path = descriptor.path().string();
            let root = path.split('.').next().unwrap_or(path.as_str());
            if path == *requested || root == requested || path.starts_with(&format!("{requested}."))
            {
                selected.insert(leaf_index);
                matched = true;
            }
        }
        if !matched {
            return Err(anyhow!("unknown column: {requested}"));
        }
    }
    Ok(selected.into_iter().collect())
}

/// Select row groups that overlap an unfiltered global offset/limit window.
fn row_groups_for_window(metadata: &ParquetMetaData, offset: usize, limit: usize) -> Vec<usize> {
    if limit == 0 {
        return Vec::new();
    }

    let start = offset as u128;
    let end = start.saturating_add(limit as u128);
    let mut cursor = 0u128;
    let mut selected = Vec::new();

    for (index, row_group) in metadata.row_groups().iter().enumerate() {
        let rows = nonnegative_i64(row_group.num_rows()) as u128;
        let row_group_start = cursor;
        let row_group_end = cursor.saturating_add(rows);
        if row_group_end > start && row_group_start < end {
            selected.push(index);
        }
        if row_group_start >= end {
            break;
        }
        cursor = row_group_end;
    }
    selected
}

fn nonnegative_i64(value: i64) -> u64 {
    u64::try_from(value.max(0)).unwrap_or(0)
}

fn ratio(numerator: u64, denominator: u64) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

impl CoreEngine {
    pub async fn analysis_summary(&self, dataset_id: &str) -> Result<AnalysisSummaryResponse> {
        let dataset = self.dataset(dataset_id)?;
        summary(&dataset)
    }

    pub async fn analysis_columns(&self, dataset_id: &str) -> Result<ColumnsAnalysisResponse> {
        let dataset = self.dataset(dataset_id)?;
        columns(&dataset)
    }

    pub async fn analysis_row_groups(&self, dataset_id: &str) -> Result<RowGroupsAnalysisResponse> {
        let dataset = self.dataset(dataset_id)?;
        row_groups(&dataset)
    }

    pub async fn analysis_pages(
        &self,
        dataset_id: &str,
        query: PagesAnalysisQuery,
    ) -> Result<PagesAnalysisResponse> {
        let dataset = self.dataset(dataset_id)?;
        pages(&dataset, query).await
    }

    pub async fn analysis_query_cost(
        &self,
        dataset_id: &str,
        request: QueryCostRequest,
    ) -> Result<QueryCostResponse> {
        let dataset = self.dataset(dataset_id)?;
        query_cost(&dataset, request)
    }

    pub async fn analysis_recommendations(
        &self,
        dataset_id: &str,
    ) -> Result<AnalysisRecommendationsResponse> {
        let dataset = self.dataset(dataset_id)?;
        recommendations(&dataset)
    }
}
