// TODO:
// - Error handling
// - Tests
// - Use Txid or Wtxid?

mod api;
mod detail;
mod types;
mod resolver;
mod error;

// #[cfg(test)]
mod tests;

pub use rgbstd;

pub mod prelude {
    pub use crate::types::{
        Beneficiary, ContractId, Outpoint, RgbCoin, RgbDistribution, TransitionInfo, Txid,
    };

    pub use crate::api::{rgb_coin_select, rgb_commit, rgb_compose};
    pub use crate::resolver::LnResolver;
}

pub use prelude::*;
pub use types::ToRaw;
