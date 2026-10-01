//! Dataset metadata derivation.
//!
//! Converts cached Parquet/Arrow metadata into the stable public `DatasetInfo`
//! returned to the browser. No remote reads occur in this module.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use parquet::{
    arrow::arrow_reader::ArrowReaderMetadata, basic::LogicalType, file::metadata::ParquetMetaData,
};
use serde::Deserialize;

use crate::model::{
    Capabilities, ColumnInfo, DatasetInfo, DatasetLayoutSummary, GeoColumnInfo, GeoParquetInfo,
};

pub(super) fn build_dataset_info(
    dataset_id: &str,
    name: Option<String>,
    uri: &str,
    reader_metadata: &ArrowReaderMetadata,
) -> Result<DatasetInfo> {
    let metadata = reader_metadata.metadata().as_ref();
    let schema_descr = metadata.file_metadata().schema_descr();
    let geo_metadata = parse_geo_metadata(metadata)?;

    let columns: Vec<ColumnInfo> = reader_metadata
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let physical = schema_descr
                .columns()
                .iter()
                .find(|column| {
                    column.path().parts().first().map(String::as_str) == Some(field.name().as_str())
                })
                .map(|column| format!("{:?}", column.physical_type()));

            ColumnInfo {
                name: field.name().to_string(),
                arrow_type: format!("{:?}", field.data_type()),
                parquet_physical_type: physical,
                nullable: field.is_nullable(),
            }
        })
        .collect();

    let mut geo_columns = Vec::new();
    for (leaf_index, column) in schema_descr.columns().iter().enumerate() {
        let (logical_type, crs, edge_interpolation) = match column.logical_type_ref() {
            Some(LogicalType::Geometry(geometry)) => (
                "GEOMETRY".to_string(),
                normalize_parquet_crs(geometry.crs.as_deref()),
                Some("PLANAR".to_string()),
            ),
            Some(LogicalType::Geography(geography)) => (
                "GEOGRAPHY".to_string(),
                normalize_parquet_crs(geography.crs.as_deref()),
                geography.algorithm().map(|value| format!("{value:?}")),
            ),
            _ => continue,
        };

        let column_name = column.path().string();
        let column_metadata = geo_metadata
            .as_ref()
            .and_then(|metadata| metadata.columns.get(&column_name));
        let mut row_groups_with_bbox = 0usize;
        let mut stats_extent: Option<[f64; 4]> = None;
        for row_group in metadata.row_groups() {
            if let Some(bbox) = row_group
                .column(leaf_index)
                .geo_statistics()
                .and_then(|stats| stats.bounding_box())
            {
                row_groups_with_bbox += 1;
                let candidate = [
                    bbox.get_xmin(),
                    bbox.get_ymin(),
                    bbox.get_xmax(),
                    bbox.get_ymax(),
                ];
                stats_extent = Some(match stats_extent {
                    None => candidate,
                    Some(current) => [
                        current[0].min(candidate[0]),
                        current[1].min(candidate[1]),
                        current[2].max(candidate[2]),
                        current[3].max(candidate[3]),
                    ],
                });
            }
        }

        let metadata_bbox = column_metadata.and_then(|column| column.bbox.clone());
        let dataset_bbox = stats_extent
            .or_else(|| metadata_bbox.as_deref().and_then(metadata_bbox_xy))
            .map(Vec::from);
        let is_primary = geo_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.primary_column == column_name);

        geo_columns.push(GeoColumnInfo {
            name: column_name,
            logical_type,
            crs,
            edge_interpolation,
            is_primary,
            geometry_types: column_metadata
                .map(|column| column.geometry_types.clone())
                .unwrap_or_default(),
            orientation: column_metadata.and_then(|column| column.orientation.clone()),
            epoch: column_metadata.and_then(|column| column.epoch),
            metadata_bbox,
            row_groups_with_bbox,
            row_groups_total: metadata.num_row_groups(),
            dataset_bbox,
        });
    }

    // GeoParquet 1.x stores WKB in ordinary BYTE_ARRAY/Binary columns and
    // declares their spatial meaning only in the file-level `geo` metadata.
    // Treat these as renderable geo columns as well, while keeping v2
    // conformance checks separate below.
    if let Some(metadata_geo) = geo_metadata.as_ref() {
        let existing = geo_columns
            .iter()
            .map(|column| column.name.clone())
            .collect::<HashSet<_>>();

        for (column_name, column_metadata) in &metadata_geo.columns {
            if existing.contains(column_name.as_str()) {
                continue;
            }
            if !column_metadata.encoding.eq_ignore_ascii_case("WKB") {
                continue;
            }

            let exists_in_schema = reader_metadata
                .schema()
                .fields()
                .iter()
                .any(|field| field.name() == column_name);
            if !exists_in_schema {
                continue;
            }

            let metadata_bbox = column_metadata.bbox.clone();
            let dataset_bbox = metadata_bbox
                .as_deref()
                .and_then(metadata_bbox_xy)
                .map(Vec::from);

            geo_columns.push(GeoColumnInfo {
                name: column_name.clone(),
                logical_type: "WKB".to_string(),
                crs: column_metadata.normalized_crs(),
                edge_interpolation: Some(
                    column_metadata
                        .edges
                        .clone()
                        .unwrap_or_else(|| "planar".to_string())
                        .to_ascii_uppercase(),
                ),
                is_primary: metadata_geo.primary_column == *column_name,
                geometry_types: column_metadata.geometry_types.clone(),
                orientation: column_metadata.orientation.clone(),
                epoch: column_metadata.epoch,
                metadata_bbox,
                row_groups_with_bbox: 0,
                row_groups_total: metadata.num_row_groups(),
                dataset_bbox,
            });
        }
    }

    // GLOBE_COMPAT_VERSION: 2026-08-27-wkb-primary-v3
    // GeoParquet 1.x commonly stores the primary geometry as plain
    // BYTE_ARRAY/Binary WKB. If file-level geo metadata names a primary
    // column and that column is binary, expose it as a renderable WKB geo
    // column even when it has no native Parquet GEOMETRY/GEOGRAPHY type.
    if let Some(metadata_geo) = geo_metadata.as_ref() {
        let primary = metadata_geo.primary_column.as_str();
        let already_known = geo_columns.iter().any(|column| column.name == primary);

        if !already_known {
            if let Some(schema_column) = columns.iter().find(|column| column.name == primary) {
                let binary = matches!(
                    schema_column.arrow_type.as_str(),
                    "Binary" | "LargeBinary" | "BinaryView"
                ) || schema_column.parquet_physical_type.as_deref()
                    == Some("BYTE_ARRAY");

                if binary {
                    let column_metadata = metadata_geo.columns.get(primary);
                    let metadata_bbox = column_metadata.and_then(|column| column.bbox.clone());
                    let dataset_bbox = metadata_bbox
                        .as_deref()
                        .and_then(metadata_bbox_xy)
                        .map(Vec::from);

                    geo_columns.push(GeoColumnInfo {
                        name: primary.to_string(),
                        logical_type: "WKB".to_string(),
                        crs: column_metadata
                            .map(|column| column.normalized_crs())
                            .unwrap_or_else(|| "OGC:CRS84".to_string()),
                        edge_interpolation: Some(
                            column_metadata
                                .and_then(|column| column.edges.clone())
                                .unwrap_or_else(|| "planar".to_string())
                                .to_ascii_uppercase(),
                        ),
                        is_primary: true,
                        geometry_types: column_metadata
                            .map(|column| column.geometry_types.clone())
                            .unwrap_or_default(),
                        orientation: column_metadata.and_then(|column| column.orientation.clone()),
                        epoch: column_metadata.and_then(|column| column.epoch),
                        metadata_bbox,
                        row_groups_with_bbox: 0,
                        row_groups_total: metadata.num_row_groups(),
                        dataset_bbox,
                    });
                }
            }
        }
    }

    let geo_parquet = build_geo_parquet_info(geo_metadata.as_ref(), &geo_columns);
    let layout = build_layout_summary(metadata);

    Ok(DatasetInfo {
        dataset_id: dataset_id.to_string(),
        name,
        uri: uri.to_string(),
        num_rows: metadata.file_metadata().num_rows(),
        num_row_groups: metadata.num_row_groups(),
        columns,
        geo_columns,
        geo_parquet,
        layout,
        capabilities: Capabilities::default(),
    })
}

