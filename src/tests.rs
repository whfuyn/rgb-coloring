use bp::{*, Tx as BpTx, Outpoint as BpOutpoint};
use rgbstd::persistence::Stock;
use rgbstd::containers::UniversalFile;

use crate::api::{
    rgb_issue,
    rgb_coin_select,
    rgb_compose,
    rgb_commit,
};
use crate::types::{
    Outpoint,
    RgbDistribution,
    Beneficiary,
};
use crate::resolver::LnResolver;
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

fn build_rgb_tx(inputs: &[Outpoint], outputs: &[u64]) -> BpTx {
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

    let mut outputs = outputs
        .iter()
        .map(|v| {
            TxOut {
                value: Sats::from_sats(*v),
                script_pubkey: ScriptPubkey::new(),
            }
        })
        .collect::<Vec<_>>();
    let opret = TxOut {
        value: Sats::ZERO,
        script_pubkey: ScriptPubkey::op_return(&[]),
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
    use rgbstd::persistence::fs::FsBinStore;
    use tempfile::tempdir;

    let data_dir = tempdir().unwrap();
    let stock_path = data_dir.into_path();

    // let provider = FsBinStore::new(stock_path.clone()).unwrap();
    // Stock::load(provider, true).unwrap();

    let provider = FsBinStore::new(stock_path).unwrap();
    let mut stock = Stock::in_memory();
    stock.make_persistent(provider, true).unwrap();

    let UniversalFile::Kit(kit) = UniversalFile::load_file("NonInflatableAssets.rgb").unwrap() else {
        panic!();
    };

    let validated_kit = kit.validate().unwrap();
    stock.import_kit(validated_kit).unwrap();


    stock
}

#[test]
fn test_rgb_issue() {
    let contract_yaml = std::fs::read_to_string("DBG.yaml").unwrap();

    let mut stock = get_stock();

    let tx = get_first_tx();
    let txid = tx.txid();
    dbg!(&txid);

    let mut resolver = LnResolver::new();
    resolver.add_tx(tx, 1, GENESIS_TIMESTAMP);

    let allocation = [
        (
            format!("opret1st:{txid}:0"),
            100000000,
        )
    ];

    let contract_id = rgb_issue(&mut stock, &contract_yaml, allocation, &resolver);

    let available_utxos = [
        Outpoint::new(txid, 0),
        // Outpoint::new(txid, 1),
        // Outpoint::new(txid, 2),
    ];

    let recipients = [
        (Beneficiary::new_witness(0), 10),
        (Beneficiary::new_witness(1), 10),
    ];

    let mut rgb_distribution = RgbDistribution::new();
    for (recipient, amount) in recipients {
        rgb_distribution
            .add_recipient_for(contract_id, recipient, amount);
    }

    let coins = rgb_coin_select(&stock, &available_utxos, &rgb_distribution);
    let ti_list = rgb_compose(&stock, dbg!(coins), rgb_distribution, Beneficiary::WitnessVout(2));
    let (commitment, partial_fascia) = rgb_commit(&available_utxos, ti_list);

    let mut tx = build_rgb_tx(&available_utxos, &[1000, 1000, 1000]);
    let opret_pos = tx
        .outputs()
        .position(|o| o.script_pubkey.is_op_return())
        .unwrap();

    tx.outputs[opret_pos].script_pubkey = ScriptPubkey::op_return(&commitment);

    let fascia = partial_fascia.complete(&tx.consensus_serialize());

    dbg!(&tx);
    let spending_txid = tx.txid();
    dbg!(&spending_txid);
    resolver.add_tx(tx, 2, GENESIS_TIMESTAMP + 1);
    stock.consume_fascia(fascia, resolver).unwrap();

    let available_utxos = [
        Outpoint::new(spending_txid, 0),
        Outpoint::new(spending_txid, 1),
        // Outpoint::new(spending_txid, 2),
        // Outpoint::new(txid, 1),
        // Outpoint::new(txid, 2),
    ];

    let recipients = [
        (Beneficiary::new_witness(0), 21),
    ];

    let mut rgb_distribution = RgbDistribution::new();
    for (recipient, amount) in recipients {
        rgb_distribution
            .add_recipient_for(contract_id, recipient, amount);
    }

    let coins = rgb_coin_select(&stock, &available_utxos, &rgb_distribution);
    let ti_list = rgb_compose(&stock, dbg!(coins), rgb_distribution, Beneficiary::WitnessVout(2));

}

