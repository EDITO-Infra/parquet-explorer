//! Core read-only dataset/query engine.
//!
//! `CoreEngine` is intentionally a small facade. Feature implementations live
//! in sibling modules and share immutable `OpenedDataset` execution contexts.

mod analysis;
mod dataset;
mod export;
mod ipc;
mod metadata;
mod query;
mod registry;
mod source;
mod spatial;
mod trace;

use std::sync::Arc;

use anyhow::Result;

use dataset::OpenedDataset;
use registry::DatasetRegistry;
use trace::TraceStore;

/// Shared native engine state.
pub struct CoreEngine {
    registry: DatasetRegistry,
    batch_size: usize,
    ipc_channel_capacity: usize,
    traces: TraceStore,
}

impl CoreEngine {
    pub fn new(
        batch_size: usize,
        max_open_datasets: usize,
        dataset_idle_timeout_seconds: u64,
    ) -> Self {
        Self {
            registry: DatasetRegistry::new(max_open_datasets, dataset_idle_timeout_seconds),
            batch_size,
            ipc_channel_capacity: 4,
            traces: TraceStore::default(),
        }
    }

    /// Starts a new trace with the given ID, operation, and message.
    pub fn start_trace(&self, trace_id: Option<&str>, operation: &str, message: &str) {
        self.traces.start(trace_id, operation, message);
    }

    /// Records an event for the given trace ID, stage, and message.
    pub fn trace_event(&self, trace_id: Option<&str>, stage: &str, message: &str) {
        if let Some(trace) = self.traces.reporter(trace_id) {
            trace.event(stage, message);
        }
    }

    /// Records a failure for the given trace ID and message.
    pub fn trace_fail(&self, trace_id: Option<&str>, message: &str) {
        if let Some(trace) = self.traces.reporter(trace_id) {
            trace.fail(message);
        }
    }

    /// Returns a snapshot of the given trace ID.
    pub fn trace_snapshot(&self, trace_id: &str) -> Option<crate::model::TraceSnapshot> {
        self.traces.snapshot(trace_id)
    }

    /// Returns the dataset with the given ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the dataset is not found.
    pub(in crate::engine) fn dataset(&self, dataset_id: &str) -> Result<Arc<OpenedDataset>> {
        self.registry.get(dataset_id)
    }
}
