// TODO:
// - Refactor types and APIs
// - Error handling
// - Tests

mod api;
mod detail;
mod types;
mod resolvers;
mod error;

#[cfg(test)]
mod tests;

pub use rgbstd;
pub use rgbinvoice;
pub use amplify;
pub use strict_types;
pub use strict_encoding;

pub mod prelude {
    pub use crate::types::{
        Beneficiary, ContractId, Outpoint, RgbAssignments, TransitionInfo, Txid,
    };

    pub use crate::api::*;
    pub use crate::resolvers::{LnResolver, LocalResolver, FasciaResolver, OnlineResolver};
    pub use strict_encoding::{StrictDeserialize, StrictSerialize};
    pub use rgbstd::{
        persistence::Stock,
        containers::Transfer,
        containers::ValidTransfer,
    };
}

pub use prelude::*;
pub use types::ToRaw;
