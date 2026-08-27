use std::{io::{self, Write}, sync::{Arc, Mutex}};

use arrow_array::RecordBatch;
use arrow_ipc::writer::StreamWriter;
use arrow_schema::SchemaRef;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use parquet::errors::ParquetError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

pub type IpcByteStream = ReceiverStream<Result<Bytes, io::Error>>;

#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn take(&self) -> Vec<u8> { std::mem::take(&mut *self.0.lock().expect("buffer poisoned")) }
}

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().map_err(|_| io::Error::other("IPC buffer poisoned"))?.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

pub fn spawn_ipc_stream<S>(mut input: S, schema: SchemaRef, capacity: usize) -> IpcByteStream
where
    S: Stream<Item = Result<RecordBatch, ParquetError>> + Send + Unpin + 'static,
{
    let (tx, rx) = mpsc::channel(capacity.max(1));
    tokio::spawn(async move {
        let sink = SharedBuffer::default();
        let mut writer = match StreamWriter::try_new(sink.clone(), schema.as_ref()) {
            Ok(writer) => writer,
            Err(error) => { let _ = tx.send(Err(io::Error::other(error.to_string()))).await; return; }
        };
        if !send_pending(&tx, &sink).await { return; }

        while let Some(item) = input.next().await {
            let batch = match item {
                Ok(batch) => batch,
                Err(error) => { let _ = tx.send(Err(io::Error::other(error.to_string()))).await; return; }
            };
            if let Err(error) = writer.write(&batch) {
                let _ = tx.send(Err(io::Error::other(error.to_string()))).await;
                return;
            }
            if !send_pending(&tx, &sink).await { return; }
        }
        if let Err(error) = writer.finish() {
            let _ = tx.send(Err(io::Error::other(error.to_string()))).await;
            return;
        }
        let _ = send_pending(&tx, &sink).await;
    });
    ReceiverStream::new(rx)
}

async fn send_pending(tx: &mpsc::Sender<Result<Bytes, io::Error>>, sink: &SharedBuffer) -> bool {
    let bytes = sink.take();
    if bytes.is_empty() { return true; }
    tx.send(Ok(Bytes::from(bytes))).await.is_ok()
}
