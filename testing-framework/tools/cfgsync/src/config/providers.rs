use std::str::FromStr;

use nomos_core::sdp::{Locator, ServiceType};
use nomos_libp2p::Multiaddr;
use testing_framework_config::topology::configs::{
    blend::GeneralBlendConfig,
    consensus::{GeneralConsensusConfig, ProviderInfo},
    da::GeneralDaConfig,
};

use crate::host::Host;

pub fn create_providers(
    hosts: &[Host],
    consensus_configs: &[GeneralConsensusConfig],
    blend_configs: &[GeneralBlendConfig],
    da_configs: &[GeneralDaConfig],
) -> Vec<ProviderInfo> {
    let mut providers: Vec<_> = da_configs
        .iter()
        .enumerate()
        .map(|(i, da_conf)| ProviderInfo {
            service_type: ServiceType::DataAvailability,
            provider_sk: da_conf.signer.clone(),
            zk_sk: da_conf.secret_zk_key.clone(),
            locator: Locator(
                Multiaddr::from_str(&format!(
                    "/ip4/{}/udp/{}/quic-v1",
                    hosts[i].ip, hosts[i].da_network_port
                ))
                .unwrap(),
            ),
            note: consensus_configs[0].da_notes[i].clone(),
        })
        .collect();
    providers.extend(blend_configs.iter().enumerate().map(|(i, blend_conf)| {
        ProviderInfo {
            service_type: ServiceType::BlendNetwork,
            provider_sk: blend_conf.signer.clone(),
            zk_sk: blend_conf.secret_zk_key.clone(),
            locator: Locator(
                Multiaddr::from_str(&format!(
                    "/ip4/{}/udp/{}/quic-v1",
                    hosts[i].ip, hosts[i].blend_port
                ))
                .unwrap(),
            ),
            note: consensus_configs[0].blend_notes[i].clone(),
        }
    }));

    providers
}
