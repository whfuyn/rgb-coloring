// TODO: error handling

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::str::FromStr;

use amplify::bmap;
use amplify::confinement::NonEmptyOrdMap;
use amplify::confinement::SmallOrdMap;
use amplify::confinement::{Confined, U24};
use bp::dbc::{opret::OpretProof, Anchor};
use bp::seals::txout::CloseMethod;
use commit_verify::mpc::{self, MPC_MINIMAL_DEPTH};
use commit_verify::CommitId as _;
use commit_verify::TryCommitVerify;
use ifaces::IssuerWrapper;
use ifaces::Rgb20;
use ifaces::SchemaIssuer;
use rgbstd::containers::BundleDichotomy;
use rgbstd::containers::ConsignmentExt;
use rgbstd::containers::Fascia;
use rgbstd::containers::PubWitness;
use rgbstd::containers::Transfer;
use rgbstd::containers::TransitionInfoError;
use rgbstd::containers::ValidContract;
use rgbstd::interface::BuilderError;
use rgbstd::interface::ContractBuilder;
use rgbstd::interface::IfaceClass;
use rgbstd::persistence::ComposeError;
use rgbstd::persistence::PersistedState;
use rgbstd::persistence::StockError;
use rgbstd::stl::AssetSpec;
use rgbstd::stl::ContractTerms;
use rgbstd::stl::RicardianContract;
use rgbstd::Amount;
use rgbstd::BlindingFactor;
use rgbstd::GenesisSeal;
use rgbstd::Identity;
use rgbstd::Opout;
use rgbstd::OutputSeal;
use rgbstd::Precision;
use rgbstd::StateType;
use rgbstd::Transition;
use rgbstd::TransitionBundle;
use rgbstd::Vin;
use rgbstd::XChain;
use rgbstd::{
    validation::ResolveWitness,
    containers::{AnchorSet, BuilderSeal, TransitionInfo},
    persistence::{IndexProvider, StashProvider, StateProvider, Stock},
    ContractId, GraphSeal, InputMap, OpId, XOutpoint, XOutputSeal,
};
use schemata::NonInflatableAsset;
use strict_encoding::FieldName;
use strict_types::encoding::TypeName;
use strict_types::value::typify::TypedVal;

use bp::{ConsensusDecode as _, Tx};
use strict_types::StrictVal;

pub(crate) type RgbAssignments = HashMap<ContractId, HashMap<Beneficiary, u64>>;
pub(crate) type Beneficiary = BuilderSeal<GraphSeal>;

pub(crate) fn rgb_balance<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
    utxos: &[XOutpoint],
) -> u64 {
    let iface_name = TypeName::from("RGB20Fixed");
    let iface = stock.iface(iface_name.clone()).unwrap();
    let operation = iface.default_operation.as_ref().unwrap();

    let assignment_name = iface
        .transitions
        .get(operation)
        .and_then(|t| t.default_assignment.clone())
        .unwrap();

    let contract = stock
        .contract_iface(contract_id, iface_name.clone())
        .unwrap();
    // .map_err(|e| e.to_string())?;

    let amount = contract
        .fungible(assignment_name, utxos)
        .unwrap()
        .map(|a| a.state)
        .sum::<Amount>();

    amount.into()
}

pub(crate) fn rgb_coin_select<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    available_utxos: &[XOutpoint],
    rgb_assignments: &RgbAssignments,
) -> Vec<XOutputSeal> {
    // Only support RGB20Fixed for now.
    let iface_name = TypeName::from("RGB20Fixed");
    let iface = stock.iface(iface_name.clone()).unwrap();
    let operation = iface.default_operation.as_ref().unwrap();

    let assignment_name = iface
        .transitions
        .get(operation)
        .and_then(|t| t.default_assignment.clone())
        .unwrap();

    let mut selected_prev_outputs: Vec<XOutputSeal> = vec![];
    for (&contract_id, rgb_assignment) in rgb_assignments {
        let total_amount_needed: u64 = rgb_assignment.iter().map(|(_, amount)| *amount).sum();

        let contract = stock
            .contract_iface(contract_id, iface_name.clone())
            .unwrap();
        // .map_err(|e| e.to_string())?;

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
            let mut sum = Amount::ZERO;
            state
                .iter()
                .rev()
                .take_while(|(val, _, _)| {
                    if sum >= total_amount_needed.into() {
                        false
                    } else {
                        sum += *val;
                        true
                    }
                })
                .map(|(_, seal, _)| *seal)
                .collect::<BTreeSet<XOutputSeal>>()
        };

        selected_prev_outputs.extend(prev_outputs);
    }

    selected_prev_outputs.sort();
    selected_prev_outputs.dedup();

    selected_prev_outputs
}

