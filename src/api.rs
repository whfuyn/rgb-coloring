use bp::dbc::Method;
use rand::{Rng, SeedableRng};
use rgbstd::containers::{Transfer, ValidContract};
use rgbstd::persistence::{IndexProvider, StashProvider, StateProvider, Stock};
use rgbstd::{Identity, OutputSeal, Precision};

use crate::types::*;

use crate::detail;
use crate::detail::PartialFascia;


pub fn rgb_issue(
    issuer: &str,
    ticker: &str,
    name: &str,
    details: Option<&str>,
    precision: u8,
    allocations: impl IntoIterator<Item = (String, u64)>,
    is_testnet: bool,
) -> ValidContract {
    detail::rgb_issue(issuer, ticker, name, details, precision, allocations, is_testnet)
}

pub fn rgb_balance<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
    utxos: &[Outpoint],
) -> u64 {
    let utxos: Vec<RawOutpoint> =
        utxos.iter().copied().map(ToRaw::to_raw).collect();

    detail::rgb_balance(stock, contract_id.to_raw(), &utxos)
}

pub fn rgb_coin_select<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    available_utxos: &[Outpoint],
    rgb_assignments: &RgbAssignments,
) -> Vec<Outpoint> {
    let available_utxos: Vec<RawOutpoint> =
        available_utxos.iter().copied().map(ToRaw::to_raw).collect();

    let coins = detail::rgb_coin_select(stock, &available_utxos, rgb_assignments);
    coins
        .into_iter()
        .map(|coin| {
            let xoutpoint = coin.to_outpoint();
            let outpoint = xoutpoint.as_reduced_unsafe();

            Outpoint::new(outpoint.txid, outpoint.vout.into_u32())
        })
        .collect()
}

pub fn rgb_compose<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    prev_outputs: impl IntoIterator<Item = Outpoint>,
    rgb_assignments: RgbAssignments,
    change_seal: Option<Beneficiary>,
    // TODO: or [u8; 32]?
    blinding_seed: u64,
) -> Vec<TransitionInfo> {
    let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(blinding_seed);

    let prev_outputs = prev_outputs
        .into_iter()
        .map(|o| {
            let o = OutputSeal::with(Method::OpretFirst, o.txid.to_raw(), o.vout);
            XChain::Bitcoin(o)
        });

    let rgb_assignments = rgb_assignments.to_raw_with_bliding_rng(&mut rng);
    let change_seal = change_seal.map(|s| s.to_raw_with_blinding(rng.gen()));
    
    let transition_info_list = detail::rgb_compose(
        stock,
        prev_outputs,
        rgb_assignments,
        change_seal,
        &mut rng,
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
