use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU64,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use nomos_core::mantle::{
    AuthenticatedMantleTx as _,
    ops::{Op, channel::ChannelId},
};
use testing_framework_core::scenario::{BlockRecord, DynError, Expectation, RunContext};
use thiserror::Error;
use tokio::sync::broadcast;

use super::workload::{planned_blob_count, planned_channel_count, planned_channel_ids};

#[derive(Debug)]
pub struct DaWorkloadExpectation {
    blob_rate_per_block: NonZeroU64,
    channel_rate_per_block: NonZeroU64,
    headroom_percent: u64,
    capture_state: Option<CaptureState>,
}

#[derive(Debug)]
struct CaptureState {
    planned: Arc<HashSet<ChannelId>>,
    inscriptions: Arc<Mutex<HashSet<ChannelId>>>,
    blobs: Arc<Mutex<HashMap<ChannelId, u64>>>,
    expected_total_blobs: u64,
}

const MIN_INCLUSION_RATIO: f64 = 0.8;

#[derive(Debug, Error)]
enum DaExpectationError {
    #[error("da workload expectation not started")]
    NotCaptured,
    #[error("missing inscriptions for {missing:?}")]
    MissingInscriptions { missing: Vec<ChannelId> },
    #[error("missing blobs for {missing:?}")]
    MissingBlobs { missing: Vec<ChannelId> },
}

impl DaWorkloadExpectation {
    /// Validates that inscriptions and blobs landed for the planned channels.
    pub const fn new(
        blob_rate_per_block: NonZeroU64,
        channel_rate_per_block: NonZeroU64,
        headroom_percent: u64,
    ) -> Self {
        Self {
            blob_rate_per_block,
            channel_rate_per_block,
            headroom_percent,
            capture_state: None,
        }
    }
}

#[async_trait]
impl Expectation for DaWorkloadExpectation {
    fn name(&self) -> &'static str {
        "da_workload_inclusions"
    }

    async fn start_capture(&mut self, ctx: &RunContext) -> Result<(), DynError> {
        if self.capture_state.is_some() {
            return Ok(());
        }

        let planned_ids = planned_channel_ids(planned_channel_count(
            self.channel_rate_per_block,
            self.headroom_percent,
        ));

        let expected_total_blobs = planned_blob_count(self.blob_rate_per_block, &ctx.run_metrics());

        tracing::info!(
            planned_channels = planned_ids.len(),
            blob_rate_per_block = self.blob_rate_per_block.get(),
            headroom_percent = self.headroom_percent,
            expected_total_blobs,
            "DA inclusion expectation starting capture"
        );

        let planned = Arc::new(planned_ids.iter().copied().collect::<HashSet<_>>());
        let inscriptions = Arc::new(Mutex::new(HashSet::new()));
        let blobs = Arc::new(Mutex::new(HashMap::new()));

        let mut receiver = ctx.block_feed().subscribe();
        let planned_for_task = Arc::clone(&planned);
        let inscriptions_for_task = Arc::clone(&inscriptions);
        let blobs_for_task = Arc::clone(&blobs);

        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(record) => capture_block(
                        record.as_ref(),
                        &planned_for_task,
                        &inscriptions_for_task,
                        &blobs_for_task,
                    ),
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!(skipped, "DA expectation: receiver lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("DA expectation: block feed closed");
                        break;
                    }
                }
            }
        });

        self.capture_state = Some(CaptureState {
            planned,
            inscriptions,
            blobs,
            expected_total_blobs,
        });

        Ok(())
    }

    async fn evaluate(&mut self, _ctx: &RunContext) -> Result<(), DynError> {
        let state = self
            .capture_state
            .as_ref()
            .ok_or(DaExpectationError::NotCaptured)
            .map_err(DynError::from)?;

        let planned_total = state.planned.len();
        let missing_inscriptions = {
            let inscriptions = state
                .inscriptions
                .lock()
                .expect("inscription lock poisoned");
            missing_channels(&state.planned, &inscriptions)
        };
        let required_inscriptions = minimum_required(planned_total, MIN_INCLUSION_RATIO);
        if planned_total.saturating_sub(missing_inscriptions.len()) < required_inscriptions {
            tracing::warn!(
                planned = planned_total,
                missing = missing_inscriptions.len(),
                required = required_inscriptions,
                "DA expectation missing inscriptions"
            );
            return Err(DaExpectationError::MissingInscriptions {
                missing: missing_inscriptions,
            }
            .into());
        }

        let observed_total_blobs = {
            let blobs = state.blobs.lock().expect("blob lock poisoned");
            blobs.values().sum::<u64>()
        };
        let required_blobs = minimum_required_u64(state.expected_total_blobs, MIN_INCLUSION_RATIO);
        if observed_total_blobs < required_blobs {
            tracing::warn!(
                planned = state.expected_total_blobs,
                observed = observed_total_blobs,
                required = required_blobs,
                "DA expectation missing blobs"
            );
            return Err(DaExpectationError::MissingBlobs {
                missing: Vec::new(),
            }
            .into());
        }

        tracing::info!(
            planned = planned_total,
            inscriptions = planned_total - missing_inscriptions.len(),
            blobs_observed = observed_total_blobs,
            "DA inclusion expectation satisfied"
        );

        Ok(())
    }
}

fn capture_block(
    block: &BlockRecord,
    planned: &HashSet<ChannelId>,
    inscriptions: &Arc<Mutex<HashSet<ChannelId>>>,
    blobs: &Arc<Mutex<HashMap<ChannelId, u64>>>,
) {
    let mut new_inscriptions = Vec::new();
    let mut new_blobs = Vec::new();

    for tx in block.block.transactions() {
        for op in &tx.mantle_tx().ops {
            match op {
                Op::ChannelInscribe(inscribe) if planned.contains(&inscribe.channel_id) => {
                    new_inscriptions.push(inscribe.channel_id);
                }
                Op::ChannelBlob(blob) if planned.contains(&blob.channel) => {
                    new_blobs.push(blob.channel);
                }
                _ => {}
            }
        }
    }

    if !new_inscriptions.is_empty() {
        let mut guard = inscriptions.lock().expect("inscription lock poisoned");
        guard.extend(new_inscriptions);
        tracing::debug!(count = guard.len(), "DA expectation captured inscriptions");
    }

    if !new_blobs.is_empty() {
        let mut guard = blobs.lock().expect("blob lock poisoned");
        for channel in new_blobs {
            let entry = guard.entry(channel).or_insert(0);
            *entry += 1;
        }
        tracing::debug!(
            total_blobs = guard.values().sum::<u64>(),
            "DA expectation captured blobs"
        );
    }
}

fn missing_channels(planned: &HashSet<ChannelId>, observed: &HashSet<ChannelId>) -> Vec<ChannelId> {
    planned.difference(observed).copied().collect()
}

fn minimum_required(total: usize, ratio: f64) -> usize {
    ((total as f64) * ratio).ceil() as usize
}

fn minimum_required_u64(total: u64, ratio: f64) -> u64 {
    ((total as f64) * ratio).ceil() as u64
}
