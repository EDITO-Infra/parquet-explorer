//! In-memory request diagnostics used by the developer-facing progress panel.
//!
//! A trace stores milestones (`elapsed_ms`) and explicitly measured operations
//! (`duration_ms`) separately. Consumers must never infer operation duration by
//! subtracting adjacent milestone timestamps.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::RwLock;

use crate::model::{TraceEvent, TraceSnapshot};

/// Thread-safe store of short-lived traces keyed by caller-provided trace ID.
#[derive(Clone, Default)]
pub struct TraceStore {
    inner: Arc<RwLock<HashMap<String, TraceState>>>,
}

struct TraceState {
    operation: String,
    status: String,
    started: Instant,
    events: Vec<TraceEvent>,
}

/// Convenience handle that appends events to one trace without exposing the
/// store's locking/details to query code.
#[derive(Clone)]
pub struct TraceReporter {
    trace_id: String,
    store: TraceStore,
}

impl TraceStore {
    /// Start/replace a trace for this request ID. Empty IDs disable tracing.
    pub fn start(
        &self,
        trace_id: Option<&str>,
        operation: &str,
        message: &str,
    ) -> Option<TraceReporter> {
        let trace_id = trace_id?.trim();
        if trace_id.is_empty() {
            return None;
        }

        self.prune();
        let started = Instant::now();
        self.inner.write().insert(
            trace_id.to_string(),
            TraceState {
                operation: operation.to_string(),
                status: "running".to_string(),
                started,
                events: vec![TraceEvent {
                    stage: "start".to_string(),
                    message: message.to_string(),
                    detail: None,
                    elapsed_ms: 0,
                    duration_ms: None,
                }],
            },
        );

        Some(TraceReporter {
            trace_id: trace_id.to_string(),
            store: self.clone(),
        })
    }

    /// Reopen a reporter for a trace that was started earlier in the request.
    pub fn reporter(&self, trace_id: Option<&str>) -> Option<TraceReporter> {
        let trace_id = trace_id?.trim();
        if trace_id.is_empty() || !self.inner.read().contains_key(trace_id) {
            return None;
        }
        Some(TraceReporter {
            trace_id: trace_id.to_string(),
            store: self.clone(),
        })
    }

    /// Clone the current trace state for an HTTP polling response.
    pub fn snapshot(&self, trace_id: &str) -> Option<TraceSnapshot> {
        let traces = self.inner.read();
        let state = traces.get(trace_id)?;
        Some(TraceSnapshot {
            trace_id: trace_id.to_string(),
            operation: state.operation.clone(),
            status: state.status.clone(),
            elapsed_ms: millis(state.started.elapsed()),
            events: state.events.clone(),
        })
    }

    fn event(
        &self,
        trace_id: &str,
        stage: &str,
        message: &str,
        detail: Option<String>,
        duration_ms: Option<u64>,
    ) {
        if let Some(state) = self.inner.write().get_mut(trace_id) {
            state.events.push(TraceEvent {
                stage: stage.to_string(),
                message: message.to_string(),
                detail,
                elapsed_ms: millis(state.started.elapsed()),
                duration_ms,
            });
        }
    }

    fn finish(&self, trace_id: &str, message: &str, detail: Option<String>) {
        if let Some(state) = self.inner.write().get_mut(trace_id) {
            state.status = "complete".to_string();
            state.events.push(TraceEvent {
                stage: "complete".to_string(),
                message: message.to_string(),
                detail,
                elapsed_ms: millis(state.started.elapsed()),
                duration_ms: None,
            });
        }
    }

    fn fail(&self, trace_id: &str, message: &str) {
        if let Some(state) = self.inner.write().get_mut(trace_id) {
            state.status = "error".to_string();
            state.events.push(TraceEvent {
                stage: "error".to_string(),
                message: message.to_string(),
                detail: None,
                elapsed_ms: millis(state.started.elapsed()),
                duration_ms: None,
            });
        }
    }

    fn prune(&self) {
        const MAX_AGE: Duration = Duration::from_secs(10 * 60);
        const SOFT_LIMIT: usize = 256;

        let mut traces = self.inner.write();
        if traces.len() < SOFT_LIMIT {
            return;
        }
        traces.retain(|_, trace| trace.started.elapsed() < MAX_AGE);
    }
}

impl TraceReporter {
    /// Record a milestone with no implied duration.
    pub fn event(&self, stage: &str, message: &str) {
        self.store.event(&self.trace_id, stage, message, None, None);
    }

    /// Record a milestone plus human-readable contextual detail.
    pub fn event_with_detail(&self, stage: &str, message: &str, detail: impl Into<String>) {
        self.store
            .event(&self.trace_id, stage, message, Some(detail.into()), None);
    }

    /// Record work whose duration was measured explicitly at the call site.
    pub fn event_with_duration(
        &self,
        stage: &str,
        message: &str,
        duration_ms: u64,
        detail: impl Into<String>,
    ) {
        self.store.event(
            &self.trace_id,
            stage,
            message,
            Some(detail.into()),
            Some(duration_ms),
        );
    }

    /// Mark the trace complete and append its final milestone.
    pub fn finish_with_detail(&self, message: &str, detail: impl Into<String>) {
        self.store
            .finish(&self.trace_id, message, Some(detail.into()));
    }

    /// Mark the trace failed while preserving all events collected so far.
    pub fn fail(&self, message: &str) {
        self.store.fail(&self.trace_id, message);
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
