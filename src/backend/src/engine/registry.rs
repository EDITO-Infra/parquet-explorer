//! Process-local registry for opened dataset execution contexts.
//!
//! The registry owns only handles and lifecycle timestamps. Parquet metadata,
//! object-store clients, and derived layout information live on `OpenedDataset`.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow, bail};
use parking_lot::RwLock;

use super::dataset::OpenedDataset;

#[derive(Clone)]
struct DatasetEntry {
    dataset: Arc<OpenedDataset>,
    opened_at: Instant,
    last_accessed_at: Instant,
}

pub(super) struct DatasetRegistry {
    entries: RwLock<HashMap<String, DatasetEntry>>,
    max_open_datasets: usize,
    idle_timeout: Option<Duration>,
}

impl DatasetRegistry {
    pub(super) fn new(max_open_datasets: usize, idle_timeout_seconds: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_open_datasets,
            idle_timeout: (idle_timeout_seconds > 0)
                .then(|| Duration::from_secs(idle_timeout_seconds)),
        }
    }

    pub(super) fn ensure_capacity(&self) -> Result<()> {
        let now = Instant::now();
        let mut entries = self.entries.write();
        Self::prune_expired_locked(&mut entries, self.idle_timeout, now);
        if entries.len() >= self.max_open_datasets {
            bail!("maximum number of open dataset handles reached");
        }
        Ok(())
    }

    pub(super) fn insert(&self, dataset: Arc<OpenedDataset>) -> Result<()> {
        let now = Instant::now();
        let mut entries = self.entries.write();
        Self::prune_expired_locked(&mut entries, self.idle_timeout, now);
        if entries.len() >= self.max_open_datasets {
            bail!("maximum number of open dataset handles reached");
        }

        entries.insert(
            dataset.info.dataset_id.clone(),
            DatasetEntry {
                dataset,
                opened_at: now,
                last_accessed_at: now,
            },
        );
        Ok(())
    }

    pub(super) fn get(&self, dataset_id: &str) -> Result<Arc<OpenedDataset>> {
        let now = Instant::now();
        let mut entries = self.entries.write();

        if let Some(timeout) = self.idle_timeout {
            let expired = entries
                .get(dataset_id)
                .map(|entry| now.duration_since(entry.last_accessed_at) >= timeout)
                .unwrap_or(false);
            if expired {
                entries.remove(dataset_id);
                bail!(
                    "dataset not found: {dataset_id} (idle handle expired; reopen the source dataset)"
                );
            }
        }

        let entry = entries
            .get_mut(dataset_id)
            .ok_or_else(|| anyhow!("dataset not found: {dataset_id}"))?;
        entry.last_accessed_at = now;
        Ok(Arc::clone(&entry.dataset))
    }

    pub(super) fn remove(&self, dataset_id: &str) -> Result<()> {
        if self.entries.write().remove(dataset_id).is_none() {
            bail!("dataset not found: {dataset_id}");
        }
        Ok(())
    }

    pub(super) fn prune_expired(&self) -> usize {
        let mut entries = self.entries.write();
        Self::prune_expired_locked(&mut entries, self.idle_timeout, Instant::now())
    }

    pub(super) fn cleanup_interval(&self) -> Option<Duration> {
        self.idle_timeout
            .map(|timeout| Duration::from_secs(timeout.as_secs().clamp(1, 60)))
    }

    fn prune_expired_locked(
        entries: &mut HashMap<String, DatasetEntry>,
        timeout: Option<Duration>,
        now: Instant,
    ) -> usize {
        let Some(timeout) = timeout else {
            return 0;
        };
        let before = entries.len();

        entries.retain(|dataset_id, entry| {
            let idle_for = now.duration_since(entry.last_accessed_at);
            let keep = idle_for < timeout;
            if !keep {
                tracing::debug!(
                    dataset_id = %dataset_id,
                    idle_seconds = idle_for.as_secs_f64(),
                    open_seconds = now.duration_since(entry.opened_at).as_secs_f64(),
                    "expired idle dataset handle"
                );
            }
            keep
        });

        before - entries.len()
    }
}
