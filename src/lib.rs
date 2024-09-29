// TODO:
// - Error handling
// - Tests

mod api;
mod detail;
mod types;
mod resolver;
mod error;

// #[cfg(test)]
mod tests;

pub use rgbstd;
pub use amplify;

pub mod prelude {
    pub use crate::types::{
        Beneficiary, ContractId, Outpoint, RgbAssignments, TransitionInfo, Txid,
    };

    pub use crate::api::{rgb_coin_select, rgb_commit, rgb_compose, rgb_balance};
    pub use crate::resolver::LnResolver;
    pub use strict_encoding::{StrictDeserialize, StrictSerialize};
    pub use rgbstd::{
        persistence::Stock,
        containers::Transfer,
    };
}

pub use prelude::*;
pub use types::ToRaw;
