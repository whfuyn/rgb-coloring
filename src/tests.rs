use bp::{*, Tx as BpTx, Outpoint as BpOutpoint};
use ifaces::IssuerWrapper;
use rgbstd::persistence::Stock;
use rgbstd::containers::{ConsignmentExt, Transfer, UniversalFile, ValidConsignment, ValidContract, ValidTransfer};
use rgbstd::BlindingFactor;
// use rgbstd::ContractId;

use crate::api::{
    rgb_issue,
    rgb_coin_select,
    rgb_compose,
    rgb_commit,
    rgb_transfer,
    rgb_balance,
};
use crate::types::{
    Outpoint,
    RgbAssignments,
    Beneficiary,
    ContractId,
};
use crate::resolvers::LnResolver;
use crate::ToRaw;

// TODO: figure out why rgb uses i64 for timestamp
const GENESIS_TIMESTAMP: i64 = 1231006505;

// TODO: deal with duplicate txid
fn get_first_tx() -> BpTx {
    let inputs = [
        TxIn {
            prev_output: BpOutpoint::new(Txid::coinbase(), 0),
            sig_script: SigScript::new(),
            sequence: SeqNo::from_consensus_u32(u32::MAX),
            witness: Witness::new(),
        },
    ];

    let output = TxOut {
        value: Sats::from_btc(1),
        script_pubkey: ScriptPubkey::new(),
    };
    let outputs = vec![output; 3];
    let tx = BpTx {
        version: TxVer::V2,
        lock_time: LockTime::from_height(0).unwrap(),
        inputs: VarIntArray::from_iter_checked(inputs),
        outputs: VarIntArray::from_iter_checked(outputs),
    };

    tx
}

fn build_rgb_tx(inputs: &[Outpoint], outputs_num: usize, commitment: &[u8; 32]) -> BpTx {
    let inputs = inputs
        .iter()
        .map(|o| {
            TxIn {
                prev_output: o.to_raw().as_reduced_unsafe().clone(),
                sig_script: SigScript::new(),
                sequence: SeqNo::from_consensus_u32(u32::MAX),
                witness: Witness::new(),
            }
        })
        .collect::<Vec<_>>();

    let mut outputs = vec![
        TxOut {
            value: Sats::from_sats(546u64),
            script_pubkey: ScriptPubkey::new(),
        };
        outputs_num
    ];
    let opret = TxOut {
        value: Sats::ZERO,
        script_pubkey: ScriptPubkey::op_return(commitment),
    };
    outputs.push(opret);

    let tx = BpTx {
        version: TxVer::V2,
        lock_time: LockTime::from_height(0).unwrap(),
        inputs: VarIntArray::from_iter_checked(inputs),
        outputs: VarIntArray::from_iter_checked(outputs),
    };

    tx
}

fn get_stock() -> Stock {
    // use rgbstd::persistence::fs::FsBinStore;
    // use tempfile::tempdir;
    use schemata::NonInflatableAsset;

    // let data_dir = tempdir().unwrap();
    // let stock_path = data_dir.into_path();

    // let provider = FsBinStore::new(stock_path.clone()).unwrap();
    // Stock::load(provider, true).unwrap();

    // let provider = FsBinStore::new(stock_path).unwrap();
    let mut stock = Stock::in_memory();
    // stock.make_persistent(provider, true).unwrap();

    stock.import_kit(NonInflatableAsset::kit()).unwrap();

    stock
}

#[test]
fn test_rgb_workflow() {
    let is_testnet = true;

    let tx = get_first_tx();
    let txid = tx.txid();
    dbg!(&txid);

    let allocations = [
        (
            format!("opret1st:{txid}:0"),
            100,
        )
    ];

    let contract = rgb_issue(
        "test", "TEST", "TestCoin", "For tests".into(), 8, allocations, is_testnet,
    );
    let contract_id: ContractId  = contract.contract_id().into();
    dbg!(&contract_id);

    let mut resolver = LnResolver::new();
    resolver.add_onchain_tx(&tx.consensus_serialize(), 1, GENESIS_TIMESTAMP);

    let mut stock = get_stock();
    stock.import_contract(contract.clone(), &resolver).unwrap();

    // let contract_id = rgb_issue(&mut stock, &contract_yaml, allocations, &resolver);

    let available_utxos = [
        Outpoint::new(txid, 0),
        // Outpoint::new(txid, 1),
        // Outpoint::new(txid, 2),
    ];

    let recipients = [
        (Beneficiary::new_witness(0), 50),
        (Beneficiary::new_witness(1), 50),
    ];

    let mut rgb_assignments = RgbAssignments::new();
    for (recipient, amount) in recipients {
        rgb_assignments
            .add_recipient_for(contract_id, recipient, amount);
    }

    let coins = rgb_coin_select(&stock, &available_utxos, &rgb_assignments);
    let ti_list = rgb_compose(&stock, dbg!(coins), rgb_assignments, Some(Beneficiary::WitnessVout(2)), 0);
    // let ti_list = rgb_compose(&stock, dbg!(coins), rgb_assignments, None);
    let (commitment, partial_fascia) = rgb_commit(&available_utxos, ti_list);

    let tx = build_rgb_tx(&available_utxos, 3, &commitment);
    let fascia = partial_fascia.complete(&tx.consensus_serialize());

    // dbg!(&tx);
    let spending_txid = tx.txid();
    dbg!(&spending_txid);
    resolver.add_onchain_tx(&tx.consensus_serialize(), 2, GENESIS_TIMESTAMP + 1);
    stock.consume_fascia(fascia, &resolver).unwrap();

    let outputs = [
        // Outpoint::new(spending_txid, 0),
        Outpoint::new(spending_txid, 1),
        // Outpoint::new(spending_txid, 2),
        // Outpoint::new(txid, 1),
        // Outpoint::new(txid, 2),
    ];
    let consign = rgb_transfer(&stock, contract_id, &outputs);
    dbg!(&consign.consignment_id());
    // dbg!(&consign);

    consign.validate(&resolver, is_testnet).unwrap();

    dbg!(rgb_balance(&stock, contract_id, &outputs));

    // let available_utxos = [
    //     Outpoint::new(spending_txid, 0),
    //     Outpoint::new(spending_txid, 1),
    //     // Outpoint::new(spending_txid, 2),
    //     // Outpoint::new(txid, 1),
    //     // Outpoint::new(txid, 2),
    // ];

    // let recipients = [
    //     (Beneficiary::new_witness(0), 21),
    // ];

    // let mut rgb_assignments = RgbAssignments::new();
    // for (recipient, amount) in recipients {
    //     rgb_assignments
    //         .add_recipient_for(contract_id, recipient, amount);
    // }

    // let coins = rgb_coin_select(&stock, &available_utxos, &rgb_assignments);
    // let ti_list = rgb_compose(&stock, dbg!(coins), rgb_assignments, Beneficiary::WitnessVout(2));

}

