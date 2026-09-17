//! Explicit finite QCL models and weak-playability validation.
//!
//! Effectivity entries retain only minimal outcome sets. Their upward closure
//! is queried semantically by [`crate::model::Effectivity::can_enforce`].

use std::collections::{HashMap, HashSet};
use std::fmt;

use thiserror::Error;

use crate::domain::{AgentId, AtomId, Coalition, StateId, StateSet};
use crate::symbols::SymbolTable;

/// The minimal outcomes stored for one state/coalition pair.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutcomeAntichain {
    outcomes: Vec<StateSet>,
}

impl OutcomeAntichain {
    /// Create an empty set of minimal outcomes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an outcome and removes every minimal outcome it dominates.
    ///
    /// Returns `true` when the antichain changed. Equal outcomes and outcomes
    /// containing an existing minimal outcome are ignored.
    pub fn insert(&mut self, outcome: StateSet) -> bool {
        if self
            .outcomes
            .iter()
            .any(|minimal| minimal.is_subset(&outcome))
        {
            return false;
        }
        self.outcomes.retain(|minimal| !outcome.is_subset(minimal));
        self.outcomes.push(outcome);
        true
    }

    /// Return whether no minimal outcomes are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    /// Return the number of minimal outcomes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.outcomes.len()
    }

    /// Borrow the minimal outcomes as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[StateSet] {
        &self.outcomes
    }

    /// Iterate over borrowed minimal outcomes.
    pub fn iter(&self) -> impl Iterator<Item = &StateSet> {
        self.outcomes.iter()
    }

    /// Test whether one minimal outcome is contained in `target`.
    ///
    /// Since outcomes are stored minimally, this query implements their
    /// implicit upward closure.
    #[must_use]
    pub fn can_enforce(&self, target: &StateSet) -> bool {
        self.outcomes
            .iter()
            .any(|minimal| minimal.is_subset(target))
    }
}

/// Explicit effectivity, indexed by source state and acting coalition.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Effectivity {
    entries: HashMap<(StateId, Coalition), OutcomeAntichain>,
}

impl Effectivity {
    /// Create an empty explicit effectivity function.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a minimal candidate outcome. Dominated outcomes are discarded.
    pub fn insert(&mut self, state: StateId, coalition: Coalition, outcome: StateSet) -> bool {
        self.entries
            .entry((state, coalition))
            .or_default()
            .insert(outcome)
    }

    /// Return the minimal outcomes for a state and coalition, if any.
    #[must_use]
    pub fn outcomes(&self, state: StateId, coalition: &Coalition) -> Option<&[StateSet]> {
        self.entries
            .get(&(state, coalition.clone()))
            .map(OutcomeAntichain::as_slice)
    }

    /// Return the antichain for a state and coalition, if any.
    #[must_use]
    pub fn antichain(&self, state: StateId, coalition: &Coalition) -> Option<&OutcomeAntichain> {
        self.entries.get(&(state, coalition.clone()))
    }

    /// Iterate over all explicitly stored entries.
    pub fn iter(&self) -> impl Iterator<Item = (StateId, &Coalition, &OutcomeAntichain)> {
        self.entries
            .iter()
            .map(|((state, coalition), outcomes)| (*state, coalition, outcomes))
    }

    /// Checks the upward closure semantically, without materialising it.
    #[must_use]
    pub fn can_enforce(&self, state: StateId, coalition: &Coalition, target: &StateSet) -> bool {
        self.antichain(state, coalition)
            .is_some_and(|outcomes| outcomes.can_enforce(target))
    }
}

/// A finite, resolved QCL model.
#[derive(Clone, Debug)]
pub struct QclModel {
    /// Dense names assigned to agents.
    pub agents: SymbolTable<AgentId>,
    /// Dense names assigned to states.
    pub states: SymbolTable<StateId>,
    /// Dense names assigned to propositions.
    pub atoms: SymbolTable<AtomId>,
    /// Propositions true at each state. Missing states have an empty valuation.
    pub valuation: HashMap<StateId, HashSet<AtomId>>,
    /// Minimal enforceable outcomes indexed by source state and coalition.
    pub effectivity: Effectivity,
}

impl QclModel {
    /// Construct a resolved finite model from its symbol tables and data.
    ///
    /// The constructor does not validate referential integrity or
    /// weak-playability; call [`ModelValidator::validate`] when accepting
    /// external model data.
    #[must_use]
    pub fn new(
        agents: SymbolTable<AgentId>,
        states: SymbolTable<StateId>,
        atoms: SymbolTable<AtomId>,
        valuation: HashMap<StateId, HashSet<AtomId>>,
        effectivity: Effectivity,
    ) -> Self {
        Self {
            agents,
            states,
            atoms,
            valuation,
            effectivity,
        }
    }