pub(crate) fn rgb_compose<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    prev_outputs: impl IntoIterator<Item = impl Into<XOutputSeal>>,
    rgb_assignments: RgbAssignments,
    change_seal: Beneficiary,
) -> Result<Vec<TransitionInfo>, StockError<S, H, P, ComposeError>> {
    let prev_outputs = prev_outputs
        .into_iter()
        .map(|o| o.into())
        .collect::<HashSet<XOutputSeal>>();

    // Only support RGB20Fixed for now.
    let iface_name = TypeName::from("RGB20Fixed");
    let iface = stock.iface(iface_name.clone()).unwrap();
    let operation = iface.default_operation.as_ref().unwrap();

    let mut transition_info_list: Vec<TransitionInfo> = vec![];

    let handled_contract_ids = rgb_assignments.keys().cloned().collect::<HashSet<_>>();
    for (contract_id, rgb_assignment) in rgb_assignments {
        let mut main_builder =
            stock.transition_builder(contract_id, iface_name.clone(), Some(operation.clone()))?;

        let assignment_name = main_builder
            .default_assignment()
            .ok()
            .ok_or(BuilderError::NoDefaultAssignment)?;
        let assignment_id = main_builder
            .assignments_type(assignment_name)
            .ok_or(BuilderError::InvalidStateField(assignment_name.clone()))?;

        let mut main_inputs = Vec::<XOutputSeal>::new();
        let mut sum_inputs = Amount::ZERO;
        for (output, list) in
            stock.contract_assignments_for(contract_id, prev_outputs.iter().copied())?
        {
            main_inputs.push(output);
            for (opout, state) in list {
                main_builder = main_builder.add_input(opout, state.clone()).unwrap();
                if opout.ty != assignment_id {
                    // TODO: update bliding
                    // let seal = output_for_assignment(contract_id, opout.ty)?;
                    // state.update_blinding(pedersen_blinder(contract_id, assignment_id));

                    // main_builder = main_builder.add_owned_state_raw(opout.ty, seal, state).unwrap();

                    main_builder = main_builder
                        .add_owned_state_raw(opout.ty, change_seal, state)
                        .unwrap();
                } else if let PersistedState::Amount(value, _, _) = state {
                    sum_inputs += value;
                } else if let PersistedState::Data(_value, _) = state {
                    todo!()
                }
            }
        }

        let amount_needed: u64 = rgb_assignment.values().sum();
        if sum_inputs.value() < amount_needed {
            return Err(ComposeError::InsufficientState.into());
        }

        for (beneficiary, amount) in rgb_assignment {
            // let blinding_beneficiary = pedersen_blinder(contract_id, assignment_id);
            let blinding_beneficiary = BlindingFactor::EMPTY;

            main_builder = main_builder.add_fungible_state_raw(
                assignment_id,
                beneficiary,
                amount,
                blinding_beneficiary,
            )?;
        }

        let change_amount = sum_inputs - amount_needed.into();
        if change_amount > Amount::ZERO {
            let blinding_change = BlindingFactor::EMPTY;
            main_builder = main_builder.add_fungible_state_raw(
                assignment_id,
                change_seal,
                change_amount,
                blinding_change,
            )?;
        }

        let transition = main_builder.complete_transition()?;
        let transition_info =
            TransitionInfo::new(transition, main_inputs).unwrap();

        transition_info_list.push(transition_info);
    }

    let mut spent_state =
        HashMap::<ContractId, HashMap<XOutputSeal, HashMap<Opout, PersistedState>>>::new();
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
        let mut blank_builder_opret = stock.blank_builder(id, iface_name.clone())?;
        let mut outputs_opret = Vec::with_capacity(list.len());
        for (output, assigns) in list {
            outputs_opret.push(output);

            for (opout, state) in assigns {
                // let seal = output_for_assignment(id, opout.ty)?;

                blank_builder_opret = blank_builder_opret
                    .add_input(opout, state.clone())?
                    .add_owned_state_raw(opout.ty, change_seal, state)?;
            }
        }

        if !blank_builder_opret.has_inputs() {
            continue;
        }
        let transition = blank_builder_opret.complete_transition()?;
        let info = TransitionInfo::new(transition, outputs_opret).map_err(|e| {
            debug_assert!(!matches!(e, TransitionInfoError::CloseMethodDivergence(_)));
            ComposeError::TooManyInputs
        })?;
        transition_info_list.push(info);
    }

    // TODO:
    // check the priority's usage, see also:
    // https://github.com/RGB-WG/RFC/issues/10
    transition_info_list
        .iter_mut()
        .for_each(|ti| ti.transition.nonce = u64::MAX);

    Ok(transition_info_list)
}

