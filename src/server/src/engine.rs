mod ipc;
mod source;

use std::{collections::{HashMap, HashSet}, fs::File, sync::Arc};

use anyhow::{Context, Result, anyhow, bail};
use parking_lot::RwLock;
use serde::Deserialize;
use parquet::{
    arrow::{ProjectionMask, arrow_writer::ArrowWriter},
    basic::LogicalType,
    file::metadata::ParquetMetaData,
};
use tempfile::{TempPath, NamedTempFile};
use uuid::Uuid;

use crate::model::{
    Capabilities, ColumnInfo, DatasetEntry, DatasetInfo, ExportFormat, ExportRequest,
    GeoColumnInfo, GeoParquetInfo, PageRequest, SpatialRequest,
};
use ipc::{IpcByteStream, spawn_ipc_stream};
use source::{ReaderBuilder, builder_for_uri};

pub struct ExportArtifact {
    pub path: TempPath,
    pub content_type: &'static str,
    pub filename: &'static str,
}

pub struct CoreEngine {
    datasets: RwLock<HashMap<String, DatasetEntry>>,
    batch_size: usize,
    ipc_channel_capacity: usize,
    max_open_datasets: usize,
}

impl CoreEngine {
    pub fn new(batch_size: usize, max_open_datasets: usize) -> Self {
        Self {
            datasets: RwLock::new(HashMap::new()),
            batch_size,
            ipc_channel_capacity: 4,
            max_open_datasets,
        }
    }

    pub async fn open_dataset(&self, uri: &str, name: Option<String>) -> Result<DatasetInfo> {
        if self.datasets.read().len() >= self.max_open_datasets {
            bail!("maximum number of open dataset handles reached");
        }

        let builder = builder_for_uri(uri).await?;
        let dataset_id = Uuid::new_v4().to_string();
        let info = build_dataset_info(&dataset_id, name, uri, &builder)?;

        let mut datasets = self.datasets.write();
        if datasets.len() >= self.max_open_datasets {
            bail!("maximum number of open dataset handles reached");
        }
        datasets.insert(dataset_id, DatasetEntry { info: info.clone() });
        Ok(info)
    }

    pub fn metadata(&self, dataset_id: &str) -> Result<DatasetInfo> {
        self.datasets
            .read()
            .get(dataset_id)
            .map(|entry| entry.info.clone())
            .ok_or_else(|| anyhow!("dataset not found: {dataset_id}"))
    }

    pub fn close_dataset(&self, dataset_id: &str) -> Result<()> {
        if self.datasets.write().remove(dataset_id).is_none() {
            bail!("dataset not found: {dataset_id}");
        }
        Ok(())
    }

    pub async fn page_stream(&self, dataset_id: &str, req: PageRequest) -> Result<IpcByteStream> {
        let entry = self.entry(dataset_id)?;
        let builder = builder_for_uri(&entry.info.uri).await?;
        let builder = apply_projection(builder, req.columns.as_deref())?
            .with_offset(req.offset)
            .with_limit(req.limit)
            .with_batch_size(self.batch_size);
        let stream = builder.build().context("could not build Parquet page reader")?;
        let schema = Arc::clone(stream.schema());
        Ok(spawn_ipc_stream(stream, schema, self.ipc_channel_capacity))
    }

    pub async fn spatial_stream(
        &self,
        dataset_id: &str,
        mut req: SpatialRequest,
    ) -> Result<IpcByteStream> {
        validate_bbox(req.bbox)?;
        let entry = self.entry(dataset_id)?;
        let geometry = choose_geometry_column(&entry.info, req.geometry_column.as_deref())?;
        let requested = req.columns.take().map(|mut columns| {
            if !columns.iter().any(|name| name == &geometry) {
                columns.push(geometry.clone());
            }
            columns
        });

        let builder = builder_for_uri(&entry.info.uri).await?;
        let row_groups = intersecting_row_groups(builder.metadata(), &geometry, req.bbox)?;
        let builder = apply_projection(builder, requested.as_deref())?
            .with_row_groups(row_groups)
            .with_limit(req.max_features)
            .with_batch_size(self.batch_size);
        let stream = builder
            .build()
            .context("could not build spatial Parquet reader")?;
        let schema = Arc::clone(stream.schema());
        Ok(spawn_ipc_stream(stream, schema, self.ipc_channel_capacity))
    }

