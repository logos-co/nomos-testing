use std::{collections::HashMap, net::Ipv4Addr, str::FromStr as _};

use nomos_core::mantle::GenesisTx as _;
use nomos_libp2p::{Multiaddr, PeerId, ed25519};
use nomos_tracing_service::TracingSettings;
use nomos_utils::net::get_available_udp_port;
use rand::{Rng as _, thread_rng};
use testing_framework_config::topology::configs::{
    GeneralConfig,
    api::GeneralApiConfig,
    blend,
    blend::create_blend_configs,
    bootstrap,
    bootstrap::{SHORT_PROLONGED_BOOTSTRAP_PERIOD, create_bootstrap_configs},
    consensus,
    consensus::{ConsensusParams, create_consensus_configs, create_genesis_tx_with_declarations},
    da,
    da::{DaParams, create_da_configs},
    network,
    network::{NetworkParams, create_network_configs},
    time::default_time_config,
    wallet::WalletConfig,
};

use crate::{
    config::{
        kms::create_kms_configs, providers::create_providers, tracing::update_tracing_identifier,
        validation::validate_inputs,
    },
    host::{Host, HostKind, sort_hosts},
    network::rewrite_initial_peers,
};

#[must_use]
pub fn create_node_configs(
    consensus_params: &ConsensusParams,
    da_params: &DaParams,
    tracing_settings: &TracingSettings,
    wallet_config: &WalletConfig,
    ids: Option<Vec<[u8; 32]>>,
    da_ports: Option<Vec<u16>>,
    blend_ports: Option<Vec<u16>>,
    hosts: Vec<Host>,
) -> HashMap<Host, GeneralConfig> {
    let hosts = sort_hosts(hosts);

    validate_inputs(
        &hosts,
        consensus_params,
        ids.as_ref(),
        da_ports.as_ref(),
        blend_ports.as_ref(),
    )
    .expect("invalid cfgsync inputs");

    let ids = generate_ids(consensus_params.n_participants, ids);
    let ports = resolve_da_ports(consensus_params.n_participants, da_ports);
    let blend_ports = resolve_blend_ports(&hosts, blend_ports);

    let BaseConfigs {
        mut consensus_configs,
        bootstrap_configs,
        da_configs,
        network_configs,
        blend_configs,
    } = build_base_configs(
        consensus_params,
        da_params,
        wallet_config,
        &ids,
        &ports,
        &blend_ports,
    );
    let api_configs = build_api_configs(&hosts);
    let mut configured_hosts = HashMap::new();

    let initial_peer_templates: Vec<Vec<Multiaddr>> = network_configs
        .iter()
        .map(|cfg| cfg.backend.initial_peers.clone())
        .collect();
    let original_network_ports: Vec<u16> = network_configs
        .iter()
        .map(|cfg| cfg.backend.swarm.port)
        .collect();
    let peer_ids = build_peer_ids(&ids);

    let host_network_init_peers = rewrite_initial_peers(
        &initial_peer_templates,
        &original_network_ports,
        &hosts,
        &peer_ids,
    );

    let providers = create_providers(&hosts, &consensus_configs, &blend_configs, &da_configs);

    // Update genesis TX to contain Blend and DA providers.
    let ledger_tx = consensus_configs[0]
        .genesis_tx
        .mantle_tx()
        .ledger_tx
        .clone();
    let genesis_tx = create_genesis_tx_with_declarations(ledger_tx, providers);
    for c in &mut consensus_configs {
        c.genesis_tx = genesis_tx.clone();
    }

    // Set Blend and DA keys in KMS of each node config.
    let kms_configs = create_kms_configs(&blend_configs, &da_configs);

    for (i, host) in hosts.into_iter().enumerate() {
        let consensus_config = consensus_configs[i].clone();
        let api_config = api_configs[i].clone();

        // DA Libp2p network config.
        let mut da_config = da_configs[i].clone();
        da_config.listening_address = Multiaddr::from_str(&format!(
            "/ip4/0.0.0.0/udp/{}/quic-v1",
            host.da_network_port,
        ))
        .unwrap();
        if matches!(host.kind, HostKind::Validator) {
            da_config.policy_settings.min_dispersal_peers = 0;
        }

        // Libp2p network config.
        let mut network_config = network_configs[i].clone();
        network_config.backend.swarm.host = Ipv4Addr::from_str("0.0.0.0").unwrap();
        network_config.backend.swarm.port = host.network_port;
        network_config.backend.initial_peers = host_network_init_peers[i].clone();
        network_config.backend.swarm.nat_config = nomos_libp2p::NatSettings::Static {
            external_address: Multiaddr::from_str(&format!(
                "/ip4/{}/udp/{}/quic-v1",
                host.ip, host.network_port
            ))
            .unwrap(),
        };

        // Blend network config.
        let mut blend_config = blend_configs[i].clone();
        blend_config.backend_core.listening_address =
            Multiaddr::from_str(&format!("/ip4/0.0.0.0/udp/{}/quic-v1", host.blend_port)).unwrap();

        let tracing_config =
            update_tracing_identifier(tracing_settings.clone(), host.identifier.clone());
        let time_config = default_time_config();

        configured_hosts.insert(
            host.clone(),
            GeneralConfig {
                consensus_config,
                bootstrapping_config: bootstrap_configs[i].clone(),
                da_config,
                network_config,
                blend_config,
                api_config,
                tracing_config,
                time_config,
                kms_config: kms_configs[i].clone(),
            },
        );
    }

    configured_hosts
}

