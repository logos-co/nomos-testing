use std::{num::NonZeroU64, sync::Arc, time::Duration};

use async_trait::async_trait;
use ed25519_dalek::SigningKey;
use executor_http_client::ExecutorHttpClient;
use futures::future::try_join_all;
use key_management_system_service::keys::Ed25519PublicKey;
use nomos_core::{
    da::BlobId,
    mantle::ops::{
        Op,
        channel::{ChannelId, MsgId},
    },
};
use rand::{Rng as _, RngCore as _, seq::SliceRandom as _, thread_rng};
use testing_framework_core::{
    nodes::ApiClient,
    scenario::{
        BlockRecord, DynError, Expectation, RunContext, RunMetrics, Workload as ScenarioWorkload,
    },
};
use tokio::{sync::broadcast, time::sleep};

use super::expectation::DaWorkloadExpectation;
use crate::{
    util::tx,
    workloads::util::{find_channel_op, submit_transaction_via_cluster},
};

const TEST_KEY_BYTES: [u8; 32] = [0u8; 32];
const DEFAULT_BLOB_RATE_PER_BLOCK: u64 = 1;
const DEFAULT_CHANNEL_RATE_PER_BLOCK: u64 = 1;
const MIN_BLOB_CHUNKS: usize = 1;
const MAX_BLOB_CHUNKS: usize = 8;
const PUBLISH_RETRIES: usize = 5;
const PUBLISH_RETRY_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_HEADROOM_PERCENT: u64 = 20;

#[derive(Clone)]
pub struct Workload {
    blob_rate_per_block: NonZeroU64,
    channel_rate_per_block: NonZeroU64,
    headroom_percent: u64,
}

impl Default for Workload {
    fn default() -> Self {
        Self::with_rate(
            NonZeroU64::new(DEFAULT_BLOB_RATE_PER_BLOCK).expect("non-zero"),
            NonZeroU64::new(DEFAULT_CHANNEL_RATE_PER_BLOCK).expect("non-zero"),
            DEFAULT_HEADROOM_PERCENT,
        )
    }
}

impl Workload {
    /// Creates a workload that targets a blobs-per-block rate and applies a
    /// headroom factor when deriving the channel count.
    #[must_use]
    pub const fn with_rate(
        blob_rate_per_block: NonZeroU64,
        channel_rate_per_block: NonZeroU64,
        headroom_percent: u64,
    ) -> Self {
        Self {
            blob_rate_per_block,
            channel_rate_per_block,
            headroom_percent,
        }
    }

    #[must_use]
    pub const fn default_headroom_percent() -> u64 {
        DEFAULT_HEADROOM_PERCENT
    }
}

#[async_trait]
impl ScenarioWorkload for Workload {
    fn name(&self) -> &'static str {
        "channel_workload"
    }

    fn expectations(&self) -> Vec<Box<dyn Expectation>> {
        vec![Box::new(DaWorkloadExpectation::new(
            self.blob_rate_per_block,
            self.channel_rate_per_block,
            self.headroom_percent,
        ))]
    }

    async fn start(&self, ctx: &RunContext) -> Result<(), DynError> {
        let planned_channels = planned_channel_ids(planned_channel_count(
            self.channel_rate_per_block,
            self.headroom_percent,
        ));

        let expected_blobs = planned_blob_count(self.blob_rate_per_block, &ctx.run_metrics());
        let per_channel_target =
            per_channel_blob_target(expected_blobs, planned_channels.len().max(1) as u64);

        tracing::info!(
            blob_rate_per_block = self.blob_rate_per_block.get(),
            channel_rate = self.channel_rate_per_block.get(),
            headroom_percent = self.headroom_percent,
            planned_channels = planned_channels.len(),
            expected_blobs,
            per_channel_target,
            "DA workload derived planned channels"
        );

        try_join_all(planned_channels.into_iter().map(|channel_id| {
            let ctx = ctx;
            async move {
                let mut receiver = ctx.block_feed().subscribe();
                tracing::info!(channel_id = ?channel_id, blobs = per_channel_target, "DA workload starting channel flow");
                run_channel_flow(ctx, &mut receiver, channel_id, per_channel_target).await?;
                tracing::info!(channel_id = ?channel_id, "DA workload finished channel flow");
                Ok::<(), DynError>(())
            }
        }))
        .await?;

        tracing::info!("DA workload completed all channel flows");
        Ok(())
    }
}

async fn run_channel_flow(
    ctx: &RunContext,
    receiver: &mut broadcast::Receiver<Arc<BlockRecord>>,
    channel_id: ChannelId,
    target_blobs: u64,
) -> Result<(), DynError> {
    tracing::debug!(channel_id = ?channel_id, "DA: submitting inscription tx");
    let tx = Arc::new(tx::create_inscription_transaction_with_id(channel_id));
    submit_transaction_via_cluster(ctx, Arc::clone(&tx)).await?;

    let inscription_id = wait_for_inscription(receiver, channel_id).await?;
    tracing::debug!(channel_id = ?channel_id, inscription_id = ?inscription_id, "DA: inscription observed");
    let mut parent_id = inscription_id;

    for _ in 0..target_blobs {
        let blob_id = publish_blob(ctx, channel_id, parent_id).await?;
        tracing::debug!(channel_id = ?channel_id, blob_id = ?blob_id, "DA: blob published");
        parent_id = wait_for_blob(receiver, channel_id, blob_id).await?;
    }
    Ok(())
}

