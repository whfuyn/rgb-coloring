use rgbstd::{
    vm::{
        WitnessOrd,
        XWitnessTx,
        WitnessPos,
    },
    validation::{
        ResolveWitness,
        WitnessResolverError,
    },
    XChain,
};
use bp::Tx;
use bp::Txid;
use std::collections::HashMap;

#[derive(Default, Debug)]
pub struct LnResolver {
    // Local known on-chain txs.
    // txid => (tx, (height, timestamp))
    local_txs: HashMap<Txid, (Tx, WitnessPos)>,

    // Channel state tx
    active: Option<Tx>,
    archived: HashMap<Txid, Tx>,
}

impl LnResolver {
    pub fn new() -> Self {
        Self::default()
    }

    // Timestamp must be greater than or equal to 1231006505
    pub fn add_tx(
        &mut self,
        tx: Tx,
        height: u32,
        timestamp: i64,
    ) {
        let witness_pos = WitnessPos::new(height, timestamp).unwrap();
        self.local_txs.insert(tx.txid(), (tx, witness_pos));
    }

}

impl ResolveWitness for LnResolver {
    fn resolve_pub_witness(
        &self,
        witness_id: rgbstd::XWitnessId,
    ) -> Result<rgbstd::vm::XWitnessTx, WitnessResolverError> {
        let txid = witness_id.as_reduced_unsafe();

        if let Some((tx, _)) = self.local_txs.get(txid) {
            return Ok(XChain::Bitcoin(tx.clone()));
        }

        if let Some(ref tx) = self.active {
            if &tx.txid() == txid {
                return Ok(XChain::Bitcoin(tx.clone()));
            }
        }

        if let Some(tx) = self.archived.get(txid) {
            return Ok(XChain::Bitcoin(tx.clone()));
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }

    fn resolve_pub_witness_ord(
        &self,
        witness_id: rgbstd::XWitnessId,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        let txid = witness_id.as_reduced_unsafe();

        if let Some((_, witness_pos)) = self.local_txs.get(txid) {
            return Ok(WitnessOrd::Mined(*witness_pos));
        }

        if let Some(ref tx) = self.active {
            if &tx.txid() == txid {
                return Ok(WitnessOrd::Tentative)
            }
        }

        if self.archived.contains_key(txid) {
            return Ok(WitnessOrd::Archived)
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }

}