fn build_layout_summary(metadata: &ParquetMetaData) -> DatasetLayoutSummary {
    let mut compressed_data_bytes = 0u64;
    let mut uncompressed_data_bytes = 0u64;
    let mut largest_row_group_compressed_bytes = 0u64;
    let mut chunks_with_statistics = 0usize;
    let mut chunks_total = 0usize;
    let mut column_index_declared = false;
    let mut offset_index_declared = false;

    for row_group in metadata.row_groups() {
        let compressed = u64::try_from(row_group.compressed_size()).unwrap_or(0);
        let uncompressed = u64::try_from(row_group.total_byte_size()).unwrap_or(0);
        compressed_data_bytes = compressed_data_bytes.saturating_add(compressed);
        uncompressed_data_bytes = uncompressed_data_bytes.saturating_add(uncompressed);
        largest_row_group_compressed_bytes = largest_row_group_compressed_bytes.max(compressed);

        for column in row_group.columns() {
            chunks_total += 1;
            chunks_with_statistics += if column.statistics().is_some() { 1 } else { 0 };
            column_index_declared |= column.column_index_offset().is_some();
            offset_index_declared |= column.offset_index_offset().is_some();
        }
    }

    let row_groups = metadata.num_row_groups();
    let rows = metadata.file_metadata().num_rows();
    DatasetLayoutSummary {
        compressed_data_bytes,
        uncompressed_data_bytes,
        average_row_group_rows: (row_groups > 0).then(|| rows as f64 / row_groups as f64),
        average_row_group_compressed_bytes: (row_groups > 0)
            .then(|| compressed_data_bytes as f64 / row_groups as f64),
        largest_row_group_compressed_bytes,
        statistics_coverage: (chunks_total > 0)
            .then(|| chunks_with_statistics as f64 / chunks_total as f64),
        column_index_declared,
        offset_index_declared,
    }
}

