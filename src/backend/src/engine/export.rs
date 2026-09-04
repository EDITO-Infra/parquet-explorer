//! Read-only subset export.

use std::{fs::File, sync::Arc};

use anyhow::{Context, Result, bail};
use parquet::arrow::arrow_writer::ArrowWriter;
use tempfile::{NamedTempFile, TempPath};

use crate::model::{ExportFormat, ExportRequest};

use super::{
    CoreEngine,
    query::{apply_query_filters, apply_query_projection, prune_query_row_groups},
    source::ReadMetrics,
};

/// Temporary file plus HTTP metadata returned by an export operation.
pub struct ExportArtifact {
    pub path: TempPath,
    pub content_type: &'static str,
    pub filename: &'static str,
}

impl CoreEngine {
    /// Materialize a read-only subset into a temporary Arrow or Parquet export.
    pub async fn export(&self, dataset_id: &str, req: ExportRequest) -> Result<ExportArtifact> {
        if req.spatial_bbox.is_some() {
            bail!(
                "spatial export is disabled until exact feature-level GeoArrow/GeoRust filtering is implemented"
            );
        }

        let dataset = self.dataset(dataset_id)?;
        let metrics = ReadMetrics::default();
        let mut builder = dataset.reader_builder(metrics);
        if !req.filters.is_empty() {
            let pruning = prune_query_row_groups(&dataset, &req.filters)?;
            builder = builder.with_row_groups(pruning.row_groups);
        }
        builder = apply_query_filters(builder, &req.filters)?;
        builder = apply_query_projection(builder, req.columns.as_deref())?
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
                    writer
                        .write(&batch)
                        .context("Parquet export write failed")?;
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
}