fn generate_ids(count: usize, ids: Option<Vec<[u8; 32]>>) -> Vec<[u8; 32]> {
    ids.unwrap_or_else(|| {
        let mut generated = vec![[0; 32]; count];
        for id in &mut generated {
            thread_rng().fill(id);
        }
        generated
    })
}

fn resolve_da_ports(count: usize, da_ports: Option<Vec<u16>>) -> Vec<u16> {
    da_ports.unwrap_or_else(|| {
        (0..count)
            .map(|_| get_available_udp_port().unwrap())
            .collect()
    })
}

fn resolve_blend_ports(hosts: &[Host], blend_ports: Option<Vec<u16>>) -> Vec<u16> {
    blend_ports.unwrap_or_else(|| hosts.iter().map(|h| h.blend_port).collect())
}

fn build_base_configs(
    consensus_params: &ConsensusParams,
    da_params: &DaParams,
    wallet_config: &WalletConfig,
    ids: &[[u8; 32]],
    da_ports: &[u16],
    blend_ports: &[u16],
) -> BaseConfigs {
    BaseConfigs {
        consensus_configs: create_consensus_configs(ids, consensus_params, wallet_config),
        bootstrap_configs: create_bootstrap_configs(ids, SHORT_PROLONGED_BOOTSTRAP_PERIOD),
        da_configs: create_da_configs(ids, da_params, da_ports),
        network_configs: create_network_configs(ids, &NetworkParams::default()),
        blend_configs: create_blend_configs(ids, blend_ports),
    }
}

fn build_api_configs(hosts: &[Host]) -> Vec<GeneralApiConfig> {
    hosts
        .iter()
        .map(|host| GeneralApiConfig {
            address: format!("0.0.0.0:{}", host.api_port).parse().unwrap(),
            testing_http_address: format!("0.0.0.0:{}", host.testing_http_port)
                .parse()
                .unwrap(),
        })
        .collect::<Vec<_>>()
}

fn build_peer_ids(ids: &[[u8; 32]]) -> Vec<PeerId> {
    ids.iter()
        .map(|bytes| {
            let mut key_bytes = *bytes;
            let secret =
                ed25519::SecretKey::try_from_bytes(&mut key_bytes).expect("valid ed25519 key");
            PeerId::from_public_key(&ed25519::Keypair::from(secret).public().into())
        })
        .collect()
}

struct BaseConfigs {
    consensus_configs: Vec<consensus::GeneralConsensusConfig>,
    bootstrap_configs: Vec<bootstrap::GeneralBootstrapConfig>,
    da_configs: Vec<da::GeneralDaConfig>,
    network_configs: Vec<network::GeneralNetworkConfig>,
    blend_configs: Vec<blend::GeneralBlendConfig>,
}
