use std::time::Duration;

use futures::future::join_all;
use nomos_core::sdp::SessionNumber;
use nomos_da_network_core::swarm::BalancerStats;
use nomos_da_network_service::MembershipResponse;
use nomos_http_api_common::paths;
use nomos_network::backends::libp2p::Libp2pInfo;
use reqwest::{Client, Url};
use thiserror::Error;
use tokio::time::{sleep, timeout};
use tracing::warn;

use crate::{adjust_timeout, topology::Topology};

#[derive(Debug, Error)]
pub enum ReadinessError {
    #[error("{message}")]
    Timeout { message: String },
}

#[async_trait::async_trait]
pub trait ReadinessCheck<'a> {
    type Data: Send;

    async fn collect(&'a self) -> Self::Data;

    fn is_ready(&self, data: &Self::Data) -> bool;

    fn timeout_message(&self, data: Self::Data) -> String;

    fn poll_interval(&self) -> Duration {
        Duration::from_millis(200)
    }

    async fn wait(&'a self) -> Result<(), ReadinessError> {
        let timeout_duration = adjust_timeout(Duration::from_secs(60));
        let poll_interval = self.poll_interval();
        let mut data = self.collect().await;

        let wait_result = timeout(timeout_duration, async {
            loop {
                if self.is_ready(&data) {
                    return;
                }

                sleep(poll_interval).await;

                data = self.collect().await;
            }
        })
        .await;

        if wait_result.is_err() {
            let message = self.timeout_message(data);
            return Err(ReadinessError::Timeout { message });
        }

        Ok(())
    }
}

pub struct NetworkReadiness<'a> {
    pub(crate) topology: &'a Topology,
    pub(crate) expected_peer_counts: &'a [usize],
    pub(crate) labels: &'a [String],
}

#[async_trait::async_trait]
impl<'a> ReadinessCheck<'a> for NetworkReadiness<'a> {
    type Data = Vec<Libp2pInfo>;

    async fn collect(&'a self) -> Self::Data {
        let (validator_infos, executor_infos) = tokio::join!(
            join_all(
                self.topology
                    .validators
                    .iter()
                    .map(crate::nodes::validator::Validator::network_info)
            ),
            join_all(
                self.topology
                    .executors
                    .iter()
                    .map(crate::nodes::executor::Executor::network_info)
            )
        );

        validator_infos.into_iter().chain(executor_infos).collect()
    }

    fn is_ready(&self, data: &Self::Data) -> bool {
        data.iter()
            .enumerate()
            .all(|(idx, info)| info.n_peers >= self.expected_peer_counts[idx])
    }

    fn timeout_message(&self, data: Self::Data) -> String {
        let summary = build_timeout_summary(self.labels, data, self.expected_peer_counts);
        format!("timed out waiting for network readiness: {summary}")
    }
}

pub struct HttpNetworkReadiness<'a> {
    pub(crate) client: &'a Client,
    pub(crate) endpoints: &'a [Url],
    pub(crate) expected_peer_counts: &'a [usize],
    pub(crate) labels: &'a [String],
}

#[async_trait::async_trait]
impl<'a> ReadinessCheck<'a> for HttpNetworkReadiness<'a> {
    type Data = Vec<Libp2pInfo>;

    async fn collect(&'a self) -> Self::Data {
        let futures = self
            .endpoints
            .iter()
            .map(|endpoint| fetch_network_info(self.client, endpoint));
        join_all(futures).await
    }

    fn is_ready(&self, data: &Self::Data) -> bool {
        data.iter()
            .enumerate()
            .all(|(idx, info)| info.n_peers >= self.expected_peer_counts[idx])
    }

    fn timeout_message(&self, data: Self::Data) -> String {
        let summary = build_timeout_summary(self.labels, data, self.expected_peer_counts);
        format!("timed out waiting for network readiness: {summary}")
    }
}

pub struct MembershipReadiness<'a> {
    pub(crate) topology: &'a Topology,
    pub(crate) session: SessionNumber,
    pub(crate) labels: &'a [String],
    pub(crate) expect_non_empty: bool,
}