    /// Return the number of states in the model.
    #[must_use]
    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    /// Return the number of agents in the model.
    #[must_use]
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    /// Return the number of propositions in the model.
    #[must_use]
    pub fn atom_count(&self) -> usize {
        self.atoms.len()
    }

    /// Test whether `atom` is true at `state`.
    #[must_use]
    pub fn is_true(&self, state: StateId, atom: AtomId) -> bool {
        self.valuation
            .get(&state)
            .is_some_and(|atoms| atoms.contains(&atom))
    }
}

/// A single referential-integrity or weak-playability violation.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ModelValidationError {
    /// The model has no states.
    #[error("model has no states")]
    EmptyStateSpace,
    /// A valuation entry names a state not in the state table.
    #[error("valuation refers to unknown state {state:?}")]
    ValuationStateOutOfRange {
        /// The invalid state ID.
        state: StateId,
    },
    /// A valuation entry names an atom not in the atom table.
    #[error("valuation refers to unknown atom {atom:?} at state {state:?}")]
    ValuationAtomOutOfRange {
        /// State containing the invalid atom reference.
        state: StateId,
        /// The invalid atom ID.
        atom: AtomId,
    },
    /// An effectivity entry names a source state not in the state table.
    #[error("effectivity refers to unknown source state {state:?}")]
    EffectivityStateOutOfRange {
        /// The invalid source state.
        state: StateId,
    },
    /// An effectivity coalition names an agent not in the agent table.
    #[error("effectivity coalition refers to unknown agent {agent:?} at state {state:?}")]
    EffectivityAgentOutOfRange {
        /// State containing the invalid coalition reference.
        state: StateId,
        /// The invalid agent ID.
        agent: AgentId,
    },
    /// An effectivity outcome names a state not in the state table.
    #[error("effectivity outcome refers to unknown state {outcome:?} from state {state:?}")]
    EffectivityOutcomeOutOfRange {
        /// Source state containing the invalid outcome reference.
        state: StateId,
        /// The invalid outcome state.
        outcome: StateId,
    },
    /// The grand coalition can enforce the empty outcome.
    #[error("empty outcome is enforceable by the grand coalition at state {state:?}")]
    GrandCoalitionEnforcesEmpty {
        /// State where the violation occurs.
        state: StateId,
    },
    /// Empty-outcome enforcement is not downward closed under subcoalitions.
    #[error(
        "empty-outcome enforcement is not propagated from {superset:?} to {subset:?} at state {state:?}"
    )]
    EmptyOutcomeNotDownwardClosed {
        /// State where the violation occurs.
        state: StateId,
        /// Coalition that enforces the empty outcome.
        superset: Coalition,
        /// Subcoalition that should also enforce the empty outcome.
        subset: Coalition,
    },
    /// A coalition cannot enforce the universe despite liveness requiring it.
    #[error(
        "liveness fails at state {state:?}: coalition {coalition:?} cannot enforce the universe"
    )]
    Liveness {
        /// State where the violation occurs.
        state: StateId,
        /// Coalition that cannot enforce all states.
        coalition: Coalition,
    },
    /// Ag-maximality fails for an outcome at a state.
    #[error("Ag-maximality fails at state {state:?} for outcome {outcome:?}")]
    AgMaximality {
        /// State where the violation occurs.
        state: StateId,
        /// Outcome that is not enforceable by the grand coalition.
        outcome: StateSet,
    },
    /// Superadditivity fails for two disjoint coalitions.
    #[error(
        "superadditivity fails at state {state:?} for disjoint coalitions {left:?} and {right:?}"
    )]
    Superadditivity {
        /// State where the violation occurs.
        state: StateId,
        /// First disjoint coalition.
        left: Coalition,
        /// Second disjoint coalition.
        right: Coalition,
        /// Minimal outcome for `left`.
        left_outcome: StateSet,
        /// Minimal outcome for `right`.
        right_outcome: StateSet,
    },
}

/// A collection of validation failures, retaining every independently useful
/// diagnostic instead of stopping at the first malformed entry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelValidationErrors {
    errors: Vec<ModelValidationError>,
}

impl ModelValidationErrors {
    /// Construct a collection of validation errors.
    #[must_use]
    pub fn new(errors: Vec<ModelValidationError>) -> Self {
        Self { errors }
    }

    /// Borrow all errors as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[ModelValidationError] {
        &self.errors
    }

    /// Iterate over validation errors in discovery order.
    pub fn iter(&self) -> impl Iterator<Item = &ModelValidationError> {
        self.errors.iter()
    }

    /// Return whether no validation errors were collected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

impl fmt::Display for ModelValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.errors.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            error.fmt(formatter)?;
        }
        Ok(())
    }
}

impl std::error::Error for ModelValidationErrors {}

