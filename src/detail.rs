// TODO: error handling

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::str::FromStr;

use amplify::bmap;
use amplify::confinement::NonEmptyOrdMap;
use amplify::confinement::{Confined, U24};
use bp::dbc::opret::OpretProof;
use commit_verify::mpc::{self, MPC_MINIMAL_DEPTH};
use commit_verify::CommitId as _;
use commit_verify::TryCommitVerify;
use rgbstd::containers::Fascia;
use rgbstd::containers::PubWitness;
use rgbstd::containers::SealWitness;
use rgbstd::containers::Transfer;
use rgbstd::containers::ValidContract;
use rgbstd::contract::AllocatedState;
use rgbstd::contract::ContractBuilder;
use rgbstd::persistence::ComposeError;
use rgbstd::persistence::StockError;
use rgbstd::stl::AssetSpec;
use rgbstd::stl::ContractTerms;
use rgbstd::stl::RicardianContract;
use rgbstd::validation::DbcProof;
use rgbstd::Amount;
use rgbstd::ChainNet;
use rgbstd::GenesisSeal;
use rgbstd::Identity;
use rgbstd::KnownTransition;
use rgbstd::Operation;
use rgbstd::Opout;
use rgbstd::OutputSeal;
use rgbstd::Precision;
use rgbstd::SecretSeal;
use rgbstd::Transition;
use rgbstd::TransitionBundle;
use rgbstd::{
    Txid,
    containers::BuilderSeal,
    persistence::{IndexProvider, StashProvider, StateProvider, Stock},
    ContractId, GraphSeal, OpId, Outpoint,
};
use schemata::NonInflatableAsset;

use bp::{ConsensusDecode as _, Tx};
use strict_types::FieldName;

use crate::ToRaw;


// Be careful when using HashMap/HashSet, its iteration order is undefined,
// which might break coloring consistency.
pub(crate) type RgbAssignments = BTreeMap<ContractId, BTreeMap<Beneficiary, u64>>;
pub(crate) type Beneficiary = BuilderSeal<GraphSeal>;

pub(crate) fn rgb_balance<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
    utxos: &[Outpoint],
) -> u64 {
    let assignment_name = FieldName::from("assetOwner");

    let contract = stock
        .contract_data(contract_id)
        .unwrap();
    // .map_err(|e| e.to_string())?;

    let amount = contract
        .fungible(assignment_name, utxos)
        .unwrap()
        .map(|a| a.state)
        .sum::<Amount>();

    amount.into()
}

pub(crate) fn rgb_assignments<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    utxos: &[Outpoint],
) -> HashMap<ContractId, HashMap<Outpoint, u64>> {
    let assignment_name = FieldName::from("assetOwner");

    let contracts = stock
        .contracts()
        .unwrap()
        .map(|c| c.id);

    let mut assignments = HashMap::new();
    
    for contract_id in contracts {
        let contract = stock
            .contract_data(contract_id)
            .unwrap();
        // .map_err(|e| e.to_string())?;

        let amounts: HashMap<Outpoint, u64> = contract
            .fungible(assignment_name.clone(), utxos)
            .unwrap()
            .map(|a| (a.seal.to_outpoint(), a.state.value()))
            .collect();

        let all_amounts: HashMap<Outpoint, u64> = contract
            .fungible(assignment_name.clone(), utxos)
            .unwrap()
            .map(|a| (a.seal.to_outpoint(), a.state.value()))
            .collect();

        println!("contract_id: {}", contract_id);
        dbg!(&amounts, &all_amounts);

        assignments.insert(contract_id, amounts);
    }

    assignments
}

pub(crate) fn filter_rgb_outpoints<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    utxos: &[Outpoint],
) -> HashSet<Outpoint> {
    let assignment_name = FieldName::from("assetOwner");

    let mut rgb_outpoints = vec![];
    for contract_info in stock.contracts().unwrap() {
        let contract_id = contract_info.id;

        let contract = stock
            .contract_data(contract_id)
            .unwrap();
        // .map_err(|e| e.to_string())?;

        rgb_outpoints.extend(
            contract
                .fungible(assignment_name.clone(), utxos)
                .unwrap()
                .map(|o| o.seal.to_outpoint())
        );
    }

    rgb_outpoints
        .into_iter()
        .collect()
}

