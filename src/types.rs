// TODO: consider if it's a better option to wrap those types or we should just expose them

use std::collections::BTreeMap;
use std::str::FromStr;

pub(crate) use crate::detail::{
    Beneficiary as RawBeneficiary, RgbAssignments as RawRgbAssignments,
};
use rand::Rng;
pub(crate) use rgbstd::{
    containers::TransitionInfo as RawTransitionInfo, ContractId as RawContractId, Txid as RawTxid,
    XChain, XOutpoint as RawOutpoint, 
};
use serde::Deserialize;
use serde::Serialize;

pub trait ToRaw {
    type RawType;

    fn to_raw(self) -> Self::RawType;
}

macro_rules! impl_from_raw {
    ($ty: ty) => {
        impl std::convert::From<<$ty as ToRaw>::RawType> for $ty {
            fn from(value: <$ty as ToRaw>::RawType) -> Self {
                Self(value)
            }
        }
        
    };
}


#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct Txid(pub(crate) RawTxid);

impl From<[u8; 32]> for Txid {
    fn from(value: [u8; 32]) -> Self {
        Self(value.into())
    }
}

impl Into<[u8; 32]> for Txid {
    fn into(self) -> [u8; 32] {
        self.0.as_ref().to_byte_array()
    }
}

impl_from_raw!(Txid);

impl ToRaw for Txid {
    type RawType = RawTxid;

    fn to_raw(self) -> Self::RawType {
        self.0
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct ContractId(pub(crate) RawContractId);

impl ContractId {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl From<[u8; 32]> for ContractId {
    fn from(value: [u8; 32]) -> Self {
        Self(value.into())
    }
}

impl Into<[u8; 32]> for ContractId {
    fn into(self) -> [u8; 32] {
        self.0.as_ref().to_byte_array()
    }
}

impl std::fmt::Display for ContractId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ContractId {
    type Err = <RawContractId as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RawContractId::from_str(s)?.into())
    }
}

impl_from_raw!(ContractId);

impl ToRaw for ContractId {
    type RawType = RawContractId;

    fn to_raw(self) -> Self::RawType {
        self.0
    }
}


#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Outpoint {
    pub txid: Txid,
    pub vout: u32,
}

impl Outpoint {
    pub fn new(txid: impl Into<Txid>, vout: u32) -> Self {
        let txid = txid.into();
        Self {
            txid,
            vout,
        }
    }
}

impl ToRaw for Outpoint {
    type RawType = RawOutpoint;

    fn to_raw(self) -> Self::RawType {
        let outpoint = rgbstd::Outpoint::new(self.txid.to_raw(), self.vout);
        From::<XChain<rgbstd::Outpoint>>::from(XChain::with(rgbstd::Layer1::Bitcoin, outpoint))
    }
}

impl From<RawOutpoint> for Outpoint {
    fn from(o: RawOutpoint) -> Self {
        let outpoint = o.as_reduced_unsafe();
        let txid = outpoint.txid;
        let vout = outpoint.vout.to_u32();
        Self::new(txid, vout)
    }
}


#[derive(Debug, Clone, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Beneficiary {
    WitnessVout(u32),
    Outpoint(Outpoint),
}

impl Beneficiary {
    pub fn new_witness(vout: u32) -> Self {
        Self::WitnessVout(vout)
    }

    pub fn new_outpoint(outpoint: Outpoint) -> Self {
        Self::Outpoint(outpoint)
    }

    pub(crate) fn to_raw_with_blinding(self, blinding: u64) -> RawBeneficiary {
        use bp::seals::txout::CloseMethod;
        use rgbstd::GraphSeal;

        let close_method = CloseMethod::OpretFirst;
        let seal: GraphSeal = match self {
            Self::WitnessVout(vout) => GraphSeal::with_blinded_vout(close_method, vout, blinding),
            Self::Outpoint(outpoint) => {
                GraphSeal::with_blinding(close_method, outpoint.txid.0, outpoint.vout, blinding)
            }
        };

        From::<XChain<GraphSeal>>::from(XChain::with(rgbstd::Layer1::Bitcoin, seal))
    }
}


#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TransitionInfo(pub(crate) RawTransitionInfo);

impl ToRaw for TransitionInfo {
    type RawType = RawTransitionInfo;

    fn to_raw(self) -> Self::RawType {
        self.0
    }
}


// Use BTreeMap to have a consistent order for generating blinding factors
#[derive(Debug, Hash, Clone, Serialize, Deserialize)]
pub struct RgbAssignments(pub(crate) BTreeMap<ContractId, BTreeMap<Beneficiary, u64>>);

impl RgbAssignments {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contracts(&self) -> impl Iterator<Item = &ContractId> {
        self.0.keys()
    }

    pub fn add_recipient_for(
        &mut self,
        contract_id: ContractId,
        recipient: Beneficiary,
        amount: u64,
    ) {
        if amount > 0 {
            let ent = self
                .0
                .entry(contract_id)
                .or_default()
                .entry(recipient)
                .or_default();

            *ent = ent.checked_add(amount).expect("rgb amount overflow");
        }
    }

    pub(crate) fn to_raw_with_blinding_rng<R: Rng>(self, rng: &mut R) -> RawRgbAssignments {
        self.0
            .into_iter()
            .map(|(cid, assignments)| {
                let assignments: BTreeMap<RawBeneficiary, u64> = assignments
                    .into_iter()
                    .map(|(b, v)| {
                        (b.to_raw_with_blinding(rng.gen()), v)
                    })
                    .collect();
                (cid.to_raw(), assignments)
            })
            .collect()
    }
}