/// Checks referential integrity and weak-playability of an explicit model.
pub struct ModelValidator;

impl ModelValidator {
    ///
    /// # Errors
    ///
    /// Returns every referential-integrity or weak-playability violation.
    pub fn validate(model: &QclModel) -> Result<(), ModelValidationErrors> {
        let mut errors = Vec::new();
        Self::validate_references(model, &mut errors);
        if errors.is_empty() {
            Self::validate_playability(model, &mut errors);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ModelValidationErrors::new(errors))
        }
    }

    fn validate_references(model: &QclModel, errors: &mut Vec<ModelValidationError>) {
        if model.state_count() == 0 {
            errors.push(ModelValidationError::EmptyStateSpace);
        }
        for (state, atoms) in &model.valuation {
            if state.index() >= model.state_count() {
                errors.push(ModelValidationError::ValuationStateOutOfRange { state: *state });
                continue;
            }
            for atom in atoms {
                if atom.index() >= model.atom_count() {
                    errors.push(ModelValidationError::ValuationAtomOutOfRange {
                        state: *state,
                        atom: *atom,
                    });
                }
            }
        }
        for (state, coalition, outcomes) in model.effectivity.iter() {
            if state.index() >= model.state_count() {
                errors.push(ModelValidationError::EffectivityStateOutOfRange { state });
            }
            for agent in coalition.iter() {
                if agent.index() >= model.agent_count() {
                    errors.push(ModelValidationError::EffectivityAgentOutOfRange { state, agent });
                }
            }
            for outcome in outcomes.iter().flat_map(StateSet::iter) {
                if outcome.index() >= model.state_count() {
                    errors.push(ModelValidationError::EffectivityOutcomeOutOfRange {
                        state,
                        outcome,
                    });
                }
            }
        }
    }

    fn validate_playability(model: &QclModel, errors: &mut Vec<ModelValidationError>) {
        let grand = Coalition::from_agents((0..model.agent_count()).map(AgentId::new));
        let empty_coalition = Coalition::empty();
        let empty_outcome = StateSet::empty();
        let universe = StateSet::from_states((0..model.state_count()).map(StateId::new));

        for state in (0..model.state_count()).map(StateId::new) {
            // Empty/grand consistency: the grand coalition cannot enforce ⊥.
            if model.effectivity.can_enforce(state, &grand, &empty_outcome) {
                errors.push(ModelValidationError::GrandCoalitionEnforcesEmpty { state });
            }

            // Downward propagation of empty: if C can enforce ⊥, every subset can.
            for coalition in Coalition::all(model.agent_count()) {
                if !model
                    .effectivity
                    .can_enforce(state, &coalition, &empty_outcome)
                {
                    continue;
                }
                for subset in Coalition::all(model.agent_count()) {
                    if subset.is_subset(&coalition)
                        && !model
                            .effectivity
                            .can_enforce(state, &subset, &empty_outcome)
                    {
                        errors.push(ModelValidationError::EmptyOutcomeNotDownwardClosed {
                            state,
                            superset: coalition.clone(),
                            subset,
                        });
                    }
                }
            }

            // Liveness/S condition: absent ⊥ for ∅, every coalition can enforce S.
            if !model
                .effectivity
                .can_enforce(state, &empty_coalition, &empty_outcome)
            {
                for coalition in Coalition::all(model.agent_count()) {
                    if !model.effectivity.can_enforce(state, &coalition, &universe) {
                        errors.push(ModelValidationError::Liveness { state, coalition });
                    }
                }
            }

            // Ag-maximality: ¬(S\X ∈ E(∅)) implies X ∈ E(Ag).
            for outcome in all_state_sets(model.state_count()) {
                let complement = complement(&universe, &outcome);
                if !model
                    .effectivity
                    .can_enforce(state, &empty_coalition, &complement)
                    && !model.effectivity.can_enforce(state, &grand, &outcome)
                {
                    errors.push(ModelValidationError::AgMaximality { state, outcome });
                }
            }

            // Superadditivity is checked on minimal outcomes; monotonicity makes
            // this sufficient for the upward closures represented by the model.
            let coalitions: Vec<_> = Coalition::all(model.agent_count()).collect();
            for left in &coalitions {
                for right in &coalitions {
                    if !left_is_disjoint(left, right) {
                        continue;
                    }
                    let Some(left_outcomes) = model.effectivity.outcomes(state, left) else {
                        continue;
                    };
                    let Some(right_outcomes) = model.effectivity.outcomes(state, right) else {
                        continue;
                    };
                    for left_outcome in left_outcomes {
                        for right_outcome in right_outcomes {
                            let intersection = intersection(left_outcome, right_outcome);
                            let union = union(left, right);
                            if !model.effectivity.can_enforce(state, &union, &intersection) {
                                errors.push(ModelValidationError::Superadditivity {
                                    state,
                                    left: left.clone(),
                                    right: right.clone(),
                                    left_outcome: left_outcome.clone(),
                                    right_outcome: right_outcome.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
}

fn all_state_sets(state_count: usize) -> Vec<StateSet> {
    let mut sets = vec![StateSet::empty()];
    for state in (0..state_count).map(StateId::new) {
        let additions: Vec<_> = sets
            .iter()
            .cloned()
            .map(|mut set| {
                set.insert(state);
                set
            })
            .collect();
        sets.extend(additions);
    }
    sets
}

fn complement(universe: &StateSet, set: &StateSet) -> StateSet {
    StateSet::from_states(universe.iter().filter(|state| !set.contains(*state)))
}

fn intersection(left: &StateSet, right: &StateSet) -> StateSet {
    StateSet::from_states(left.iter().filter(|state| right.contains(*state)))
}

fn union(left: &Coalition, right: &Coalition) -> Coalition {
    Coalition::from_agents(left.iter().chain(right.iter()))
}

fn left_is_disjoint(left: &Coalition, right: &Coalition) -> bool {
    left.iter().all(|agent| !right.contains(agent))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> (
        SymbolTable<AgentId>,
        SymbolTable<StateId>,
        SymbolTable<AtomId>,
    ) {
        let mut agents = SymbolTable::new();
        agents.insert("a").unwrap();
        agents.insert("b").unwrap();
        let mut states = SymbolTable::new();
        states.insert("s0").unwrap();
        states.insert("s1").unwrap();
        (agents, states, SymbolTable::new())
    }

    fn model(effectivity: Effectivity) -> QclModel {
        let (agents, states, atoms) = tables();
        QclModel::new(agents, states, atoms, HashMap::new(), effectivity)
    }

    #[test]
    fn antichain_removes_dominated_outcomes() {
        let mut antichain = OutcomeAntichain::new();
        let singleton = StateSet::singleton(StateId::new(0));
        let larger = StateSet::from_states([StateId::new(0), StateId::new(1)]);
        assert!(antichain.insert(larger.clone()));
        assert!(antichain.insert(singleton.clone()));
        assert_eq!(antichain.as_slice(), &[singleton]);
        assert!(!antichain.insert(larger));
    }

    #[test]
    fn effectivity_uses_upward_closure_without_storing_it() {
        let coalition = Coalition::singleton(AgentId::new(0));
        let target = StateSet::from_states([StateId::new(0), StateId::new(1)]);
        let mut effectivity = Effectivity::new();
        effectivity.insert(
            StateId::new(0),
            coalition.clone(),
            StateSet::singleton(StateId::new(0)),
        );
        assert_eq!(
            effectivity
                .outcomes(StateId::new(0), &coalition)
                .unwrap()
                .len(),
            1
        );
        assert!(effectivity.can_enforce(StateId::new(0), &coalition, &target));
    }

    #[test]
    fn validator_reports_liveness_and_grand_empty() {
        let mut effectivity = Effectivity::new();
        let grand = Coalition::from_agents([AgentId::new(0), AgentId::new(1)]);
        effectivity.insert(StateId::new(0), grand, StateSet::empty());
        let errors = ModelValidator::validate(&model(effectivity)).unwrap_err();
        assert!(errors.iter().any(|error| matches!(
            error,
            ModelValidationError::GrandCoalitionEnforcesEmpty { .. }
        )));
    }

    #[test]
    fn validator_accepts_small_playable_model() {
        let mut effectivity = Effectivity::new();
        let universe = StateSet::from_states([StateId::new(0), StateId::new(1)]);
        for coalition in Coalition::all(2) {
            effectivity.insert(StateId::new(0), coalition.clone(), universe.clone());
            effectivity.insert(StateId::new(1), coalition, universe.clone());
        }
        let grand = Coalition::from_agents([AgentId::new(0), AgentId::new(1)]);
        for state in [StateId::new(0), StateId::new(1)] {
            effectivity.insert(state, grand.clone(), StateSet::singleton(StateId::new(0)));
            effectivity.insert(state, grand.clone(), StateSet::singleton(StateId::new(1)));
        }
        assert!(ModelValidator::validate(&model(effectivity)).is_ok());
    }

    #[test]
    fn validator_rejects_non_superadditive_entries() {
        let mut effectivity = Effectivity::new();
        effectivity.insert(
            StateId::new(0),
            Coalition::singleton(AgentId::new(0)),
            StateSet::singleton(StateId::new(0)),
        );
        effectivity.insert(
            StateId::new(0),
            Coalition::singleton(AgentId::new(1)),
            StateSet::singleton(StateId::new(1)),
        );
        let errors = ModelValidator::validate(&model(effectivity)).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, ModelValidationError::Superadditivity { .. }))
        );
    }
}
