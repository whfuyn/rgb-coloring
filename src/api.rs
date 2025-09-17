use std::collections::HashMap;

use rand::{Rng, SeedableRng};
use rgbinvoice::{RgbInvoice, RgbInvoiceBuilder};
use rgbstd::containers::{Contract, Transfer, ValidContract};
use rgbstd::persistence::{IndexProvider, StashProvider, StateProvider, Stock};
use rgbstd::OutputSeal;

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
    chain_net: &str,
) -> ValidContract {
    let chain_net = match chain_net {
        "bitcoin" => rgbstd::ChainNet::BitcoinMainnet,
        "regtest" => rgbstd::ChainNet::BitcoinRegtest,
        "testnet" => rgbstd::ChainNet::BitcoinTestnet3,
        "testnet4" => rgbstd::ChainNet::BitcoinTestnet4,
        "signet" => rgbstd::ChainNet::BitcoinSignet,
        _ => todo!(),

    };
    detail::rgb_issue(issuer, ticker, name, details, precision, allocations, chain_net)
}

pub fn rgb_balance<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
    utxos: &[Outpoint],
) -> u64 {
    detail::rgb_balance(stock, contract_id.to_raw(), &utxos)
}

pub fn rgb_assignments<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    utxos: &[Outpoint],
) -> HashMap<ContractId, HashMap<Outpoint, u64>> {
    detail::rgb_assignments(stock, &utxos)
        .into_iter()
        .map(|(cid, map)| {
            let cid = ContractId::from(cid);
            (cid, map)
        })
        .collect()
}

pub fn filter_rgb_outpoints<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    utxos: &[Outpoint],
) -> Vec<Outpoint> {
    // let utxos: Vec<RawOutpoint> =
    //     utxos.iter().copied().map(ToRaw::to_raw).collect();

    detail::filter_rgb_outpoints(stock, &utxos)
        .into_iter()
        .map(|o| Outpoint::from(o))
        .collect()
}

pub fn rgb_coin_select<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    available_utxos: &[Outpoint],
    rgb_assignments: &RgbAssignments,
) -> Vec<Outpoint> {
    // let available_utxos: Vec<RawOutpoint> =
    //     available_utxos.iter().copied().map(ToRaw::to_raw).collect();

    let coins = detail::rgb_coin_select(stock, &available_utxos, rgb_assignments);
    coins
        .into_iter()
        .map(|coin| {
            let outpoint = coin.to_outpoint();

            Outpoint::new(outpoint.txid, outpoint.vout.into_u32())
        })
        .collect()
}

pub fn rgb_compose<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    prev_outputs: impl IntoIterator<Item = Outpoint>,
    rgb_assignments: RgbAssignments,
    change_seal: Option<Beneficiary>,
) -> Vec<Transition> {
    let prev_outputs = prev_outputs
        .into_iter()
        .collect::<Vec<_>>();

    let mut rng = {
        // TODO(fy): use a stable hash function, std hasher algorithm is not specified.
        let blinding_seed = {
            use std::hash::{DefaultHasher, Hash, Hasher};

            #[derive(Debug, Hash, Clone)]
            struct ColoringInfo {
                prev_outputs: Vec<Outpoint>,
                rgb_assignments: RgbAssignments,
                change_seal: Option<Beneficiary>,
            }

            let mut hasher = DefaultHasher::new();
            let coloring_info = ColoringInfo {
                prev_outputs: prev_outputs.clone(),
                rgb_assignments: rgb_assignments.clone(),
                change_seal: change_seal.clone(),
            };
            coloring_info.hash(&mut hasher);
            hasher.finish()
        };
        rand_chacha::ChaCha20Rng::seed_from_u64(blinding_seed)
    };

    let prev_outputs = prev_outputs
        .into_iter()
        .map(|o| {
            OutputSeal::with(o.txid, o.vout)
        });

    let rgb_assignments = rgb_assignments.to_raw_with_blinding_rng(&mut rng);
    let change_seal = change_seal.map(|s| s.to_raw_with_blinding(rng.gen()));
    
    let transition_list = detail::rgb_compose(
        stock,
        prev_outputs,
        rgb_assignments,
        change_seal,
    )
    .unwrap();

    transition_list
        .into_iter()
        .map(Transition)
        .collect()
}