async fn wait_for_inscription(
    receiver: &mut broadcast::Receiver<Arc<BlockRecord>>,
    channel_id: ChannelId,
) -> Result<MsgId, DynError> {
    wait_for_channel_op(receiver, move |op| {
        if let Op::ChannelInscribe(inscribe) = op
            && inscribe.channel_id == channel_id
        {
            Some(inscribe.id())
        } else {
            None
        }
    })
    .await
}

async fn wait_for_blob(
    receiver: &mut broadcast::Receiver<Arc<BlockRecord>>,
    channel_id: ChannelId,
    blob_id: BlobId,
) -> Result<MsgId, DynError> {
    wait_for_channel_op(receiver, move |op| {
        if let Op::ChannelBlob(blob_op) = op
            && blob_op.channel == channel_id
            && blob_op.blob == blob_id
        {
            Some(blob_op.id())
        } else {
            None
        }
    })
    .await
}

async fn wait_for_channel_op<F>(
    receiver: &mut broadcast::Receiver<Arc<BlockRecord>>,
    mut matcher: F,
) -> Result<MsgId, DynError>
where
    F: FnMut(&Op) -> Option<MsgId>,
{
    loop {
        match receiver.recv().await {
            Ok(record) => {
                if let Some(msg_id) = find_channel_op(record.block.as_ref(), &mut matcher) {
                    tracing::debug!(?msg_id, "DA: matched channel operation");
                    return Ok(msg_id);
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => {
                return Err("block feed closed while waiting for channel operations".into());
            }
        }
    }
}

async fn publish_blob(
    ctx: &RunContext,
    channel_id: ChannelId,
    parent_msg: MsgId,
) -> Result<BlobId, DynError> {
    let executors = ctx.node_clients().executor_clients();
    if executors.is_empty() {
        return Err("da workload requires at least one executor".into());
    }

    let signer: Ed25519PublicKey = SigningKey::from_bytes(&TEST_KEY_BYTES)
        .verifying_key()
        .into();
    let data = random_blob_payload();
    tracing::debug!(channel = ?channel_id, payload_bytes = data.len(), "DA: prepared blob payload");
    let client = ExecutorHttpClient::new(None);

    let mut candidates: Vec<&ApiClient> = executors.iter().collect();
    let mut last_err = None;
    for attempt in 1..=PUBLISH_RETRIES {
        candidates.shuffle(&mut thread_rng());
        for executor in &candidates {
            let executor_url = executor.base_url().clone();
            match client
                .publish_blob(executor_url, channel_id, parent_msg, signer, data.clone())
                .await
            {
                Ok(blob_id) => return Ok(blob_id),
                Err(err) => {
                    tracing::debug!(attempt, executor = %executor.base_url(), %err, "DA: publish_blob failed");
                    last_err = Some(err.into())
                }
            }
        }

        if attempt < PUBLISH_RETRIES {
            sleep(PUBLISH_RETRY_DELAY).await;
        }
    }

    Err(last_err.unwrap_or_else(|| "da workload could not publish blob".into()))
}

fn random_blob_payload() -> Vec<u8> {
    let mut rng = thread_rng();
    let chunks = rng.gen_range(MIN_BLOB_CHUNKS..=MAX_BLOB_CHUNKS);
    let mut data = vec![0u8; 31 * chunks];
    rng.fill_bytes(&mut data);
    data
}

pub fn planned_channel_ids(total: usize) -> Vec<ChannelId> {
    (0..total as u64)
        .map(deterministic_channel_id)
        .collect::<Vec<_>>()
}

fn deterministic_channel_id(index: u64) -> ChannelId {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(b"chn_wrkd");
    bytes[24..].copy_from_slice(&index.to_be_bytes());
    ChannelId::from(bytes)
}

#[must_use]
pub fn planned_channel_count(channel_rate_per_block: NonZeroU64, headroom_percent: u64) -> usize {
    let base = channel_rate_per_block.get() as usize;
    let extra = (base.saturating_mul(headroom_percent as usize) + 99) / 100;
    let total = base.saturating_add(extra);
    total.max(1)
}

#[must_use]
pub fn planned_blob_count(blob_rate_per_block: NonZeroU64, run_metrics: &RunMetrics) -> u64 {
    let expected_blocks = run_metrics.expected_consensus_blocks().max(1);
    blob_rate_per_block.get().saturating_mul(expected_blocks)
}

#[must_use]
pub fn per_channel_blob_target(total_blobs: u64, channel_count: u64) -> u64 {
    if channel_count == 0 {
        return total_blobs.max(1);
    }
    let per = (total_blobs + channel_count - 1) / channel_count;
    per.max(1)
}
