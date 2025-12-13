use std::{collections::HashMap, iter};

use groth16::fr_to_bytes;
use key_management_system_service::{backend::preload::PreloadKMSBackendSettings, keys::Key};
use nomos_utils::net::get_available_udp_port;
use rand::{Rng, thread_rng};

use crate::topology::configs::{
    blend::GeneralBlendConfig, da::GeneralDaConfig, wallet::WalletAccount,
};

#[must_use]
/// Build preload KMS configs for blend/DA and wallet keys for every node.
pub fn create_kms_configs(
    blend_configs: &[GeneralBlendConfig],
    da_configs: &[GeneralDaConfig],
    wallet_accounts: &[WalletAccount],
) -> Vec<PreloadKMSBackendSettings> {
    da_configs
        .iter()
        .zip(blend_configs.iter())
        .map(|(da_conf, blend_conf)| {
            let mut keys = HashMap::from([
                (
                    hex::encode(blend_conf.signer.public_key().to_bytes()),
                    Key::Ed25519(blend_conf.signer.clone()),
                ),
                (
                    hex::encode(fr_to_bytes(
                        blend_conf.secret_zk_key.to_public_key().as_fr(),
                    )),
                    Key::Zk(blend_conf.secret_zk_key.clone()),
                ),
                (
                    hex::encode(da_conf.signer.public_key().to_bytes()),
                    Key::Ed25519(da_conf.signer.clone()),
                ),
                (
                    hex::encode(fr_to_bytes(da_conf.secret_zk_key.to_public_key().as_fr())),
                    Key::Zk(da_conf.secret_zk_key.clone()),
                ),
            ]);

            for account in wallet_accounts {
                let key_id = hex::encode(fr_to_bytes(account.public_key().as_fr()));
                keys.entry(key_id)
                    .or_insert_with(|| Key::Zk(account.secret_key.clone()));
            }

            PreloadKMSBackendSettings { keys }
        })
        .collect()
}

pub fn resolve_ids(ids: Option<Vec<[u8; 32]>>, count: usize) -> Vec<[u8; 32]> {
    ids.map_or_else(
        || {
            let mut generated = vec![[0; 32]; count];
            for id in &mut generated {
                thread_rng().fill(id);
            }
            generated
        },
        |ids| {
            assert_eq!(
                ids.len(),
                count,
                "expected {count} ids but got {}",
                ids.len()
            );
            ids
        },
    )
}

pub fn resolve_ports(ports: Option<Vec<u16>>, count: usize, label: &str) -> Vec<u16> {
    let resolved = ports.unwrap_or_else(|| {
        iter::repeat_with(|| get_available_udp_port().unwrap())
            .take(count)
            .collect()
    });
    assert_eq!(
        resolved.len(),
        count,
        "expected {count} {label} ports but got {}",
        resolved.len()
    );
    resolved
}

pub fn multiaddr_port(addr: &nomos_libp2p::Multiaddr) -> Option<u16> {
    for protocol in addr {
        match protocol {
            nomos_libp2p::Protocol::Udp(port) | nomos_libp2p::Protocol::Tcp(port) => {
                return Some(port);
            }
            _ => {}
        }
    }
    None
}
