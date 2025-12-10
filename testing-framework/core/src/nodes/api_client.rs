use std::net::SocketAddr;

use chain_service::CryptarchiaInfo;
use common_http_client::CommonHttpClient;
use hex;
use nomos_core::{block::Block, da::BlobId, mantle::SignedMantleTx, sdp::SessionNumber};
use nomos_da_network_core::swarm::{BalancerStats, MonitorStats};
use nomos_da_network_service::MembershipResponse;
use nomos_http_api_common::paths::{
    CRYPTARCHIA_HEADERS, CRYPTARCHIA_INFO, DA_BALANCER_STATS, DA_BLACKLISTED_PEERS, DA_BLOCK_PEER,
    DA_GET_MEMBERSHIP, DA_HISTORIC_SAMPLING, DA_MONITOR_STATS, DA_UNBLOCK_PEER, MEMPOOL_ADD_TX,
    NETWORK_INFO, STORAGE_BLOCK,
};
use nomos_network::backends::libp2p::Libp2pInfo;
use nomos_node::{HeaderId, api::testing::handlers::HistoricSamplingRequest};
use reqwest::{Client, RequestBuilder, Response, Url};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

pub const DA_GET_TESTING_ENDPOINT_ERROR: &str = "Failed to connect to testing endpoint. The binary was likely built without the 'testing' \
     feature. Try: cargo build --workspace --all-features";

/// Thin async client for node HTTP/testing endpoints.
#[derive(Clone)]
pub struct ApiClient {
    pub(crate) base_url: Url,
    pub(crate) testing_url: Option<Url>,
    client: Client,
    pub(crate) http_client: CommonHttpClient,
}

impl ApiClient {
    #[must_use]
    /// Construct from socket addresses.
    pub fn new(base_addr: SocketAddr, testing_addr: Option<SocketAddr>) -> Self {
        let base_url =
            Url::parse(&format!("http://{base_addr}")).expect("Valid base address for node");
        let testing_url = testing_addr
            .map(|addr| Url::parse(&format!("http://{addr}")).expect("Valid testing address"));
        Self::from_urls(base_url, testing_url)
    }

    #[must_use]
    /// Construct from prebuilt URLs.
    pub fn from_urls(base_url: Url, testing_url: Option<Url>) -> Self {
        let client = Client::new();
        Self {
            base_url,
            testing_url,
            http_client: CommonHttpClient::new_with_client(client.clone(), None),
            client,
        }
    }

    #[must_use]
    /// Testing URL, when built with testing features.
    pub fn testing_url(&self) -> Option<Url> {
        self.testing_url.clone()
    }

    /// Build a GET request against the base API.
    pub fn get_builder(&self, path: &str) -> RequestBuilder {
        self.client.get(self.join_base(path))
    }

    /// Issue a GET request against the base API.
    pub async fn get_response(&self, path: &str) -> reqwest::Result<Response> {
        self.client.get(self.join_base(path)).send().await
    }

