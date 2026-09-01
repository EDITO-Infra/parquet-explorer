//! Streaming bridge from Parquet `RecordBatch` output to Arrow IPC bytes.
//!
//! The stream is encoded incrementally into an in-memory sink and forwarded
//! through a bounded Tokio channel. The bounded channel provides backpressure
//! while the trace records reader, storage, encoding, and queue timings.

use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use arrow_array::RecordBatch;
use arrow_ipc::writer::StreamWriter;
use arrow_schema::SchemaRef;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use parquet::errors::ParquetError;

use super::{
    source::ReadMetrics,
    trace::TraceReporter,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// Byte stream returned to Axum for an Arrow IPC streaming response.
pub type IpcByteStream = ReceiverStream<Result<Bytes, io::Error>>;

#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.0.lock().expect("buffer poisoned"))
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("IPC buffer poisoned"))?
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Spawn the asynchronous Parquet→Arrow IPC pump.
///
/// The task polls RecordBatches, records storage/reader timings, encodes each
/// batch into Arrow IPC, and forwards pending bytes through a bounded channel.
pub fn spawn_ipc_stream<S>(
    mut input: S,
    schema: SchemaRef,
    capacity: usize,
    trace: Option<TraceReporter>,
    read_metrics: Option<ReadMetrics>,
) -> IpcByteStream
where
    S: Stream<Item = Result<RecordBatch, ParquetError>> + Send + Unpin + 'static,
{
    let (tx, rx) = mpsc::channel(capacity.max(1));
    tokio::spawn(async move {
        if let Some(trace) = &trace {
            trace.event("read_data", "Reading Parquet row groups and column pages");
        }

        let sink = SharedBuffer::default();
        let writer_started = Instant::now();
        let mut writer = match StreamWriter::try_new(sink.clone(), schema.as_ref()) {
            Ok(writer) => writer,
            Err(error) => {
                let _ = tx.send(Err(io::Error::other(error.to_string()))).await;
                return;
            }
        };
        let mut ipc_encode_ms = millis(writer_started.elapsed());
        let mut response_queue_ms = 0u64;
        let mut ipc_bytes = 0u64;

        let Some(schema_send) = send_pending(&tx, &sink).await else {
            return;
        };
        response_queue_ms = response_queue_ms.saturating_add(schema_send.queue_ms);
        ipc_bytes = ipc_bytes.saturating_add(schema_send.bytes);

        let mut batch_count = 0usize;
        let mut row_count = 0usize;
        let read_started = Instant::now();
        let mut reader_wall_ms = 0u64;

        loop {
            let next_started = Instant::now();
            let item = input.next().await;
            let next_ms = millis(next_started.elapsed());
            reader_wall_ms = reader_wall_ms.saturating_add(next_ms);

            let Some(item) = item else {
                break;
            };
            let batch = match item {
                Ok(batch) => batch,
                Err(error) => {
                    let _ = tx.send(Err(io::Error::other(error.to_string()))).await;
                    return;
                }
            };

            batch_count += 1;
            row_count += batch.num_rows();

            if batch_count == 1 {
                let first_batch_wall_ms = millis(read_started.elapsed());
                let storage = read_metrics.as_ref().map(ReadMetrics::snapshot).unwrap_or_default();
                if let Some(trace) = &trace {
                    trace.event_with_duration(
                        "data_storage",
                        "Fetching Parquet column data from storage",
                        storage.io_ms,
                        format!(
                            "{} object-store call{} · {} range{} · {} requested · {} received",
                            storage.calls,
                            plural(storage.calls),
                            storage.ranges,
                            plural(storage.ranges),
                            format_bytes(storage.requested_bytes),
                            format_bytes(storage.returned_bytes),
                        ),
                    );
                    trace.event_with_duration(
                        "parquet_reader_work",
                        "Decompressing and decoding Parquet",
                        first_batch_wall_ms.saturating_sub(storage.io_ms),
                        "Derived: first-batch reader wall time minus measured object-store wait",
                    );
                    trace.event_with_detail(
                        "first_batch",
                        "First Arrow RecordBatch produced",
                        format!("{} rows · {} total reader wait", batch.num_rows(), format_duration(first_batch_wall_ms)),
                    );
                }
            }

            let encode_started = Instant::now();
            if let Err(error) = writer.write(&batch) {
                let _ = tx.send(Err(io::Error::other(error.to_string()))).await;
                return;
            }
            let encode_ms = millis(encode_started.elapsed());
            ipc_encode_ms = ipc_encode_ms.saturating_add(encode_ms);

            let Some(send) = send_pending(&tx, &sink).await else {
                return;
            };
            response_queue_ms = response_queue_ms.saturating_add(send.queue_ms);
            ipc_bytes = ipc_bytes.saturating_add(send.bytes);

            if batch_count == 1 {
                if let Some(trace) = &trace {
                    trace.event_with_duration(
                        "arrow_ipc_encode_first",
                        "Encoding first batch as Arrow IPC",
                        encode_ms,
                        format!("{} encoded", format_bytes(send.bytes)),
                    );
                    trace.event_with_duration(
                        "response_queue_first",
                        "Queueing first batch for the HTTP response",
                        send.queue_ms,
                        "Server-side queue/backpressure time; this is not network transfer time",
                    );
                }
            }
        }

        let finish_started = Instant::now();
        if let Err(error) = writer.finish() {
            let _ = tx.send(Err(io::Error::other(error.to_string()))).await;
            return;
        }
        ipc_encode_ms = ipc_encode_ms.saturating_add(millis(finish_started.elapsed()));

        if let Some(send) = send_pending(&tx, &sink).await {
            response_queue_ms = response_queue_ms.saturating_add(send.queue_ms);
            ipc_bytes = ipc_bytes.saturating_add(send.bytes);
        }

        if let Some(trace) = &trace {
            let storage = read_metrics.as_ref().map(ReadMetrics::snapshot).unwrap_or_default();
            trace.event_with_duration(
                "data_storage_total",
                "Storage I/O total",
                storage.io_ms,
                format!(
                    "{} object-store call{} · {} range{} · {} received",
                    storage.calls,
                    plural(storage.calls),
                    storage.ranges,
                    plural(storage.ranges),
                    format_bytes(storage.returned_bytes),
                ),
            );
            trace.event_with_duration(
                "parquet_reader_total",
                "Parquet reader work total",
                reader_wall_ms.saturating_sub(storage.io_ms),
                "Derived from RecordBatch polling time minus measured object-store wait",
            );
            trace.event_with_duration(
                "arrow_ipc_total",
                "Arrow IPC encoding total",
                ipc_encode_ms,
                format!("{} response bytes", format_bytes(ipc_bytes)),
            );
            trace.event_with_duration(
                "response_queue_total",
                "HTTP response queue/backpressure total",
                response_queue_ms,
                "Time awaiting the server response channel; network transfer is measured in the browser",
            );
            trace.finish_with_detail(
                "Data stream ready",
                format!(
                    "{row_count} rows · {batch_count} batches · {} Arrow IPC",
                    format_bytes(ipc_bytes),
                ),
            );
        }
    });
    ReceiverStream::new(rx)
}

struct SendStats {
    bytes: u64,
    queue_ms: u64,
}

async fn send_pending(
    tx: &mpsc::Sender<Result<Bytes, io::Error>>,
    sink: &SharedBuffer,
) -> Option<SendStats> {
    let bytes = sink.take();
    if bytes.is_empty() {
        return Some(SendStats {
            bytes: 0,
            queue_ms: 0,
        });
    }

    let byte_count = bytes.len() as u64;
    let started = Instant::now();
    tx.send(Ok(Bytes::from(bytes))).await.ok()?;
    Some(SendStats {
        bytes: byte_count,
        queue_ms: millis(started.elapsed()),
    })
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn plural(value: u64) -> &'static str {
    if value == 1 { "" } else { "s" }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.2} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.2} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn format_duration(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms} ms")
    } else {
        format!("{:.2} s", ms as f64 / 1_000.0)
    }
}