#[derive(Debug)]
pub struct PartialFascia {
    anchor_set: AnchorSet,
    bundles: NonEmptyOrdMap<ContractId, BundleDichotomy, U24>,
}

impl PartialFascia {
    pub fn complete(self, consensus_serialized_tx: &[u8]) -> Fascia {
        let tx = Tx::consensus_deserialize(consensus_serialized_tx).unwrap();
        let witness = PubWitness::with(tx);
        Fascia {
            witness: XChain::with(rgbstd::Layer1::Bitcoin, witness),
            anchor: self.anchor_set,
            bundles: self.bundles,
        }
    }
}

pub(crate) fn rgb_commit(
    finalized_txins: &[XOutpoint],
    transition_info_list: Vec<TransitionInfo>,
) -> (mpc::Commitment, PartialFascia) {
    let contract_ids: Vec<ContractId> = transition_info_list
        .iter()
        .map(|ti| ti.transition.contract_id)
        .collect();

    let rgb_consumers = {
        let mut rgb_consumers: HashMap<ContractId, Vec<(OpId, Vin)>> = HashMap::new();
        for transition_info in &transition_info_list {
            let contract_id = transition_info.transition.contract_id;
            let info_opid = transition_info.id;

            for outpoint in &transition_info.inputs {
                let input_pos = finalized_txins
                    .iter()
                    .position(|txin| txin == outpoint)
                    .unwrap();
                rgb_consumers
                    .entry(contract_id)
                    .or_default()
                    .push((info_opid, Vin::from_u32(input_pos as u32)));
            }
        }
        rgb_consumers
    };

    let transition_map = {
        let mut transition_map = HashMap::new();
        for transition_info in transition_info_list {
            let transition = transition_info.transition;
            let info_opid = transition_info.id;
            transition_map.insert(info_opid, transition);
        }
        transition_map
    };

    let mut contract_bundles: BTreeMap<ContractId, BundleDichotomy> = BTreeMap::new();
    for contract_id in contract_ids {
        let mut input_map = HashMap::<CloseMethod, SmallOrdMap<Vin, OpId>>::new();
        let mut known_transitions = HashMap::<CloseMethod, SmallOrdMap<OpId, Transition>>::new();

        let rgb_consumer = rgb_consumers.get(&contract_id).unwrap();
        for &(opid, vin) in rgb_consumer {
            input_map
                .entry(CloseMethod::OpretFirst)
                .or_default()
                .insert(vin, opid)
                .unwrap();

            let Some(transition) = transition_map.get(&opid) else {
                unreachable!()
            };
            known_transitions
                .entry(CloseMethod::OpretFirst)
                .or_default()
                .insert(opid, transition.clone())
                .unwrap();
        }

        let mut bundles = vec![];
        for (method, input_map) in input_map {
            let known_transitions = known_transitions.remove(&method).unwrap_or_default();
            bundles.push(TransitionBundle {
                close_method: method,
                input_map: InputMap::from(
                    Confined::try_from(input_map.release()).unwrap(), // .map_err(|_| RgbPsbtError::NoTransitions(contract_id))?,
                ),
                known_transitions: Confined::try_from(known_transitions.release()).unwrap(), // .map_err(|_| RgbPsbtError::NoTransitions(contract_id))?,
            });
        }

        let mut bundles = bundles.into_iter();
        let first = bundles.next().unwrap();
        // .ok_or(RgbPsbtError::NoTransitions(contract_id))?;

        contract_bundles.insert(contract_id, BundleDichotomy::with(first, bundles.next()));
    }

    let merkle_tree = {
        let mpc_messages: BTreeMap<mpc::ProtocolId, mpc::Message> = contract_bundles
            .iter()
            .map(|(cid, bundles)| {
                let mut it = bundles.iter();
                let bundle = it.next().unwrap();
                debug_assert!(it.next().is_none());

                let protocol_id = mpc::ProtocolId::from(*cid);
                let message = mpc::Message::from(bundle.bundle_id());
                (protocol_id, message)
            })
            .collect();

        let min_depth = MPC_MINIMAL_DEPTH;
        let source = mpc::MultiSource {
            min_depth,
            messages: Confined::try_from(mpc_messages).unwrap(),
            static_entropy: None,
        };
        mpc::MerkleTree::try_commit(&source).unwrap()
    };

    let commitment = merkle_tree.commit_id();
    let partial_fascia = {
        let anchor_set = {
            let mpc_proof = mpc::MerkleBlock::from(merkle_tree);
            let anchor = Anchor::new(mpc_proof, OpretProof::default());
            AnchorSet::Opret(anchor)
        };
        let bundles =
            Confined::<BTreeMap<ContractId, BundleDichotomy>, 1, U24>::try_from(contract_bundles)
                .unwrap();

        PartialFascia {
            anchor_set,
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
    is_testnet: bool,
) -> ValidContract {
    let issuer = Identity::from_str(issuer).unwrap();
    let precision = Precision::try_from(precision).unwrap();

    let spec = AssetSpec::with(ticker, name, precision, details).unwrap();
    let terms = ContractTerms {
        text: RicardianContract::default(),
        media: None,
    };

    let iface = NonInflatableAsset::FEATURES.iface();
    let schema = NonInflatableAsset::schema();
    let iimpl = NonInflatableAsset::issue_impl();
    let scripts = NonInflatableAsset::scripts();
    let types = NonInflatableAsset::types();

    let mut builder = ContractBuilder::with(issuer, iface, schema, iimpl, types, scripts);
    builder = builder
        .add_global_state("spec", spec)
        .expect("invalid RGB20 schema (token specification mismatch)");

    let mut issued = 0u64;
    for (seal, amount) in allocations {
        issued = issued.checked_add(amount).unwrap();

        let seal = OutputSeal::from_str(&seal).unwrap();
        let seal = GenesisSeal::new_random(seal.method, seal.txid, seal.vout);
        let seal = BuilderSeal::Revealed(XChain::Bitcoin(seal));

        builder = builder
            .add_fungible_state("assetOwner", seal, amount)
            .expect("invalid fungible state data");
    }

    builder = builder
        .add_global_state("issuedSupply", Amount::from(issued))
        .unwrap()
        .add_global_state("terms", terms)
        .unwrap();
    
    if !is_testnet {
        builder = builder.set_mainnet();
    }

    builder.issue_contract().unwrap()
}


pub(crate) fn rgb_transfer<S: StashProvider, H: StateProvider, P: IndexProvider>(
    stock: &Stock<S, H, P>,
    contract_id: ContractId,
    outputs: &[XOutputSeal],
) -> Transfer {
    stock.transfer(contract_id, outputs, None).unwrap()
}
