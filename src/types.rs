// TODO: consider if it's a better option to wrap those types or we should just expose them

pub(crate) use crate::detail::{
    Beneficiary as RawBeneficiary, RgbAssignments as RawRgbAssignments,
};
pub(crate) use rgbstd::{
    containers::TransitionInfo as RawTransitionInfo, ContractId as RawContractId, Txid as RawTxid,
    XChain, XOutpoint as RawOutpoint, XOutputSeal as RawCoin,
};

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

impl_from_raw!(Txid);
impl_from_raw!(ContractId);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Txid(pub(crate) RawTxid);

impl From<[u8; 32]> for Txid {
    fn from(value: [u8; 32]) -> Self {
        Self(value.into())
    }
}

// impl From<RawTxid> for Txid {
//     fn from(value: RawTxid) -> Self {
//         Self(value)
//     }
// }

impl Into<[u8; 32]> for Txid {
    fn into(self) -> [u8; 32] {
        self.0.as_ref().to_byte_array()
    }
}

impl ToRaw for Txid {
    type RawType = RawTxid;

    fn to_raw(self) -> Self::RawType {
        self.0
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct ContractId(pub(crate) RawContractId);

// impl From<RawContractId> for ContractId {
//     fn from(value: RawContractId) -> Self {
//         Self(value)
//     }
// }

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

impl ToRaw for ContractId {
    type RawType = RawContractId;

    fn to_raw(self) -> Self::RawType {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
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

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
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
}

impl ToRaw for Beneficiary {
    type RawType = RawBeneficiary;

    fn to_raw(self) -> Self::RawType {
        use bp::seals::txout::CloseMethod;
        use rgbstd::GraphSeal;

        let close_method = CloseMethod::OpretFirst;
        let seal: GraphSeal = match self {
            Self::WitnessVout(vout) => GraphSeal::new_random_vout(close_method, vout),
            Self::Outpoint(outpoint) => {
                GraphSeal::new_random(close_method, outpoint.txid.0, outpoint.vout)
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RgbCoin(pub(crate) RawCoin);

impl RgbCoin {
    pub fn outpoint(&self) -> Outpoint {
        let xoutpoint = self.0.to_outpoint();
        let outpoint = xoutpoint.as_reduced_unsafe();

        Outpoint {
            txid: Txid(outpoint.txid),
            vout: outpoint.vout.into_u32(),
        }
    }
}

impl ToRaw for RgbCoin {
    type RawType = RawCoin;

    fn to_raw(self) -> Self::RawType {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct RgbAssignments(pub(crate) RawRgbAssignments);

impl RgbAssignments {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add_recipient_for(
        &mut self,
        contract_id: ContractId,
        recipient: Beneficiary,
        amount: u64,
    ) {
        let contract_id = contract_id.to_raw();
        let ent = self
            .0
            .entry(contract_id)
            .or_default()
            .entry(recipient.to_raw())
            .or_default();

        *ent = ent.checked_add(amount).expect("rgb amount overflow");
    }
}

impl ToRaw for RgbAssignments {
    type RawType = RawRgbAssignments;

    fn to_raw(self) -> Self::RawType {
        self.0
    }
}

impl<'a> ToRaw for &'a RgbAssignments {
    type RawType = &'a RawRgbAssignments;

    fn to_raw(self) -> Self::RawType {
        &self.0
    }
}