#[derive(Debug, Deserialize)]
struct RawGeoMetadata {
    version: String,
    primary_column: String,
    columns: HashMap<String, RawGeoColumnMetadata>,
}

#[derive(Debug, Deserialize)]
struct RawGeoColumnMetadata {
    encoding: String,
    geometry_types: Vec<String>,
    #[serde(default)]
    edges: Option<String>,
    #[serde(default)]
    orientation: Option<String>,
    #[serde(default)]
    bbox: Option<Vec<f64>>,
    #[serde(default)]
    epoch: Option<f64>,
    /// GeoParquet 1.x: CRS is absent (default OGC:CRS84), a CRS string, or a
    /// PROJJSON object. Raw value kept so both shapes can be normalized.
    #[serde(default)]
    crs: Option<serde_json::Value>,
}

impl RawGeoColumnMetadata {
    fn normalized_crs(&self) -> String {
        match &self.crs {
            None | Some(serde_json::Value::Null) => "OGC:CRS84".to_string(),
            Some(serde_json::Value::String(value)) => normalize_parquet_crs(Some(value)),
            Some(serde_json::Value::Object(projjson)) => {
                let id = projjson.get("id").and_then(|id| id.as_object());
                let authority = id
                    .and_then(|id| id.get("authority"))
                    .and_then(|value| value.as_str());
                let code = id
                    .and_then(|id| id.get("code"))
                    .and_then(|value| value.as_i64());
                match (authority, code) {
                    (Some(authority), Some(code)) => format!("{authority}:{code}"),
                    _ => projjson
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("OGC:CRS84")
                        .to_string(),
                }
            }
            Some(_) => "OGC:CRS84".to_string(),
        }
    }
}

