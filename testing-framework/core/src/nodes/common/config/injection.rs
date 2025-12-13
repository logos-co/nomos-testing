use serde_yaml::{Mapping, Number as YamlNumber, Value};

/// Convert any ed25519_sig entries from a YAML sequence of bytes into the
/// hex-encoded string format expected by nomos-node config deserialization.
pub fn normalize_ed25519_sigs(value: &mut Value) {
    match value {
        Value::Mapping(map) => {
            for (k, v) in map.iter_mut() {
                if let Value::String(key) = k {
                    if key == "ed25519_sig" {
                        if let Value::Sequence(seq) = v {
                            let bytes: Option<Vec<u8>> = seq
                                .iter()
                                .map(|val| val.as_i64().and_then(|n| u8::try_from(n).ok()))
                                .collect();
                            if let Some(bytes) = bytes {
                                *v = Value::String(hex::encode(bytes));
                                continue;
                            }
                        }
                    }
                }
                normalize_ed25519_sigs(v);
            }
        }
        Value::Sequence(seq) => seq.iter_mut().for_each(normalize_ed25519_sigs),
        Value::Tagged(tagged) => normalize_ed25519_sigs(&mut tagged.value),
        _ => {}
    }
}

/// Inject cryptarchia/IBD defaults into a YAML config in-place.
pub fn inject_ibd_into_cryptarchia(yaml_value: &mut Value) {
    let Some(cryptarchia) = cryptarchia_section(yaml_value) else {
        return;
    };
    ensure_network_adapter(cryptarchia);
    ensure_sync_defaults(cryptarchia);
    ensure_ibd_bootstrap(cryptarchia);
    normalize_ed25519_sigs(yaml_value);
}

fn cryptarchia_section(yaml_value: &mut Value) -> Option<&mut Mapping> {
    yaml_value
        .as_mapping_mut()
        .and_then(|root| root.get_mut(&Value::String("cryptarchia".into())))
        .and_then(Value::as_mapping_mut)
}

fn ensure_network_adapter(cryptarchia: &mut Mapping) {
    if cryptarchia.contains_key(&Value::String("network_adapter_settings".into())) {
        return;
    }
    let mut network = Mapping::new();
    network.insert(
        Value::String("topic".into()),
        Value::String("/cryptarchia/proto".into()),
    );
    cryptarchia.insert(
        Value::String("network_adapter_settings".into()),
        Value::Mapping(network),
    );
}

fn ensure_sync_defaults(cryptarchia: &mut Mapping) {
    if cryptarchia.contains_key(&Value::String("sync".into())) {
        return;
    }
    let mut orphan = Mapping::new();
    orphan.insert(
        Value::String("max_orphan_cache_size".into()),
        Value::Number(YamlNumber::from(5)),
    );
    let mut sync = Mapping::new();
    sync.insert(Value::String("orphan".into()), Value::Mapping(orphan));
    cryptarchia.insert(Value::String("sync".into()), Value::Mapping(sync));
}

fn ensure_ibd_bootstrap(cryptarchia: &mut Mapping) {
    let Some(bootstrap) = cryptarchia
        .get_mut(&Value::String("bootstrap".into()))
        .and_then(Value::as_mapping_mut)
    else {
        return;
    };

    let ibd_key = Value::String("ibd".into());
    if bootstrap.contains_key(&ibd_key) {
        return;
    }

    let mut ibd = Mapping::new();
    ibd.insert(Value::String("peers".into()), Value::Sequence(vec![]));

    bootstrap.insert(ibd_key, Value::Mapping(ibd));
}
