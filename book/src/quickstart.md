# Quickstart

Get a working example running quickly.

## Prerequisites

- Rust toolchain (nightly)
- Sibling `nomos-node` checkout built and available
- This repository cloned
- Unix-like system (tested on Linux and macOS)

## Your First Test

The framework ships with runnable example binaries in `examples/src/bin/`. Let's start with the local runner:

```bash
# From the nomos-testing directory
POL_PROOF_DEV_MODE=true cargo run -p runner-examples --bin local_runner
```

This runs a complete scenario with **defaults**: 1 validator + 1 executor, mixed transaction + DA workload (5 tx/block + 1 channel + 1 blob), 60s duration.

**Core API Pattern** (simplified example):

```rust
use testing_framework_core::scenario::{Deployer, ScenarioBuilder};
use testing_framework_runner_local::LocalDeployer;
use testing_framework_workflows::ScenarioBuilderExt;
use std::time::Duration;

// Define the scenario (1 validator + 1 executor, tx + DA workload)
let mut plan = ScenarioBuilder::topology_with(|t| {
        t.network_star()
            .validators(1)
            .executors(1)
    })
    .wallets(64)
    .transactions_with(|txs| {
        txs.rate(5)                 // 5 transactions per block
            .users(8)
    })
    .da_with(|da| {
        da.channel_rate(1)         // 1 channel operation per block
            .blob_rate(1)          // 1 blob dispersal per block
    })
    .expect_consensus_liveness()
    .with_run_duration(Duration::from_secs(60))
    .build();

// Deploy and run
let deployer = LocalDeployer::default();
let runner = deployer.deploy(&plan).await?;
let _handle = runner.run(&mut plan).await?;
```

**Note:** The examples are binaries with `#[tokio::main]`, not test functions. If you want to write integration tests, wrap this pattern in `#[tokio::test]` functions in your own test suite.

**Important:** `POL_PROOF_DEV_MODE=true` disables expensive Groth16 zero-knowledge proof generation for leader election. Without it, proof generation is CPU-intensive and tests will timeout. **This is required for all runners** (local, compose, k8s) for practical testing. Never use in production.

**What you should see:**
- Nodes spawn as local processes
- Consensus starts producing blocks
- Scenario runs for the configured duration
- Node logs written to temporary directories in working directory (auto-cleaned up after test)
- To persist logs: set `NOMOS_TESTS_TRACING=true` and `NOMOS_LOG_DIR=/path/to/logs` (files will have prefix like `nomos-node-0*`, may include timestamps)

## What Just Happened?

Let's unpack the code:

### 1. Topology Configuration

```rust
ScenarioBuilder::topology_with(|t| {
        t.network_star()      // Star topology: all nodes connect to seed
            .validators(1)    // 1 validator node
            .executors(1)     // 1 executor node (validator + DA dispersal)
    })
```

This defines **what** your test network looks like.

### 2. Wallet Seeding

```rust
.wallets(64)                 // Seed 64 funded wallet accounts
```

Provides funded accounts for transaction submission.

### 3. Workloads

```rust
.transactions()
    .rate(5)                 // 5 transactions per block
    .users(8)                // Use 8 of the 64 wallets
    .apply()
.da()
    .channel_rate(1)         // 1 channel operation per block
    .blob_rate(1)            // 1 blob dispersal per block
    .apply()
```

Generates both transaction and DA traffic to stress both subsystems.

### 4. Expectation

```rust
.expect_consensus_liveness()
```

This says **what success means**: blocks must be produced continuously.

### 5. Run Duration

```rust
.with_run_duration(Duration::from_secs(60))
```

Run for 60 seconds (~27 blocks with default 2s slots, 0.9 coefficient). Framework ensures this is at least 2× the consensus slot duration.

### 6. Deploy and Execute

```rust
let deployer = LocalDeployer::default();  // Use local process deployer
let runner = deployer.deploy(&plan).await?;  // Provision infrastructure
let _handle = runner.run(&mut plan).await?;  // Execute workloads & expectations
```

**Deployer** provisions the infrastructure. **Runner** orchestrates execution.

## Adjust the Topology

The binary accepts environment variables to adjust defaults:

```bash
# Scale up to 3 validators + 2 executors, run for 2 minutes
LOCAL_DEMO_VALIDATORS=3 \
LOCAL_DEMO_EXECUTORS=2 \
LOCAL_DEMO_RUN_SECS=120 \
POL_PROOF_DEV_MODE=true \
cargo run -p runner-examples --bin local_runner
```

## Try Docker Compose

Use the same API with a different deployer for reproducible containerized environment:

```bash
# Build the test image first (includes circuit assets)
chmod +x scripts/setup-nomos-circuits.sh
scripts/setup-nomos-circuits.sh v0.3.1 /tmp/nomos-circuits
cp -r /tmp/nomos-circuits/* testing-framework/assets/stack/kzgrs_test_params/

chmod +x testing-framework/assets/stack/scripts/build_test_image.sh
testing-framework/assets/stack/scripts/build_test_image.sh

# Run with Compose
NOMOS_TESTNET_IMAGE=nomos-testnet:local \
POL_PROOF_DEV_MODE=true \
cargo run -p runner-examples --bin compose_runner
```

**Benefit:** Reproducible containerized environment with Prometheus at `http://localhost:9090`.

**In code:** Just swap the deployer:

```rust
use testing_framework_runner_compose::ComposeDeployer;

// ... same scenario definition ...

let deployer = ComposeDeployer::default();  // Use Docker Compose
let runner = deployer.deploy(&plan).await?;
let _handle = runner.run(&mut plan).await?;
```

## Next Steps

Now that you have a working test:

- **Understand the philosophy**: [Testing Philosophy](testing-philosophy.md)
- **Learn the architecture**: [Architecture Overview](architecture-overview.md)
- **See more examples**: [Examples](examples.md)
- **API reference**: [Builder API Quick Reference](dsl-cheat-sheet.md)
- **Debug failures**: [Troubleshooting](troubleshooting.md)
