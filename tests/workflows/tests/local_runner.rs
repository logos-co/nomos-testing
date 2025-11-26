use std::time::Duration;

use serial_test::serial;
use testing_framework_core::scenario::{Deployer as _, Runner, ScenarioBuilder};
use testing_framework_runner_local::LocalDeployer;
use tests_workflows::ScenarioBuilderExt as _;

const RUN_DURATION: Duration = Duration::from_secs(60);
const VALIDATORS: usize = 1;
const EXECUTORS: usize = 1;
const MIXED_TXS_PER_BLOCK: u64 = 5;
const TOTAL_WALLETS: usize = 64;
const TRANSACTION_WALLETS: usize = 8;

fn build_plan() -> testing_framework_core::scenario::Scenario {
    ScenarioBuilder::with_node_counts(VALIDATORS, EXECUTORS)
        .topology()
        .network_star()
        .validators(VALIDATORS)
        .executors(EXECUTORS)
        .apply()
        .wallets(TOTAL_WALLETS)
        .transactions()
        .rate(MIXED_TXS_PER_BLOCK)
        .users(TRANSACTION_WALLETS)
        .apply()
        .da()
        .channel_rate(1)
        .blob_rate(1)
        .apply()
        .with_run_duration(RUN_DURATION)
        .expect_consensus_liveness()
        .build()
}

#[tokio::test]
#[serial]
/// Drives both workloads concurrently to mimic a user mixing transaction flow
/// with blob publishing on the same topology.
async fn local_runner_mixed_workloads() {
    let mut plan = build_plan();
    let deployer = LocalDeployer::default();
    let runner: Runner = deployer.deploy(&plan).await.expect("scenario deployment");
    let _handle = runner.run(&mut plan).await.expect("scenario executed");
}