#[async_trait::async_trait]
impl<'a> ReadinessCheck<'a> for MembershipReadiness<'a> {
    type Data = Vec<Result<MembershipResponse, reqwest::Error>>;

    async fn collect(&'a self) -> Self::Data {
        let (validator_responses, executor_responses) = tokio::join!(
            join_all(
                self.topology
                    .validators
                    .iter()
                    .map(|node| node.da_get_membership(self.session)),
            ),
            join_all(
                self.topology
                    .executors
                    .iter()
                    .map(|node| node.da_get_membership(self.session)),
            )
        );

        validator_responses
            .into_iter()
            .chain(executor_responses)
            .collect()
    }

    fn is_ready(&self, data: &Self::Data) -> bool {
        self.assignation_statuses(data)
            .into_iter()
            .all(|ready| ready)
    }

    fn timeout_message(&self, data: Self::Data) -> String {
        let statuses = self.assignation_statuses(&data);
        let description = if self.expect_non_empty {
            "non-empty assignations"
        } else {
            "empty assignations"
        };
        let summary = build_membership_summary(self.labels, &statuses, description);
        format!("timed out waiting for DA membership readiness ({description}): {summary}")
    }
}

impl MembershipReadiness<'_> {
    fn assignation_statuses(
        &self,
        responses: &[Result<MembershipResponse, reqwest::Error>],
    ) -> Vec<bool> {
        responses
            .iter()
            .map(|res| {
                res.as_ref()
                    .map(|resp| {
                        let is_non_empty = !resp.assignations.is_empty();
                        if self.expect_non_empty {
                            is_non_empty
                        } else {
                            !is_non_empty
                        }
                    })
                    .unwrap_or(false)
            })
            .collect()
    }
}

pub struct HttpMembershipReadiness<'a> {
    pub(crate) client: &'a Client,
    pub(crate) endpoints: &'a [Url],
    pub(crate) session: SessionNumber,
    pub(crate) labels: &'a [String],
    pub(crate) expect_non_empty: bool,
}

#[async_trait::async_trait]
impl<'a> ReadinessCheck<'a> for HttpMembershipReadiness<'a> {
    type Data = Vec<Result<MembershipResponse, reqwest::Error>>;

    async fn collect(&'a self) -> Self::Data {
        let futures = self
            .endpoints
            .iter()
            .map(|endpoint| fetch_membership(self.client, endpoint, self.session));
        futures::future::join_all(futures).await
    }

    fn is_ready(&self, data: &Self::Data) -> bool {
        assignation_statuses(data, self.expect_non_empty)
            .into_iter()
            .all(|ready| ready)
    }

    fn timeout_message(&self, data: Self::Data) -> String {
        let statuses = assignation_statuses(&data, self.expect_non_empty);
        let description = if self.expect_non_empty {
            "non-empty assignations"
        } else {
            "empty assignations"
        };
        let summary = build_membership_summary(self.labels, &statuses, description);
        format!("timed out waiting for DA membership readiness ({description}): {summary}")
    }
}

pub struct DaBalancerReadiness<'a> {
    pub(crate) topology: &'a Topology,
    pub(crate) labels: &'a [String],
}

