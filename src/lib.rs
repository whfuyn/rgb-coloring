// TODO:
// - Refactor types and APIs
// - Error handling
// - Tests

mod api;
mod detail;
mod types;
mod resolvers;
mod error;

// #[cfg(test)]
mod tests;

pub use rgbstd;
pub use amplify;
pub use strict_types;
pub use strict_encoding;

pub mod prelude {
    pub use crate::types::{
        Beneficiary, ContractId, Outpoint, RgbAssignments, TransitionInfo, Txid,
    };

    pub use crate::api::{rgb_coin_select, rgb_commit, rgb_compose, rgb_balance, rgb_transfer, rgb_issue, get_empty_stock, rgb_export_contract, filter_rgb_outpoints};
    pub use crate::resolvers::{LnResolver, LocalResolver, FasciaResolver};
    pub use strict_encoding::{StrictDeserialize, StrictSerialize};
    pub use rgbstd::{
        persistence::Stock,
        containers::Transfer,
    };
}

pub use prelude::*;
pub use types::ToRaw;