#[test]
fn test_coloring_consistency() {
    let is_testnet = true;

    let genesis_tx = get_first_tx();
    let genesis_txid = genesis_tx.txid();

    let allocations = [
        (
            format!("opret1st:{genesis_txid}:0"),
            100,
        )
    ];

    let contract = rgb_issue(
        "test", "TEST", "TestCoin", "For tests".into(), 8, allocations, is_testnet,
    );

    for blinding_seed in 0..10 {
        let (first_commitment, first_consignment) = basic_transfer(genesis_tx.clone(), contract.clone(), blinding_seed, is_testnet);
        let (second_commitment, second_consignment) = basic_transfer(genesis_tx.clone(), contract.clone(), blinding_seed, is_testnet);

        assert_eq!(first_commitment, second_commitment);
        assert_eq!(first_consignment.consignment_id(), second_consignment.consignment_id());
    }
}

fn basic_transfer(
    genesis_tx: Tx,
    contract: ValidContract,
    blinding_seed: u64,
    is_testnet: bool,
) -> ([u8; 32], ValidTransfer) {
    let genesis_txid = genesis_tx.txid();
    let contract_id: ContractId  = contract.contract_id().into();

    let mut resolver = LnResolver::new();
    resolver.add_onchain_tx(&genesis_tx.consensus_serialize(), 1, GENESIS_TIMESTAMP);

    let mut stock = get_stock();
    stock.import_contract(contract.clone(), &resolver).unwrap();

    let recipients = [
        (Beneficiary::new_witness(0), 20),
        (Beneficiary::new_witness(1), 80),
    ];
    let mut rgb_assignments = RgbAssignments::new();
    for (recipient, amount) in recipients {
        rgb_assignments
            .add_recipient_for(contract_id, recipient, amount);
    }

    let available_utxos = [
        Outpoint::new(genesis_txid, 0),
    ];
    let prev_outputs = rgb_coin_select(&stock, &available_utxos, &rgb_assignments);
    let ti_list = rgb_compose(&stock, prev_outputs, rgb_assignments, Some(Beneficiary::WitnessVout(2)), blinding_seed);
    let (commitment, partial_fascia) = rgb_commit(&available_utxos, ti_list);

    let spending_tx = build_rgb_tx(&available_utxos, 3, &commitment);
    let spending_txid = spending_tx.txid();

    let fascia = partial_fascia.complete(&spending_tx.consensus_serialize());

    resolver.add_onchain_tx(&spending_tx.consensus_serialize(), 2, GENESIS_TIMESTAMP + 1);
    stock.consume_fascia(fascia.clone(), &resolver).unwrap();

    let outputs = [
        Outpoint::new(spending_txid, 0),
        // Outpoint::new(spending_txid, 1),
        // Outpoint::new(spending_txid, 2),
    ];
    let transfer = rgb_transfer(&stock, contract_id, &outputs);

    let valid_transfer = transfer.validate(&resolver, is_testnet).unwrap();

    let balance = rgb_balance(&stock, contract_id, &outputs);

    assert_eq!(balance, 20);

    {
        let outputs = [
            // Outpoint::new(spending_txid, 0),
            Outpoint::new(spending_txid, 1),
            // Outpoint::new(spending_txid, 2),
        ];

        let mut stock = get_stock();
        stock.accept_transfer(valid_transfer.clone(), resolver).unwrap();

        let balance = rgb_balance(&stock, contract_id, &outputs);

        assert_eq!(balance, 80);
    }

    (commitment, valid_transfer)
}
