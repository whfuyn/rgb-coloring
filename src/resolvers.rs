use std::collections::HashMap;

use rgbstd::{
    containers::{Consignment, PubWitness},
    validation::{
        ResolveWitness,
        WitnessResolverError,
    },
    vm::{
        WitnessOrd, WitnessPos,
    },
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
        let height = std::num::NonZeroU32::new(height).unwrap();
        let tx = Tx::consensus_deserialize(consensus_serialized_tx).unwrap();
        let witness_pos = WitnessPos::bitcoin(height, timestamp).unwrap();
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
        witness_id: rgbstd::Txid,
    ) -> Result<bp::Tx, WitnessResolverError> {
        if let Some((tx, _)) = self.local_txs.get(&witness_id) {
            return Ok(tx.clone());
        }

        if let Some(ref tx) = self.active_tx {
            if tx.txid() == witness_id {
                return Ok(tx.clone());
            }
        }

        if let Some(tx) = self.archived_txs.get(&witness_id) {
            return Ok(tx.clone());
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }

    fn resolve_pub_witness_ord(
        &self,
        witness_id: Txid,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        if let Some((_, witness_pos)) = self.local_txs.get(&witness_id) {
            return Ok(WitnessOrd::Mined(*witness_pos));
        }

        if let Some(ref tx) = self.active_tx {
            if tx.txid() == witness_id {
                return Ok(WitnessOrd::Tentative)
            }
        }

        if self.archived_txs.contains_key(&witness_id) {
            return Ok(WitnessOrd::Archived)
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }

    fn check_chain_net(&self, chain_net: rgbstd::ChainNet) -> Result<(), WitnessResolverError> {
        // TODO
        let _ = chain_net;
        Ok(())
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
                .filter_map(|bw| {
                    match bw.pub_witness.clone() {
                        PubWitness::Tx(tx) => Some((tx.txid(), tx)),
                        _ => None,
                    }
                })
        );
    }
}

impl ResolveWitness for LocalResolver {
    fn resolve_pub_witness(
        &self,
        witness_id: Txid,
    ) -> Result<Tx, WitnessResolverError> {
        if let Some(tx) = self.terminal_txes.get(&witness_id) {
            return Ok(tx.clone());
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }

    fn resolve_pub_witness_ord(
        &self,
        witness_id: Txid,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        if self.terminal_txes.contains_key(&witness_id) {
            return Ok(WitnessOrd::Tentative);
        }

        return Err(WitnessResolverError::Unknown(witness_id));
    }

    fn check_chain_net(&self, chain_net: rgbstd::ChainNet) -> Result<(), WitnessResolverError> {
        let _ = chain_net;
        Ok(())
    }
}


#[derive(Default, Debug)]
pub struct WithLocalResolver<T: ResolveWitness> {
    inner: T,
    terminal_txes: HashMap<Txid, Tx>,
}

impl<T: ResolveWitness> WithLocalResolver<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            terminal_txes: HashMap::new(),
        }
    }

    pub fn add_witness(&mut self, witness: Tx) {
        self.terminal_txes.insert(witness.txid(), witness);
    }

    pub fn add_terminals<const TYPE: bool>(&mut self, consignment: &Consignment<TYPE>) {
        self.terminal_txes.extend(
            consignment
                .bundles
                .iter()
                .filter_map(|bw| {
                    match bw.pub_witness.clone() {
                        PubWitness::Tx(tx) => Some((tx.txid(), tx)),
                        _ => None,
                    }
                })
        );
    }

    pub fn add_pending_tx_from_consignment<const TYPE: bool>(&mut self, txid: Txid, consignment: &Consignment<TYPE>) {
        self.terminal_txes.extend(
            consignment
                .bundles
                .iter()
                .find_map(|bw| {
                    match bw.pub_witness.clone() {
                        PubWitness::Tx(tx) if tx.txid() == txid => Some((tx.txid(), tx)),
                        _ => None,
                    }
                })
        );
    }
}