fn parse_geo_metadata(metadata: &ParquetMetaData) -> Result<Option<RawGeoMetadata>> {
    let Some(key_values) = metadata.file_metadata().key_value_metadata() else {
        return Ok(None);
    };
    let Some(value) = key_values
        .iter()
        .rev()
        .find(|entry| entry.key == "geo")
        .and_then(|entry| entry.value.as_deref())
    else {
        return Ok(None);
    };

    let parsed = serde_json::from_str::<RawGeoMetadata>(value)
        .context("invalid GeoParquet 'geo' file metadata JSON")?;
    Ok(Some(parsed))
}

fn build_geo_parquet_info(
    metadata: Option<&RawGeoMetadata>,
    geo_columns: &[GeoColumnInfo],
) -> Option<GeoParquetInfo> {
    let metadata = metadata?;
    let native_names = geo_columns
        .iter()
        .filter(|column| matches!(column.logical_type.as_str(), "GEOMETRY" | "GEOGRAPHY"))
        .map(|column| column.name.as_str())
        .collect::<HashSet<_>>();
    let mut warnings = Vec::new();

    if metadata.version != "2.0.0" {
        warnings.push(format!(
            "geo metadata version is {}, not 2.0.0",
            metadata.version
        ));
    }
    if !metadata.columns.contains_key(&metadata.primary_column) {
        warnings.push("primary_column is not present in geo metadata columns".to_string());
    }
    if !native_names.contains(metadata.primary_column.as_str()) {
        warnings.push("primary_column is not a native GEOMETRY/GEOGRAPHY column".to_string());
    }
    for (name, column) in &metadata.columns {
        let native = geo_columns.iter().find(|candidate| candidate.name == *name);
        if native.is_none() {
            warnings.push(format!(
                "geo metadata column '{name}' is not a native GEOMETRY/GEOGRAPHY column"
            ));
        }
        if column.encoding != "WKB" {
            warnings.push(format!(
                "geo metadata column '{name}' uses unsupported encoding '{}'",
                column.encoding
            ));
        }
        if let Some(orientation) = column.orientation.as_deref() {
            if orientation != "counterclockwise" {
                warnings.push(format!(
                    "geo metadata column '{name}' has invalid orientation '{orientation}'"
                ));
            }
        }
        if let Some(bbox) = column.bbox.as_deref() {
            if !matches!(bbox.len(), 4 | 6 | 8) || bbox.iter().any(|value| !value.is_finite()) {
                warnings.push(format!("geo metadata column '{name}' has an invalid bbox"));
            }
        }
        if column.epoch.is_some_and(|epoch| !epoch.is_finite()) {
            warnings.push(format!(
                "geo metadata column '{name}' has a non-finite coordinate epoch"
            ));
        }

        if let Some(native) = native {
            let native_edges = native
                .edge_interpolation
                .as_deref()
                .unwrap_or("PLANAR")
                .to_ascii_lowercase();
            let metadata_edges = column.edges.as_deref().unwrap_or("planar");
            if native_edges != metadata_edges {
                warnings.push(format!(
                    "geo metadata column '{name}' edges '{metadata_edges}' do not match native Parquet edge interpolation '{native_edges}'"
                ));
            }
        }
    }
    for name in &native_names {
        if !metadata.columns.contains_key(*name) {
            warnings.push(format!(
                "native geospatial column '{name}' is missing from geo metadata"
            ));
        }
    }

    Some(GeoParquetInfo {
        version: metadata.version.clone(),
        primary_column: metadata.primary_column.clone(),
        v2_metadata_checks_passed: metadata.version == "2.0.0" && warnings.is_empty(),
        warnings,
    })
}

fn normalize_parquet_crs(crs: Option<&str>) -> String {
    match crs {
        None => "OGC:CRS84".to_string(),
        Some("srid:0") => "UNDEFINED".to_string(),
        Some(value) => value.to_string(),
    }
}

fn metadata_bbox_xy(values: &[f64]) -> Option<[f64; 4]> {
    match values {
        [xmin, ymin, xmax, ymax] => Some([*xmin, *ymin, *xmax, *ymax]),
        [xmin, ymin, _, xmax, ymax, _] => Some([*xmin, *ymin, *xmax, *ymax]),
        [xmin, ymin, _, _, xmax, ymax, _, _] => Some([*xmin, *ymin, *xmax, *ymax]),
        _ => None,
    }
}