#[async_trait::async_trait]
impl<'a> ReadinessCheck<'a> for DaBalancerReadiness<'a> {
    type Data = Vec<(String, usize, BalancerStats)>;

    async fn collect(&'a self) -> Self::Data {
        let mut data = Vec::new();
        for (idx, validator) in self.topology.validators.iter().enumerate() {
            data.push((
                self.labels[idx].clone(),
                validator.config().da_network.subnet_threshold,
                validator.balancer_stats().await,
            ));
        }
        for (offset, executor) in self.topology.executors.iter().enumerate() {
            let label_index = self.topology.validators.len() + offset;
            data.push((
                self.labels[label_index].clone(),
                executor.config().da_network.subnet_threshold,
                executor.balancer_stats().await,
            ));
        }
        data
    }

    fn is_ready(&self, data: &Self::Data) -> bool {
        data.iter().all(|(_, threshold, stats)| {
            if *threshold == 0 {
                return true;
            }
            connected_subnetworks(stats) >= *threshold
        })
    }

    fn timeout_message(&self, data: Self::Data) -> String {
        let summary = data
            .into_iter()
            .map(|(label, threshold, stats)| {
                let connected = connected_subnetworks(&stats);
                let details = format_balancer_stats(&stats);
                format!("{label}: connected={connected}, required={threshold}, stats={details}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("timed out waiting for DA balancer readiness: {summary}")
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_secs(1)
    }
}

fn connected_subnetworks(stats: &BalancerStats) -> usize {
    stats
        .values()
        .filter(|stat| stat.inbound > 0 || stat.outbound > 0)
        .count()
}

fn format_balancer_stats(stats: &BalancerStats) -> String {
    if stats.is_empty() {
        return "empty".into();
    }
    stats
        .iter()
        .map(|(subnet, stat)| format!("{}:in={},out={}", subnet, stat.inbound, stat.outbound))
        .collect::<Vec<_>>()
        .join(";")
}

fn build_timeout_summary(
    labels: &[String],
    infos: Vec<Libp2pInfo>,
    expected_counts: &[usize],
) -> String {
    infos
        .into_iter()
        .zip(expected_counts.iter())
        .zip(labels.iter())
        .map(|((info, expected), label)| {
            format!("{}: peers={}, expected={}", label, info.n_peers, expected)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn build_membership_summary(labels: &[String], statuses: &[bool], description: &str) -> String {
    statuses
        .iter()
        .zip(labels.iter())
        .map(|(ready, label)| {
            let status = if *ready { "ready" } else { "waiting" };
            format!("{label}: status={status}, expected {description}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

async fn fetch_network_info(client: &Client, base: &Url) -> Libp2pInfo {
    let url = join_path(base, paths::NETWORK_INFO);
    let response = match client.get(url).send().await {
        Ok(resp) => resp,
        Err(err) => {
            return log_network_warning(base, err, "failed to reach network info endpoint");
        }
    };

    let response = match response.error_for_status() {
        Ok(resp) => resp,
        Err(err) => {
            return log_network_warning(base, err, "network info endpoint returned error");
        }
    };

    match response.json::<Libp2pInfo>().await {
        Ok(info) => info,
        Err(err) => log_network_warning(base, err, "failed to decode network info response"),
    }
}

async fn fetch_membership(
    client: &Client,
    base: &Url,
    session: SessionNumber,
) -> Result<MembershipResponse, reqwest::Error> {
    let url = join_path(base, paths::DA_GET_MEMBERSHIP);
    client
        .post(url)
        .json(&session)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

fn log_network_warning(base: &Url, err: impl std::fmt::Display, message: &str) -> Libp2pInfo {
    warn!(target: "readiness", url = %base, error = %err, "{message}");
    empty_libp2p_info()
}

fn empty_libp2p_info() -> Libp2pInfo {
    Libp2pInfo {
        listen_addresses: Vec::with_capacity(0),
        n_peers: 0,
        n_connections: 0,
        n_pending_connections: 0,
    }
}

fn join_path(base: &Url, path: &str) -> Url {
    base.join(path.trim_start_matches('/'))
        .unwrap_or_else(|err| panic!("failed to join url {base} with path {path}: {err}"))
}

fn assignation_statuses(
    responses: &[Result<MembershipResponse, reqwest::Error>],
    expect_non_empty: bool,
) -> Vec<bool> {
    responses
        .iter()
        .map(|res| {
            res.as_ref()
                .map(|resp| {
                    let is_non_empty = !resp.assignations.is_empty();
                    if expect_non_empty {
                        is_non_empty
                    } else {
                        !is_non_empty
                    }
                })
                .unwrap_or(false)
        })
        .collect()
}
