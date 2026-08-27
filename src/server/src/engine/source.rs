use std::{ops::Range, sync::Arc};

use anyhow::{Context, Result};
use bytes::Bytes;
use futures::future::BoxFuture;
use futures::{FutureExt, TryFutureExt};
use object_store::path::Path;
use object_store::{GetOptions, GetRange, ObjectStore, ObjectStoreExt};
use parquet::arrow::ParquetRecordBatchStreamBuilder;
use parquet::arrow::arrow_reader::ArrowReaderOptions;
use parquet::arrow::async_reader::{AsyncFileReader, MetadataSuffixFetch};
use parquet::errors::{ParquetError, Result as ParquetResult};
use parquet::file::metadata::{ParquetMetaData, ParquetMetaDataReader};
use url::Url;

/// Minimal `object_store` adapter for Parquet-RS' current async reader API.
///
/// Parquet-RS 59.2 deprecated its built-in `ParquetObjectReader` integration in
/// favor of user-defined `AsyncFileReader` implementations. Keeping this
/// adapter here lets the server use HTTP/S3/Azure/GCS/local range reads without
/// coupling the engine to Parquet's deprecated object_store feature.
#[derive(Clone)]
pub struct ObjectStoreReader {
    store: Arc<dyn ObjectStore>,
    path: Path,
}

impl ObjectStoreReader {
    fn new(store: Arc<dyn ObjectStore>, path: Path) -> Self {
        Self { store, path }
    }
}

fn to_parquet_err(error: object_store::Error) -> ParquetError {
    ParquetError::External(Box::new(error))
}

impl AsyncFileReader for ObjectStoreReader {
    fn get_bytes(&mut self, range: Range<u64>) -> BoxFuture<'_, ParquetResult<Bytes>> {
        self.store
            .get_range(&self.path, range)
            .map_err(to_parquet_err)
            .boxed()
    }

    fn get_byte_ranges(
        &mut self,
        ranges: Vec<Range<u64>>,
    ) -> BoxFuture<'_, ParquetResult<Vec<Bytes>>> {
        async move {
            self.store
                .get_ranges(&self.path, &ranges)
                .await
                .map_err(to_parquet_err)
        }
        .boxed()
    }

    fn get_metadata<'a>(
        &'a mut self,
        options: Option<&'a ArrowReaderOptions>,
    ) -> BoxFuture<'a, ParquetResult<Arc<ParquetMetaData>>> {
        async move {
            let metadata = ParquetMetaDataReader::new()
                .with_arrow_reader_options(options)
                .load_via_suffix_and_finish(self)
                .await?;
            Ok(Arc::new(metadata))
        }
        .boxed()
    }
}

/// Allow the Parquet metadata reader to fetch the footer with a suffix request,
/// avoiding a preliminary whole-object read (and, where supported, an extra
/// HEAD request just to discover the file size).
impl MetadataSuffixFetch for &mut ObjectStoreReader {
    fn fetch_suffix(&mut self, suffix: usize) -> BoxFuture<'_, ParquetResult<Bytes>> {
        let options = GetOptions {
            range: Some(GetRange::Suffix(suffix as u64)),
            ..Default::default()
        };

        async move {
            let response = self
                .store
                .get_opts(&self.path, options)
                .await
                .map_err(to_parquet_err)?;
            response.bytes().await.map_err(to_parquet_err)
        }
        .boxed()
    }
}

pub type ReaderBuilder = ParquetRecordBatchStreamBuilder<ObjectStoreReader>;

pub async fn builder_for_uri(uri: &str) -> Result<ReaderBuilder> {
    let url = Url::parse(uri).with_context(|| format!("invalid source URI: {uri}"))?;
    let (store, path) = object_store::parse_url(&url)
        .with_context(|| format!("unsupported or invalid object-store URI: {uri}"))?;
    let store: Arc<dyn ObjectStore> = Arc::from(store);
    let reader = ObjectStoreReader::new(store, path);

    ParquetRecordBatchStreamBuilder::new(reader)
        .await
        .with_context(|| format!("could not open Parquet metadata: {uri}"))
}