pub(crate) fn rgb_coin_select<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    available_utxos: &[Outpoint],
    rgb_assignments: &crate::types::RgbAssignments,
) -> Vec<OutputSeal> {
    // Only support RGB20Fixed for now.
    let assignment_name = FieldName::from("assetOwner");

    let mut selected_prev_outputs: Vec<OutputSeal> = vec![];
    for (&contract_id, rgb_assignment) in &rgb_assignments.0 {
        let total_amount_needed: u64 = rgb_assignment.iter().map(|(_, amount)| *amount).sum();
        let mut total_amount_collected = Amount::ZERO;

        let contract = stock
            .contract_data(contract_id.to_raw())
            // .unwrap();
            .map_err(|e| e.to_string()).unwrap();

        let prev_outputs = {
            let state: BTreeMap<_, Vec<Amount>> = contract
                .fungible(assignment_name.clone(), available_utxos)
                .unwrap()
                .fold(bmap![], |mut set, a| {
                    set.entry(a.seal).or_default().push(a.state);
                    set
                });
            let mut state: Vec<_> = state
                .into_iter()
                .map(|(seal, vals)| (vals.iter().copied().sum::<Amount>(), seal, vals))
                .collect();
            state.sort_by_key(|(sum, _, _)| *sum);
            state
                .iter()
                .rev()
                .take_while(|(val, _, _)| {
                    if total_amount_collected >= total_amount_needed.into() {
                        false
                    } else {
                        total_amount_collected += *val;
                        true
                    }
                })
                .map(|(_, seal, _)| *seal)
                .collect::<BTreeSet<OutputSeal>>()
        };

        selected_prev_outputs.extend(prev_outputs);
    }

    selected_prev_outputs.sort();
    selected_prev_outputs.dedup();

    selected_prev_outputs
}

pub(crate) fn rgb_compose<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    prev_outputs: impl IntoIterator<Item = impl Into<OutputSeal>>,
    rgb_assignments: BTreeMap<ContractId, BTreeMap<Beneficiary, u64>>,
    change_seal: Option<Beneficiary>,
) -> Result<Vec<Transition>, StockError<S, H, P, ComposeError>> {
    let prev_outputs = prev_outputs
        .into_iter()
        .map(|o| o.into())
        .collect::<HashSet<OutputSeal>>();

    let mut transition_list: Vec<Transition> = vec![];

    let transition_name = FieldName::from("transfer");
    let assignment_name = FieldName::from("assetOwner");

    let handled_contract_ids = rgb_assignments.keys().cloned().collect::<HashSet<_>>();
    for (contract_id, rgb_assignment) in rgb_assignments {
        let mut main_builder =
            stock.transition_builder(contract_id, transition_name.clone())?;

        let assignment_id = main_builder
            .assignment_type(assignment_name.clone());

        let mut main_inputs = Vec::<OutputSeal>::new();
        let mut sum_inputs = Amount::ZERO;
        for (output, list) in
            stock.contract_assignments_for(contract_id, prev_outputs.iter().copied())?
        {
            main_inputs.push(output);
            for (opout, state) in list {
                main_builder = main_builder.add_input(opout, state.clone()).unwrap();
                if opout.ty != assignment_id {
                    // TODO: update blinding
                    // let seal = output_for_assignment(contract_id, opout.ty)?;
                    // state.update_blinding(pedersen_blinder(contract_id, assignment_id));

                    // main_builder = main_builder.add_owned_state_raw(opout.ty, seal, state).unwrap();

                    main_builder = main_builder
                        .add_owned_state_raw(opout.ty, change_seal.expect("no change seal"), state)
                        .unwrap();
                } else if let AllocatedState::Amount(value) = state {
                    sum_inputs += value.into();
                } else {
                    unimplemented!()
                }
            }
        }

        let amount_needed: u64 = rgb_assignment.values().sum();
        if sum_inputs.value() < amount_needed {
            return Err(ComposeError::InsufficientState.into());
        }

        for (beneficiary, amount) in rgb_assignment {
            // let blinding_beneficiary = pedersen_blinder(contract_id, assignment_id);
            // let blinding_beneficiary = get_blinding_factor(rng);

            main_builder = main_builder.add_fungible_state_raw(
                assignment_id,
                beneficiary,
                amount,
            )?;
        }

        let change_amount = sum_inputs - amount_needed.into();
        if change_amount > Amount::ZERO {
            // let blinding_change = BlindingFactor::random();
            // let blinding_change = get_blinding_factor(rng);
            main_builder = main_builder.add_fungible_state_raw(
                assignment_id,
                change_seal.expect("no change seal for change amount"),
                change_amount,
            )?;
        }

        let transition = main_builder.complete_transition()?;
        transition_list.push(transition);
    }

    let mut spent_state =
        HashMap::<ContractId, HashMap<OutputSeal, HashMap<Opout, AllocatedState>>>::new();
    for id in stock.contracts_assigning(prev_outputs.iter().copied())? {
        // Skip handled contracts
        if handled_contract_ids.contains(&id) {
            continue;
        }
        let state = stock.contract_assignments_for(id, prev_outputs.iter().copied())?;
        let entry = spent_state.entry(id).or_default();
        for (seal, assigns) in state {
            entry.entry(seal).or_default().extend(assigns);
        }
    }

    for (id, list) in spent_state {
        let mut blank_builder_opret = stock.transition_builder(id, transition_name.clone())?;
        let mut outputs_opret = Vec::with_capacity(list.len());
        for (output, assigns) in list {
            outputs_opret.push(output);

            for (opout, state) in assigns {
                // let seal = output_for_assignment(id, opout.ty)?;

                blank_builder_opret = blank_builder_opret
                    .add_input(opout, state.clone())?
                    .add_owned_state_raw(opout.ty, change_seal.expect("no change seal for blank transition"), state)?;
            }
        }

        if !blank_builder_opret.has_inputs() {
            continue;
        }
        let transition = blank_builder_opret.complete_transition()?;
        transition_list.push(transition);
    }

    // TODO:
    // check the priority's usage, see also:
    // https://github.com/RGB-WG/RFC/issues/10
    transition_list
        .iter_mut()
        .for_each(|t| t.nonce = u64::MAX);

    Ok(transition_list)
}

