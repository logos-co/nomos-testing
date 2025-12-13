use key_management_system_service::keys::{Ed25519Key, ZkKey};
use nomos_core::mantle::{
    MantleTx, Op, OpProof, SignedMantleTx, Transaction as _,
    ledger::Tx as LedgerTx,
    ops::channel::{ChannelId, MsgId, inscribe::InscriptionOp},
};

/// Builds a signed inscription transaction with deterministic payload for
/// testing.
#[must_use]
pub fn create_inscription_transaction_with_id(id: ChannelId) -> SignedMantleTx {
    let signing_key = Ed25519Key::from_bytes(&[0u8; 32]);
    let signer = signing_key.public_key();

    let inscription_op = InscriptionOp {
        channel_id: id,
        inscription: format!("Test channel inscription {id:?}").into_bytes(),
        parent: MsgId::root(),
        signer,
    };

    let mantle_tx = MantleTx {
        ops: vec![Op::ChannelInscribe(inscription_op)],
        ledger_tx: LedgerTx::new(vec![], vec![]),
        storage_gas_price: 0,
        execution_gas_price: 0,
    };

    let tx_hash = mantle_tx.hash();
    let signature = signing_key.sign_payload(tx_hash.as_signing_bytes().as_ref());
    let zk_key = ZkKey::zero();
    tracing::debug!(channel = ?id, tx_hash = ?tx_hash, "building inscription transaction");

    SignedMantleTx::new(
        mantle_tx,
        vec![OpProof::Ed25519Sig(signature)],
        ZkKey::multi_sign(&[zk_key], tx_hash.as_ref()).expect("zk signature generation"),
    )
    .expect("valid transaction")
}
