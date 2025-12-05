use std::{
    env,
    path::Path,
    process::{Command, Stdio},
};

// Manually run the local runner binary as a smoke test.
// This spins up real nodes and should be invoked explicitly:
// POL_PROOF_DEV_MODE=true cargo test -p runner-examples --test
// local_runner_bin_smoke -- --ignored --nocapture
#[test]
#[ignore = "runs local_runner binary (~2min) and requires local assets/binaries"]
fn local_runner_bin_smoke() {
    // Prefer a prebuilt local_runner binary (if provided), otherwise fall back to
    // cargo run.
    let runner_bin = env::var("LOCAL_RUNNER_BIN").ok();
    let mut cmd = match runner_bin.as_deref() {
        Some(path) => {
            let mut c = Command::new(path);
            c.args(["--nocapture"]);
            c
        }
        None => {
            let mut c = Command::new("cargo");
            c.args([
                "run",
                "-p",
                "runner-examples",
                "--bin",
                "local_runner",
                "--",
                "--nocapture",
            ]);
            c
        }
    };

    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("POL_PROOF_DEV_MODE", "true")
        .env(
            "NOMOS_CIRCUITS",
            env::var("NOMOS_CIRCUITS")
                .or_else(|_| {
                    let default = ".tmp/nomos-circuits";
                    if Path::new(default).exists() {
                        Ok(default.to_string())
                    } else {
                        Err(env::VarError::NotPresent)
                    }
                })
                .expect("NOMOS_CIRCUITS must be set or .tmp/nomos-circuits must exist"),
        )
        .env(
            "LOCAL_DEMO_RUN_SECS",
            env::var("LOCAL_DEMO_RUN_SECS").unwrap_or_else(|_| "120".into()),
        )
        .env(
            "LOCAL_DEMO_VALIDATORS",
            env::var("LOCAL_DEMO_VALIDATORS").unwrap_or_else(|_| "1".into()),
        )
        .env(
            "LOCAL_DEMO_EXECUTORS",
            env::var("LOCAL_DEMO_EXECUTORS").unwrap_or_else(|_| "1".into()),
        )
        .env("RUST_BACKTRACE", "1")
        .status()
        .expect("failed to spawn local runner");

    if !status.success() {
        panic!("local runner binary failed: status={status}");
    }
}