#[derive(Debug)]
pub struct PartialFascia {
    merkle_block: mpc::MerkleBlock,
    dbc_proof: rgbstd::validation::DbcProof,
    bundles: NonEmptyOrdMap<ContractId, TransitionBundle, U24>,
}

impl PartialFascia {
    #[must_use]
    pub fn complete_with_tx(self, consensus_serialized_tx: &[u8]) -> Fascia {
        let tx = Tx::consensus_deserialize(consensus_serialized_tx).unwrap();
        let seal_witness = SealWitness {
            public: PubWitness::Tx(tx),
            merkle_block: self.merkle_block,
            dbc_proof: self.dbc_proof,
        };
        Fascia {
            seal_witness,
            bundles: self.bundles,
        }
    }

    #[must_use]
    pub fn complete_with_txid(self, txid: impl Into<crate::types::Txid>) -> Fascia {
        let txid = txid.into();
        let seal_witness = SealWitness {
            public: PubWitness::Txid(txid),
            merkle_block: self.merkle_block,
            dbc_proof: self.dbc_proof,
        };
        Fascia {
            seal_witness,
            bundles: self.bundles,
        }
    }
}

pub(crate) fn rgb_commit(
    _finalized_txins: &[Outpoint],
    transition_list: Vec<Transition>,
) -> (mpc::Commitment, PartialFascia) {
    let contract_ids: Vec<ContractId> = transition_list
        .iter()
        .map(|t| t.contract_id)
        .collect();

    let (mut input_maps, mut known_transitions) = {
        let mut input_maps: HashMap<ContractId, BTreeMap<Opout, OpId>> = HashMap::new();
        let mut known_transitions: HashMap<ContractId, Vec<KnownTransition>> = HashMap::new();

        for transition in &transition_list {
            let contract_id = transition.contract_id;

            for opout in &transition.inputs {
                input_maps
                    .entry(contract_id)
                    .or_default()
                    .entry(opout)
                    .or_insert(transition.id());
                known_transitions
                    .entry(contract_id)
                    .or_default()
                    .push(KnownTransition::new(
                        transition.id(),
                        transition.clone(),
                    ));
            }
        }
        // TODO: This impl skips double spend check. But since it's constructed by us, it should be fine.
        known_transitions
            .values_mut()
            .for_each(|transitions|{
                transitions.sort_by_key(|t| t.opid);
                transitions.dedup_by_key(|t| t.opid);

            });
        (input_maps, known_transitions)
    };

    // let transition_map = {
    //     let mut transition_map: HashMap<_, Transition> = HashMap::new();
    //     for transition in transition_list {
    //         transition_map.insert(transition., transition);
    //     }
    //     transition_map
    // };

    let mut contract_bundles: BTreeMap<ContractId, TransitionBundle> = BTreeMap::new();
    for contract_id in contract_ids {
        // let mut input_map = BTreeMap::<Opout, OpId>::new();
        // let mut known_transitions = Vec::<KnownTransition>::new();

        let input_map = input_maps.remove(&contract_id).unwrap();
        let known_transitions = known_transitions.remove(&contract_id).unwrap();

        let bundle = TransitionBundle {
            input_map: Confined::try_from(input_map).unwrap(), // .map_err(|_| RgbPsbtError::NoTransitions(contract_id))?
            known_transitions: Confined::try_from(known_transitions).unwrap(), // .map_err(|_| RgbPsbtError::NoTransitions(contract_id))?,
        };
        contract_bundles.insert(contract_id, bundle);
    }

    let merkle_tree = {
        let mpc_messages: BTreeMap<mpc::ProtocolId, mpc::Message> = contract_bundles
            .iter()
            .map(|(cid, bundle)| {
                let protocol_id = mpc::ProtocolId::from(*cid);
                let message = mpc::Message::from(bundle.bundle_id());
                (protocol_id, message)
            })
            .collect();

        let min_depth = MPC_MINIMAL_DEPTH;
        let source = mpc::MultiSource {
            min_depth,
            messages: Confined::try_from(mpc_messages).unwrap(),
            // TODO: set entropy
            static_entropy: Some(0),
        };
        mpc::MerkleTree::try_commit(&source).unwrap()
    };

    let commitment = merkle_tree.commit_id();
    let partial_fascia = {
        let merkle_block = mpc::MerkleBlock::from(merkle_tree);
        let dbc_proof = DbcProof::from(OpretProof::default());
        let bundles =
            Confined::<BTreeMap<ContractId, TransitionBundle>, 1, U24>::try_from(contract_bundles)
                .unwrap();

        PartialFascia {
            merkle_block,
            dbc_proof,
            bundles,
        }
    };

    (commitment, partial_fascia)
}

