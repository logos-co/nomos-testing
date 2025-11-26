use std::time::Duration;

use serial_test::serial;
use testing_framework_core::scenario::{Deployer as _, Runner, ScenarioBuilder};
use testing_framework_runner_k8s::{K8sRunner, K8sRunnerError};
use tests_workflows::ScenarioBuilderExt as _;

const RUN_DURATION: Duration = Duration::from_secs(60);
const VALIDATORS: usize = 1;
const EXECUTORS: usize = 1;
const MIXED_TXS_PER_BLOCK: u64 = 5;
const TOTAL_WALLETS: usize = 64;
const TRANSACTION_WALLETS: usize = 8;

#[tokio::test]
#[ignore = "requires access to a Kubernetes cluster"]
#[serial]
async fn k8s_runner_tx_workload() {
    let mut plan = ScenarioBuilder::with_node_counts(VALIDATORS, EXECUTORS)
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
        .build();

    let deployer = K8sRunner::new();
    let runner: Runner = match deployer.deploy(&plan).await {
        Ok(runner) => runner,
        Err(K8sRunnerError::ClientInit { source }) => {
            eprintln!("Skipping k8s_runner_tx_workload: Kubernetes cluster unavailable ({source})");
            return;
        }
        Err(err) => panic!("scenario deployment failed: {err}"),
    };

    let context = runner.context();
    assert!(
        context.telemetry().is_configured(),
        "k8s runner should expose prometheus metrics"
    );
    let validator_clients = context.node_clients().validator_clients().to_vec();

    let _handle = runner
        .run(&mut plan)
        .await
        .expect("k8s scenario should execute");

    for (idx, client) in validator_clients.iter().enumerate() {
        let info = client
            .consensus_info()
            .await
            .unwrap_or_else(|err| panic!("validator {idx} consensus_info failed: {err}"));
        assert!(
            info.height >= 5,
            "validator {idx} height {} should reach at least 5 blocks",
            info.height
        );
    }
}
