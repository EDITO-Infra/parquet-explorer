//! Spatial query orchestration and conservative row-group bbox pruning.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use parquet::file::metadata::ParquetMetaData;

use crate::model::{DatasetInfo, SpatialRequest};

use super::{
    CoreEngine,
    ipc::{IpcByteStream, spawn_ipc_stream},
    query::{apply_query_filters, apply_query_projection, prune_query_row_groups},
    source::ReadMetrics,
};

impl CoreEngine {
    /// Stream conservative spatial candidates selected using row-group bboxes.
    /// Exact feature-level geometry filtering is intentionally not claimed here.
    pub async fn spatial_stream(
        &self,
        dataset_id: &str,
        mut req: SpatialRequest,
    ) -> Result<IpcByteStream> {
        let trace = self
            .traces
            .start(req.trace_id.as_deref(), "spatial", "Preparing spatial query");
        validate_bbox(req.bbox)?;
        let dataset = self.dataset(dataset_id)?;
        if let Some(trace) = &trace {
            trace.event("dataset_found", "Dataset handle found");
            trace.event("metadata_cache", "Reusing cached Parquet metadata");
        }
        let geometry = choose_geometry_column(&dataset.info, req.geometry_column.as_deref())?;
        let requested = req.columns.take().map(|mut columns| {
            if !columns.iter().any(|name| name == &geometry) {
                columns.push(geometry.clone());
            }
            columns
        });

        let metadata = dataset.reader_metadata.metadata();
        let total_row_groups = metadata.num_row_groups();
        let spatial_row_groups = intersecting_row_groups(metadata.as_ref(), &geometry, req.bbox)?;
        let attribute_pruning = prune_query_row_groups(&dataset, &req.filters)?;
        let attribute_set = attribute_pruning
            .row_groups
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        let row_groups = spatial_row_groups
            .into_iter()
            .filter(|index| attribute_set.contains(index))
            .collect::<Vec<_>>();
        if let Some(trace) = &trace {
            trace.event_with_detail(
                "prune_row_groups",
                "Pruning row groups with spatial and attribute statistics",
                format!(
                    "{} of {} row groups selected · {} attribute filters pruned row groups",
                    row_groups.len(),
                    total_row_groups,
                    attribute_pruning.filters_pruning_row_groups
                ),
            );
        }

        let read_metrics = ReadMetrics::default();
        let builder = dataset.reader_builder(read_metrics.clone());
        let builder = apply_query_filters(builder, &req.filters)?;
        let builder = apply_query_projection(builder, requested.as_deref())?
            .with_row_groups(row_groups)
            .with_limit(req.max_features)
            .with_batch_size(self.batch_size);
        let stream = builder
            .build()
            .context("could not build spatial Parquet reader")?;
        let schema = Arc::clone(stream.schema());
        Ok(spawn_ipc_stream(
            stream,
            schema,
            self.ipc_channel_capacity,
            trace,
            Some(read_metrics),
        ))
    }
}

fn choose_geometry_column(info: &DatasetInfo, requested: Option<&str>) -> Result<String> {
    let metadata_primary = info
        .geo_parquet
        .as_ref()
        .map(|metadata| metadata.primary_column.as_str());

    if let Some(requested) = requested {
        if info
            .geo_columns
            .iter()
            .any(|column| column.name == requested)
            || (metadata_primary == Some(requested)
                && info.columns.iter().any(|column| column.name == requested))
        {
            return Ok(requested.to_string());
        }
        bail!("not a recognized geospatial column: {requested}");
    }

    if let Some(primary) = metadata_primary {
        if info.geo_columns.iter().any(|column| column.name == primary)
            || info.columns.iter().any(|column| column.name == primary)
        {
            return Ok(primary.to_string());
        }
    }

    info.geo_columns
        .first()
        .map(|column| column.name.clone())
        .ok_or_else(|| anyhow!("dataset has no recognized geospatial column"))
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
        let include = match column
            .geo_statistics()
            .and_then(|stats| stats.bounding_box())
        {
            Some(bbox) => bbox_intersects(
                [
                    bbox.get_xmin(),
                    bbox.get_ymin(),
                    bbox.get_xmax(),
                    bbox.get_ymax(),
                ],
                query,
            ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_intersection_includes_touching_edges() {
        assert!(bbox_intersects(
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 2.0, 2.0]
        ));
        assert!(!bbox_intersects(
            [0.0, 0.0, 1.0, 1.0],
            [1.1, 1.1, 2.0, 2.0]
        ));
    }

    #[test]
    fn invalid_bbox_is_rejected() {
        assert!(validate_bbox([10.0, 0.0, 1.0, 2.0]).is_err());
        assert!(validate_bbox([0.0, f64::NAN, 1.0, 2.0]).is_err());
    }
}