pub(crate) fn rgb_issue(
    issuer: &str,
    ticker: &str,
    name: &str,
    details: Option<&str>,
    precision: u8,
    allocations: impl IntoIterator<Item = (String, u64)>,
    chain_net: ChainNet,
) -> ValidContract {
    use rgbstd::contract::IssuerWrapper;

    let issuer = Identity::from_str(issuer).unwrap();
    let precision = Precision::try_from(precision).unwrap();

    let spec = AssetSpec::with(ticker, name, precision, details).unwrap();
    let terms = ContractTerms {
        text: RicardianContract::default(),
        media: None,
    };

    let schema = NonInflatableAsset::schema();
    let scripts = NonInflatableAsset::scripts();
    let types = NonInflatableAsset::types();

    let mut builder = ContractBuilder::with(issuer, schema, types, scripts, chain_net);
    builder = builder
        .add_global_state("spec", spec)
        .expect("invalid RGB20 schema (token specification mismatch)");

    let mut issued = 0u64;
    for (seal, amount) in allocations {
        issued = issued.checked_add(amount).unwrap();

        let seal = OutputSeal::from_str(&seal).unwrap();
        let seal = GenesisSeal::new_random(seal.txid, seal.vout);
        let seal = BuilderSeal::Revealed(seal);

        builder = builder
            .add_fungible_state("assetOwner", seal, amount)
            .expect("invalid fungible state data");
    }

    builder = builder
        .add_global_state("issuedSupply", Amount::from(issued))
        .unwrap()
        .add_global_state("terms", terms)
        .unwrap();
    
    builder.issue_contract().unwrap()
}


pub(crate) fn rgb_transfer<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
    outputs: &[OutputSeal],
    secret_seal: Option<SecretSeal>,
    witness_id: Option<Txid>,
) -> Transfer {
    let secret_seals = if let Some(secret_seal) = secret_seal {
        vec![secret_seal]
    } else {
        vec![]
    };
    stock.transfer(contract_id, outputs, secret_seals, witness_id).unwrap()
}

// #[inline]
// fn get_blinding_factor<R: Rng>(rng: &mut R) -> BlindingFactor {
//     let mut failed = 0;
//     loop {
//         let blind: [u8; 32] = rng.gen();
//         match BlindingFactor::try_from(blind) {
//             Ok(blind) => break blind,
//             Err(_) => {
//                 if failed < 5 {
//                     failed += 1;
//                     continue
//                 } else {
//                     panic!("RNG is broken");
//                 }
//             }
//         }
//     }
// }