impl<T: ResolveWitness> ResolveWitness for WithLocalResolver<T> {
    fn resolve_pub_witness(
        &self,
        witness_id: Txid,
    ) -> Result<Tx, WitnessResolverError> {
        if let Some(tx) = self.terminal_txes.get(&witness_id) {
            return Ok(tx.clone());
        }
        self.inner.resolve_pub_witness(witness_id)
    }

    fn resolve_pub_witness_ord(
        &self,
        witness_id: Txid,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        if self.terminal_txes.contains_key(&witness_id) {
            return Ok(WitnessOrd::Tentative);
        }
        self.inner.resolve_pub_witness_ord(witness_id)
    }

    fn check_chain_net(&self, chain_net: rgbstd::ChainNet) -> Result<(), WitnessResolverError> {
        self.inner.check_chain_net(chain_net)
    }
}


#[derive(Debug)]
pub enum GlobalResolver {
    Online(OnlineResolver),
    Local(LocalResolver),
}

impl GlobalResolver {
    pub fn new_online(esplora_url: &str) -> Self {
        Self::Online(OnlineResolver::new(esplora_url))
    }

    pub fn new_local(local_resolver: LocalResolver) -> Self {
        Self::Local(local_resolver)
    }
}

impl ResolveWitness for GlobalResolver {
    fn resolve_pub_witness(
        &self,
        witness_id: Txid,
    ) -> Result<Tx, WitnessResolverError> {
        match self {
            Self::Online(resolver) => resolver.resolve_pub_witness(witness_id),
            Self::Local(resolver) => resolver.resolve_pub_witness(witness_id),
        }
    }

    fn resolve_pub_witness_ord(
        &self,
        witness_id: Txid,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        match self {
            Self::Online(resolver) => resolver.resolve_pub_witness_ord(witness_id),
            Self::Local(resolver) => resolver.resolve_pub_witness_ord(witness_id),
        }
    }

    fn check_chain_net(&self, chain_net: rgbstd::ChainNet) -> Result<(), WitnessResolverError> {
        match self {
            Self::Online(resolver) => resolver.check_chain_net(chain_net),
            Self::Local(resolver) => resolver.check_chain_net(chain_net),
        }
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
        witness_id: Txid,
    ) -> Result<Tx, WitnessResolverError> {
        let txid = witness_id
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
            .and_then(|r| r.ok_or(WitnessResolverError::Unknown(witness_id)));

        op.retry(default_backoff()).call()
    }

    fn resolve_pub_witness_ord(
        &self,
        witness_id: Txid,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        let txid = witness_id
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
                    
                    let h = std::num::NonZeroU32::new(h).ok_or_else(|| WitnessResolverError::Other(witness_id, "Invalid block height".to_string()))?;
                    let pos = WitnessPos::bitcoin(h, t as i64)
                        .ok_or_else(|| WitnessResolverError::Other(witness_id, "Invalid server data".to_string()))?;
                    WitnessOrd::Mined(pos)
                }
                None => WitnessOrd::Tentative,
            };
            Ok(ord)
        };
        op.retry(default_backoff()).call()
    }

    fn check_chain_net(&self, chain_net: rgbstd::ChainNet) -> Result<(), WitnessResolverError> {
        // TODO
        let _ = chain_net;
        Ok(())
    }
}

/// Unchecked fascia resolver
pub struct FasciaResolver;

impl ResolveWitness for FasciaResolver {
    fn resolve_pub_witness(
        &self,
        _: Txid,
    ) -> Result<Tx, WitnessResolverError> {
        unreachable!()
    }

    fn resolve_pub_witness_ord(
        &self,
        _witness_id: Txid,
    ) -> Result<WitnessOrd, WitnessResolverError> {
        Ok(WitnessOrd::Tentative)
    }

    fn check_chain_net(&self, _chain_net: rgbstd::ChainNet) -> Result<(), WitnessResolverError> {
        unreachable!()
    }
}


fn default_backoff() -> ExponentialBuilder {
    ExponentialBuilder::default()
}