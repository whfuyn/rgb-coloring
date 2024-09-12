use rgbstd::containers::Transfer;
use rgbstd::persistence::{IndexProvider, StashProvider, StateProvider, Stock};
use rgbstd::validation::ResolveWitness;

use crate::types::*;

use crate::detail;
use crate::detail::PartialFascia;

pub fn rgb_coin_select<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    available_utxos: &[Outpoint],
    rgb_distribution: &RgbDistribution,
) -> Vec<RgbCoin> {
    let available_utxos: Vec<RawOutpoint> =
        available_utxos.iter().copied().map(ToRaw::to_raw).collect();

    let coins = detail::rgb_coin_select(stock, &available_utxos, rgb_distribution.to_raw());
    coins.into_iter().map(RgbCoin).collect()
}

pub fn rgb_compose<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    prev_outputs: impl IntoIterator<Item = RgbCoin>,
    rgb_distribution: RgbDistribution,
    change_seal: Beneficiary,
) -> Vec<TransitionInfo> {
    let transition_info_list = detail::rgb_compose(
        stock,
        prev_outputs.into_iter().map(ToRaw::to_raw),
        rgb_distribution.0,
        change_seal.to_raw(),
    )
    .unwrap();

    transition_info_list
        .into_iter()
        .map(TransitionInfo)
        .collect()
}

pub fn rgb_commit(
    finalized_txins: &[Outpoint],
    transition_info_list: Vec<TransitionInfo>,
) -> ([u8; 32], PartialFascia) {
    let finalized_txins = finalized_txins
        .iter()
        .copied()
        .map(ToRaw::to_raw)
        .collect::<Vec<_>>();

    let transition_info_list = transition_info_list
        .into_iter()
        .map(ToRaw::to_raw)
        .collect();

    let (commitment, partial_fascia) = detail::rgb_commit(&finalized_txins, transition_info_list);

    (commitment.to_byte_array(), partial_fascia)
}

// TODO: better interface
pub fn rgb_issue<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &mut Stock<S, H, P>,
    contract_yaml: &str,
    additional_genesis_allocation: impl IntoIterator<Item = (String, u64)>,
    resolver: impl ResolveWitness,
) -> ContractId {
    let contract_id = detail::rgb_issue(stock, contract_yaml, additional_genesis_allocation, resolver);
    contract_id.into()
}

pub(crate) fn rgb_transfer<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
    outputs: &[Outpoint],
) -> Transfer {
    use rgbstd::OutputSeal;
    use bp::seals::txout::CloseMethod;

    let outputs = outputs
        .into_iter()
        .map(|o| {
            o
                .to_raw()
                .map(|o|
                    OutputSeal::new(CloseMethod::OpretFirst, o)
                )
        })
        .collect::<Vec<_>>();

    detail::rgb_transfer(stock, contract_id.to_raw(), &outputs)
}