pub fn rgb_commit(
    // The order of txins must not be changed after `rgb_commit`
    finalized_txins: &[Outpoint],
    transition_list: Vec<Transition>,
) -> ([u8; 32], PartialFascia) {
    // let finalized_txins = finalized_txins
    //     .iter()
    //     .copied()
    //     .map(ToRaw::to_raw)
    //     .collect::<Vec<_>>();

    let transition_list = transition_list
        .into_iter()
        .map(ToRaw::to_raw)
        .collect();

    let (commitment, partial_fascia) = detail::rgb_commit(&finalized_txins, transition_list);

    (commitment.to_byte_array(), partial_fascia)
}

pub fn rgb_transfer<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
    outputs: &[Outpoint],
    secret_seal: Option<[u8; 32]>,
    witness_tx: Option<Txid>,
) -> Transfer {
    let outputs = outputs
        .into_iter()
        .cloned()
        .map(rgbstd::OutputSeal::new)
        .collect::<Vec<_>>();

    let secret_seal = secret_seal.map(SecretSeal::from);
    detail::rgb_transfer(stock, contract_id.to_raw(), &outputs, secret_seal, witness_tx)
}

pub fn get_empty_stock() -> Stock {
    use schemata::NonInflatableAsset;
    use rgbstd::contract::IssuerWrapper;

    let mut stock = Stock::in_memory();
    stock.import_kit(NonInflatableAsset::kit()).unwrap();

    stock
}

pub fn rgb_export_contract<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
) -> Contract {
    stock.export_contract(contract_id.to_raw()).unwrap()
}

pub fn rgb_build_invoice<'a, S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &mut Stock<S, H, P>,
    contract_id: ContractId,
    amount: u64,
    beneficiary: Beneficiary,
    transports: impl IntoIterator<Item = &'a str>,
    expiry_secs: Option<u32>,
    chain_net: rgbstd::ChainNet,
) -> RgbInvoice {
    use rgbstd::GraphSeal;
    use commit_verify::Conceal;

    let beneficiary = {
        let b = match beneficiary {
            Beneficiary::WitnessVout(vout) => {
                let seal = GraphSeal::new_random_vout(vout);
                stock.store_secret_seal(seal).unwrap();
                // TODO: check
                rgbinvoice::Beneficiary::BlindedSeal(seal.conceal())
            }
            Beneficiary::Outpoint(outpoint) => {
                let seal = GraphSeal::new_random(outpoint.txid, outpoint.vout);
                stock.store_secret_seal(seal).unwrap();
                rgbinvoice::Beneficiary::BlindedSeal(seal.conceal())
            }
            Beneficiary::SecretSeal(secret_seal) => {
                // TODO: check
                rgbinvoice::Beneficiary::BlindedSeal(SecretSeal::from(secret_seal))
            }
        };
        rgbinvoice::XChainNet::with(chain_net, b)
    };

    let expiry = {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        (timestamp + expiry_secs.unwrap_or(600) as u64) as i64
    };
    RgbInvoiceBuilder::with(contract_id.to_raw(), beneficiary)
        .set_expiry_timestamp(expiry)
        .set_amount_raw(amount)
        .add_transports(transports)
        .unwrap()
        .finish()
}

pub fn to_rgb_chain_net(bitcoin_network: &str) -> rgbstd::ChainNet {
    match bitcoin_network {
        "bitcoin" => rgbstd::ChainNet::BitcoinMainnet,
        "regtest" => rgbstd::ChainNet::BitcoinRegtest,
        "testnet" => rgbstd::ChainNet::BitcoinTestnet3,
        "testnet4" => rgbstd::ChainNet::BitcoinTestnet4,
        "signet" => rgbstd::ChainNet::BitcoinSignet,
        _ => todo!(),
    }
}