    /// GET and decode JSON from the base API.
    pub async fn get_json<T>(&self, path: &str) -> reqwest::Result<T>
    where
        T: DeserializeOwned,
    {
        self.get_response(path)
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// POST JSON to the base API and decode a response.
    pub async fn post_json_decode<T, R>(&self, path: &str, body: &T) -> reqwest::Result<R>
    where
        T: Serialize + Sync + ?Sized,
        R: DeserializeOwned,
    {
        self.post_json_response(path, body)
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// POST JSON to the base API and return the raw response.
    pub async fn post_json_response<T>(&self, path: &str, body: &T) -> reqwest::Result<Response>
    where
        T: Serialize + Sync + ?Sized,
    {
        self.client
            .post(self.join_base(path))
            .json(body)
            .send()
            .await
    }

    /// POST JSON to the base API and expect a success status.
    pub async fn post_json_unit<T>(&self, path: &str, body: &T) -> reqwest::Result<()>
    where
        T: Serialize + Sync + ?Sized,
    {
        self.post_json_response(path, body)
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// GET and decode JSON from the testing API.
    pub async fn get_testing_json<T>(&self, path: &str) -> reqwest::Result<T>
    where
        T: DeserializeOwned,
    {
        self.get_testing_response(path)
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// POST JSON to the testing API and decode a response.
    pub async fn post_testing_json_decode<T, R>(&self, path: &str, body: &T) -> reqwest::Result<R>
    where
        T: Serialize + Sync + ?Sized,
        R: DeserializeOwned,
    {
        self.post_testing_json_response(path, body)
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// POST JSON to the testing API and expect a success status.
    pub async fn post_testing_json_unit<T>(&self, path: &str, body: &T) -> reqwest::Result<()>
    where
        T: Serialize + Sync + ?Sized,
    {
        self.post_testing_json_response(path, body)
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// POST JSON to the testing API and return the raw response.
    pub async fn post_testing_json_response<T>(
        &self,
        path: &str,
        body: &T,
    ) -> reqwest::Result<Response>
    where
        T: Serialize + Sync + ?Sized,
    {
        let testing_url = self
            .testing_url
            .as_ref()
            .expect(DA_GET_TESTING_ENDPOINT_ERROR);
        self.client
            .post(Self::join_url(testing_url, path))
            .json(body)
            .send()
            .await
    }

    /// GET from the testing API and return the raw response.
    pub async fn get_testing_response(&self, path: &str) -> reqwest::Result<Response> {
        let testing_url = self
            .testing_url
            .as_ref()
            .expect(DA_GET_TESTING_ENDPOINT_ERROR);
        self.client
            .get(Self::join_url(testing_url, path))
            .send()
            .await
    }

    /// Block a peer via the DA testing API.
    pub async fn block_peer(&self, peer_id: &str) -> reqwest::Result<bool> {
        self.post_json_decode(DA_BLOCK_PEER, &peer_id).await
    }

    /// Unblock a peer via the DA testing API.
    pub async fn unblock_peer(&self, peer_id: &str) -> reqwest::Result<bool> {
        self.post_json_decode(DA_UNBLOCK_PEER, &peer_id).await
    }

    /// Fetch the list of blacklisted peers.
    pub async fn blacklisted_peers(&self) -> reqwest::Result<Vec<String>> {
        self.get_json(DA_BLACKLISTED_PEERS).await
    }

    /// Fetch balancer stats from DA API.
    pub async fn balancer_stats(&self) -> reqwest::Result<BalancerStats> {
        self.get_json(DA_BALANCER_STATS).await
    }

    /// Fetch monitor stats from DA API.
    pub async fn monitor_stats(&self) -> reqwest::Result<MonitorStats> {
        self.get_json(DA_MONITOR_STATS).await
    }

    /// Fetch consensus info from the base API.
    pub async fn consensus_info(&self) -> reqwest::Result<CryptarchiaInfo> {
        self.get_json(CRYPTARCHIA_INFO).await
    }

    /// Fetch libp2p network info.
    pub async fn network_info(&self) -> reqwest::Result<Libp2pInfo> {
        self.get_json(NETWORK_INFO).await
    }

    /// Fetch a block by hash from storage.
    pub async fn storage_block(
        &self,
        id: &HeaderId,
    ) -> reqwest::Result<Option<Block<SignedMantleTx>>> {
        self.post_json_decode(STORAGE_BLOCK, id).await
    }

    /// Fetch header ids between optional bounds.
    /// When `from` is None, defaults to tip; when `to` is None, defaults to
    /// LIB.
    pub async fn consensus_headers(
        &self,
        from: Option<HeaderId>,
        to: Option<HeaderId>,
    ) -> reqwest::Result<Vec<HeaderId>> {
        let mut url = self.join_base(CRYPTARCHIA_HEADERS);
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(from) = from {
                let bytes: [u8; 32] = from.into();
                pairs.append_pair("from", &hex::encode(bytes));
            }
            if let Some(to) = to {
                let bytes: [u8; 32] = to.into();
                pairs.append_pair("to", &hex::encode(bytes));
            }
        }
        self.client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// Query DA membership via testing API.
    pub async fn da_get_membership(
        &self,
        session_id: &SessionNumber,
    ) -> reqwest::Result<MembershipResponse> {
        self.post_testing_json_decode(DA_GET_MEMBERSHIP, session_id)
            .await
    }

    /// Query historic sampling via testing API.
    pub async fn da_historic_sampling(
        &self,
        request: &HistoricSamplingRequest<BlobId>,
    ) -> reqwest::Result<bool> {
        self.post_testing_json_decode(DA_HISTORIC_SAMPLING, request)
            .await
    }

    /// Submit a mantle transaction through the base API.
    pub async fn submit_transaction(&self, tx: &SignedMantleTx) -> reqwest::Result<()> {
        self.post_json_unit(MEMPOOL_ADD_TX, tx).await
    }

    /// Execute a custom request built by the caller.
    pub async fn get_headers_raw(&self, builder: RequestBuilder) -> reqwest::Result<Response> {
        builder.send().await
    }

    /// Fetch raw mempool metrics from the testing endpoint.
    pub async fn mempool_metrics(&self, pool: &str) -> reqwest::Result<Value> {
        self.get_json(&format!("/{pool}/metrics")).await
    }

    #[must_use]
    /// Base API URL.
    pub const fn base_url(&self) -> &Url {
        &self.base_url
    }

    #[must_use]
    /// Underlying common HTTP client wrapper.
    pub const fn http_client(&self) -> &CommonHttpClient {
        &self.http_client
    }

    fn join_base(&self, path: &str) -> Url {
        Self::join_url(&self.base_url, path)
    }

    fn join_url(base: &Url, path: &str) -> Url {
        let trimmed = path.trim_start_matches('/');
        base.join(trimmed).expect("valid relative path")
    }
}