    pub async fn export(&self, dataset_id: &str, req: ExportRequest) -> Result<ExportArtifact> {
        if req.spatial_bbox.is_some() {
            bail!(
                "spatial export is disabled until exact feature-level GeoArrow/GeoRust filtering is implemented"
            );
        }

        let entry = self.entry(dataset_id)?;
        let mut builder = builder_for_uri(&entry.info.uri).await?;
        builder = apply_projection(builder, req.columns.as_deref())?
            .with_offset(req.offset)
            .with_batch_size(self.batch_size);
        if let Some(limit) = req.limit {
            builder = builder.with_limit(limit);
        }

        let mut stream = builder.build().context("could not build export reader")?;
        let schema = Arc::clone(stream.schema());
        let temp = NamedTempFile::new().context("could not create temporary export file")?;
        let path = temp.into_temp_path();
        let output = File::create(&path).context("could not open temporary export file")?;

        match req.format {
            ExportFormat::Arrow => {
                let mut writer = arrow_ipc::writer::StreamWriter::try_new(output, schema.as_ref())
                    .context("could not create Arrow IPC export writer")?;
                use futures::TryStreamExt;
                while let Some(batch) = stream.try_next().await.context("export read failed")? {
                    writer.write(&batch).context("Arrow export write failed")?;
                }
                writer.finish().context("Arrow export finalize failed")?;
                Ok(ExportArtifact {
                    path,
                    content_type: "application/vnd.apache.arrow.stream",
                    filename: "subset.arrow",
                })
            }
            ExportFormat::Parquet => {
                let mut writer = ArrowWriter::try_new(output, Arc::clone(&schema), None)
                    .context("could not create Parquet export writer")?;
                use futures::TryStreamExt;
                while let Some(batch) = stream.try_next().await.context("export read failed")? {
                    writer.write(&batch).context("Parquet export write failed")?;
                }
                writer.close().context("Parquet export finalize failed")?;
                Ok(ExportArtifact {
                    path,
                    content_type: "application/vnd.apache.parquet",
                    filename: "subset.parquet",
                })
            }
        }
    }

    fn entry(&self, dataset_id: &str) -> Result<DatasetEntry> {
        self.datasets
            .read()
            .get(dataset_id)
            .cloned()
            .ok_or_else(|| anyhow!("dataset not found: {dataset_id}"))
    }
}

fn apply_projection(mut builder: ReaderBuilder, columns: Option<&[String]>) -> Result<ReaderBuilder> {
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

fn choose_geometry_column(info: &DatasetInfo, requested: Option<&str>) -> Result<String> {
    if let Some(requested) = requested {
        if info.geo_columns.iter().any(|column| column.name == requested) {
            return Ok(requested.to_string());
        }
        bail!("not a GeoParquet 2 geometry/geography column: {requested}");
    }

    if let Some(primary) = info
        .geo_parquet
        .as_ref()
        .map(|metadata| metadata.primary_column.as_str())
    {
        if info.geo_columns.iter().any(|column| column.name == primary) {
            return Ok(primary.to_string());
        }
    }

    info.geo_columns
        .first()
        .map(|column| column.name.clone())
        .ok_or_else(|| anyhow!("dataset has no native GEOMETRY/GEOGRAPHY column"))
}

fn validate_bbox(bbox: [f64; 4]) -> Result<()> {
    if bbox.iter().any(|value| !value.is_finite()) {
        bail!("bbox values must be finite numbers");
    }
    if bbox[0] > bbox[2] {
        bail!("bbox xmin must be <= xmax");
    }
    if bbox[1] > bbox[3] {
        bail!("bbox ymin must be <= ymax");
    }
    Ok(())
}

fn intersecting_row_groups(
    metadata: &ParquetMetaData,
    geometry_column: &str,
    query: [f64; 4],
) -> Result<Vec<usize>> {
    let schema = metadata.file_metadata().schema_descr();
    let leaf_index = schema
        .columns()
        .iter()
        .position(|column| column.path().string() == geometry_column)
        .ok_or_else(|| anyhow!("geometry column is not a leaf column: {geometry_column}"))?;

    let mut row_groups = Vec::new();
    for (index, row_group) in metadata.row_groups().iter().enumerate() {
        let column = row_group.column(leaf_index);
        let include = match column.geo_statistics().and_then(|stats| stats.bounding_box()) {
            Some(bbox) => bbox_intersects(
                [bbox.get_xmin(), bbox.get_ymin(), bbox.get_xmax(), bbox.get_ymax()],
                query,
            ),
            // Missing statistics must be retained for conservative correctness.
            None => true,
        };
        if include {
            row_groups.push(index);
        }
    }
    Ok(row_groups)
}

fn bbox_intersects(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

fn build_dataset_info(
    dataset_id: &str,
    name: Option<String>,
    uri: &str,
    builder: &ReaderBuilder,
) -> Result<DatasetInfo> {
    let metadata = builder.metadata();
    let schema_descr = metadata.file_metadata().schema_descr();
    let geo_metadata = parse_geo_metadata(metadata)?;

    let columns = builder
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let physical = schema_descr
                .columns()
                .iter()
                .find(|column| {
                    column.path().parts().first().map(String::as_str)
                        == Some(field.name().as_str())
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

    let geo_parquet = build_geo_parquet_info(geo_metadata.as_ref(), &geo_columns);

    Ok(DatasetInfo {
        dataset_id: dataset_id.to_string(),
        name,
        uri: uri.to_string(),
        num_rows: metadata.file_metadata().num_rows(),
        num_row_groups: metadata.num_row_groups(),
        columns,
        geo_columns,
        geo_parquet,
        capabilities: Capabilities::default(),
    })
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
                warnings.push(format!(
                    "geo metadata column '{name}' has an invalid bbox"
                ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_intersection_includes_touching_edges() {
        assert!(bbox_intersects([0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 2.0, 2.0]));
        assert!(!bbox_intersects([0.0, 0.0, 1.0, 1.0], [1.1, 1.1, 2.0, 2.0]));
    }

    #[test]
    fn invalid_bbox_is_rejected() {
        assert!(validate_bbox([10.0, 0.0, 1.0, 2.0]).is_err());
        assert!(validate_bbox([0.0, f64::NAN, 1.0, 2.0]).is_err());
    }
}
