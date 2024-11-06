# RGB Coloring

An experimental library for working with the RGB protocol.

It provides the ability to color the Bitcoin transactions, i.e. generates the RGB commitment and embed it in the Bitcoin transaction.

The APIs allows a more flexible control over the distribution of the RGB tokens in the transaction inputs.

You can use it to:
- Issue a new RGB20 token.
- Transfer the RGB20 tokens.
- Check the balance of the RGB20 tokens.

Currently, only the RGB20(Fungible token) interface is supported.

And it's still a work in progress.

## Usage

### 1. Issue a new RGB20 token.
```rust
let is_testnet = true;

// Get an UTXO for issuing the token.
let genesis_tx = get_first_tx();
let genesis_txid = genesis_tx.txid();

// Define the genesis allocations for the token.
let allocations = [
    // Allocate 100 tokens to this UTXO.
    (format!("opret1st:{genesis_txid}:0"), 100),
];

let contract = rgb_issue(
    "test", "TEST", "TestCoin", "For tests".into(), 8, allocations, is_testnet,
);
```

### 2. Transfer the token.
```rust
// Sender's stock.
let mut stock = get_stock();
stock.import_contract(contract.clone(), &resolver).unwrap();

// Define the recipients and their amounts.
let recipients = [
    // Send 20 tokens to the first recipient.
    // WitnessVout(0) is the first output of the spending transaction.
    (Beneficiary::new_witness(0), 20),
    // Send 80 tokens to the second recipient.
    (Beneficiary::new_witness(1), 80),
];
let mut rgb_assignments = RgbAssignments::new();
for (recipient, amount) in recipients {
    rgb_assignments
        .add_recipient_for(contract_id, recipient, amount);
}

// The UTXOs, possibly containing the token, that we can spend.
let available_utxos = [
    Outpoint::new(genesis_txid, 0),
];
// Select UTXOs that satisfies the RGB assignments.
let prev_outputs = rgb_coin_select(&stock, &available_utxos, &rgb_assignments);
let transition_list = rgb_compose(
    &stock,
    prev_outputs,
    rgb_assignments,
    // Where to put the change.
    Some(Beneficiary::WitnessVout(2)),
    // The seed for generating the blinding factor.
    blinding_seed,
);
// The `commitment` is what we put in the OP_RETURN output.
// The `partial_fascia` is the incomplete data that needs to be consumed by the stock.
let (commitment, partial_fascia) = rgb_commit(&available_utxos, transition_list);

// The transaction that needs to be broadcasted to actually transfer the token.
let spending_tx = build_rgb_tx(&available_utxos, 3, &commitment);
let spending_txid = spending_tx.txid();

let fascia = partial_fascia.complete_with_tx(&spending_tx.consensus_serialize());
stock.consume_fascia(fascia.clone(), &resolver).unwrap();

// Generate the transfer data.
let outputs = [
    Outpoint::new(spending_txid, 1),
];
let transfer = rgb_transfer(&stock, contract_id, &outputs);
```

### 3. Accept the transfer.
```rust
// Recipient's stock.
let mut stock = get_stock();

// The recipient should use an online resolver to validate the transfer.
let valid_transfer = transfer.validate(&resolver, is_testnet).unwrap();
stock.accept_transfer(valid_transfer.clone(), resolver).unwrap();

let outputs = [
    Outpoint::new(spending_txid, 1),
];
let balance = rgb_balance(&stock, contract_id, &outputs);

assert_eq!(balance, 80);
```
