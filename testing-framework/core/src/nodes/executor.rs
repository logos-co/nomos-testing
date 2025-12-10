use std::{ops::Deref, path::PathBuf};

use nomos_executor::config::Config;
use nomos_tracing_service::LoggerLayer;
pub use testing_framework_config::nodes::executor::create_executor_config;

use super::{persist_tempdir, should_persist_tempdir};
use crate::{
    IS_DEBUG_TRACING,
    nodes::{
        LOGS_PREFIX,
        common::{
            binary::{BinaryConfig, BinaryResolver},
            lifecycle::kill::kill_child,
            node::{NodeConfigCommon, NodeHandle, spawn_node},
        },
    },
};

const BIN_PATH: &str = "target/debug/nomos-executor";

fn binary_path() -> PathBuf {
    let cfg = BinaryConfig {
        env_var: "NOMOS_EXECUTOR_BIN",
        binary_name: "nomos-executor",
        fallback_path: BIN_PATH,
        shared_bin_subpath: "testing-framework/assets/stack/bin/nomos-executor",
    };
    BinaryResolver::resolve_path(&cfg)
}

pub struct Executor {
    handle: NodeHandle<Config>,
}

impl Deref for Executor {
    type Target = NodeHandle<Config>;

    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        if should_persist_tempdir()
            && let Err(e) = persist_tempdir(&mut self.handle.tempdir, "nomos-executor")
        {
            println!("failed to persist tempdir: {e}");
        }

        kill_child(&mut self.handle.child);
    }
}

impl Executor {
    pub async fn spawn(config: Config) -> Self {
        let handle = spawn_node(
            config,
            LOGS_PREFIX,
            "executor.yaml",
            binary_path(),
            !*IS_DEBUG_TRACING,
        )
        .await
        .expect("executor did not become ready");

        Self { handle }
    }
}

impl NodeConfigCommon for Config {
    fn set_logger(&mut self, logger: LoggerLayer) {
        self.tracing.logger = logger;
    }

    fn set_paths(&mut self, base: &std::path::Path) {
        self.storage.db_path = base.join("db");
        base.clone_into(
            &mut self
                .da_verifier
                .storage_adapter_settings
                .blob_storage_directory,
        );
    }

    fn addresses(&self) -> (std::net::SocketAddr, Option<std::net::SocketAddr>) {
        (
            self.http.backend_settings.address,
            Some(self.testing_http.backend_settings.address),
        )
    }
}
