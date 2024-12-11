use std::time::Duration;
use std::collections::HashMap;

use rgbstd::{
    containers::Consignment, validation::{
        ResolveWitness,
        WitnessResolverError,
    }, vm::{
        WitnessOrd, WitnessPos, XWitnessTx
    }, XChain, XWitnessId
};
use bp::{ConsensusDecode, ConsensusEncode, Tx};
use bp::Txid;

use backon::{
    BlockingRetryable,
    ExponentialBuilder,
};


// TODO: maybe remove this resolver.
#[derive(Default, Debug)]
pub struct LnResolver {
    // Local known on-chain txs.
    // txid => (tx, (height, timestamp))
    local_txs: HashMap<Txid, (Tx, WitnessPos)>,

    // Channel state tx
    active_tx: Option<Tx>,
    archived_txs: HashMap<Txid, Tx>,
}

impl LnResolver {
    pub fn new() -> Self {
        Self::default()
    }

    // Timestamp must be greater than or equal to 1231006505
    pub fn add_onchain_tx(
        &mut self,
        consensus_serialized_tx: &[u8],
        height: u32,
        timestamp: i64,
    ) {
        let tx = Tx::consensus_deserialize(consensus_serialized_tx).unwrap();
        let witness_pos = WitnessPos::new(height, timestamp).unwrap();
        self.local_txs.insert(tx.txid(), (tx, witness_pos));
    }

    pub fn replace_active(
        &mut self,
        consensus_serialized_tx: &[u8],
    ) {
        let tx = Tx::consensus_deserialize(consensus_serialized_tx).unwrap();

        if let Some(old) = self.active_tx.replace(tx) {
            let old_txid = old.txid();
            self.archived_txs.insert(old_txid, old);
        }
    }

    pub fn get_consensus_serialized_active_tx(&self) -> Option<Vec<u8>> {
        self.active_tx.as_ref().map(|tx| tx.consensus_serialize())
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

        if let Some(ref tx) = self.active_tx {
            if &tx.txid() == txid {
                return Ok(XChain::Bitcoin(tx.clone()));
            }
        }

        if let Some(tx) = self.archived_txs.get(txid) {
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

        if let Some(ref tx) = self.active_tx {
            if &tx.txid() == txid {
                return Ok(WitnessOrd::Tentative)
            }
        }

        if self.archived_txs.contains_key(txid) {
            return Ok(WitnessOrd::Archived)
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }

}


#[derive(Default, Debug)]
pub struct LocalResolver {
    terminal_txes: HashMap<Txid, Tx>,
}

impl LocalResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_terminals<const TYPE: bool>(&mut self, consignment: &Consignment<TYPE>) {
        self.terminal_txes.extend(
            consignment
                .bundles
                .iter()
                .filter_map(|bw| bw.pub_witness.maybe_map_ref(|w| w.tx().cloned()))
                .filter_map(|tx| match tx {
                    XChain::Bitcoin(tx) => Some(tx),
                    XChain::Liquid(_) | XChain::Other(_) => None,
                })
                .map(|tx| (tx.txid(), tx)),
        );
    }
}

impl ResolveWitness for LocalResolver {
    fn resolve_pub_witness(
        &self,
        witness_id: XWitnessId,
    ) -> Result<XWitnessTx, WitnessResolverError> {
        let XWitnessId::Bitcoin(txid) = witness_id else {
            return Err(WitnessResolverError::Other(
                witness_id,
                format!("{} is not supported as layer 1 network", witness_id.layer1()),
            ));
        };

        if let Some(tx) = self.terminal_txes.get(&txid) {
            return Ok(XWitnessTx::Bitcoin(tx.clone()));
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }

    fn resolve_pub_witness_ord(
        &self,
        witness_id: XWitnessId,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        let XWitnessId::Bitcoin(txid) = witness_id else {
            return Err(WitnessResolverError::Other(
                witness_id,
                format!("{} is not supported as layer 1 network", witness_id.layer1()),
            ));
        };

        if self.terminal_txes.contains_key(&txid) {
            return Ok(WitnessOrd::Tentative);
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }
}


#[derive(Debug)]
pub struct OnlineResolver {
    // TODO
    #[allow(unused)]
    esplora_url: String,
    client: esplora_client::BlockingClient,
}

impl OnlineResolver {
    pub fn new(esplora_url: &str) -> Self {
        let builder = esplora_client::Builder::new(esplora_url);

        Self {
            esplora_url: esplora_url.to_string(),
            client: builder.build_blocking(),
        }
    }
}

impl ResolveWitness for OnlineResolver {
    fn resolve_pub_witness(
        &self,
        witness_id: XWitnessId,
    ) -> Result<XWitnessTx, WitnessResolverError> {
        let XWitnessId::Bitcoin(txid) = witness_id else {
            return Err(WitnessResolverError::Other(
                witness_id,
                format!("{} is not supported as layer 1 network", witness_id.layer1()),
            ));
        };

        let txid = txid
            .to_string()
            .parse()
            .unwrap();

        let op = || self
            .client
            .get_tx(&txid)
            .map(|tx_opt| {
                tx_opt.map(|tx| {
                    use bitcoin::consensus::Encodable;

                    let mut buf = Vec::new();
                    tx.consensus_encode(&mut buf).unwrap();
                    Tx::consensus_deserialize(&buf).unwrap()
                })
            })
            .map_err(|e| WitnessResolverError::Other(witness_id, e.to_string()))
            .and_then(|r| r.ok_or(WitnessResolverError::Unknown(witness_id)))
            .map(XChain::Bitcoin);

        op.retry(default_backoff()).call()
    }

    fn resolve_pub_witness_ord(
        &self,
        witness_id: XWitnessId,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        let XWitnessId::Bitcoin(txid) = witness_id else {
            return Err(WitnessResolverError::Other(
                witness_id,
                format!("{} is not supported as layer 1 network", witness_id.layer1()),
            ));
        };
        let txid = txid
            .to_string()
            .parse()
            .unwrap();

        let op = || {
            let tx_opt = self
                .client
                .get_tx(&txid)
                .map_err(|e| WitnessResolverError::Other(witness_id, e.to_string()))?;
            if tx_opt.is_none() {
                return Ok(WitnessOrd::Archived);
            }

            let status = self.client.get_tx_status(&txid)
                .map_err(|e| WitnessResolverError::Other(witness_id, e.to_string()))?;
            let ord = match status
                .block_height
                .and_then(|h| status.block_time.map(|t| (h, t)))
            {
                Some((h, t)) => {
                    let pos = WitnessPos::new(h, t as i64)
                        .ok_or_else(|| WitnessResolverError::Other(witness_id, "Invalid server data".to_string()))?;
                    WitnessOrd::Mined(pos)
                }
                None => WitnessOrd::Tentative,
            };
            Ok(ord)
        };
        op.retry(default_backoff()).call()
    }
}

/// Unchecked fascia resolver
pub struct FasciaResolver;

impl ResolveWitness for FasciaResolver {
    fn resolve_pub_witness(
        &self,
        _: XWitnessId,
    ) -> Result<XWitnessTx, WitnessResolverError> {
        unreachable!()
    }
    fn resolve_pub_witness_ord(
        &self,
        _witness_id: XWitnessId,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        Ok(WitnessOrd::Tentative)
    }
}


fn default_backoff() -> ExponentialBuilder {
    ExponentialBuilder::default()
}