//! Finite concurrent game forms and effectivity derivation.
//!
//! This module derives an explicit QCL effectivity function from an executable
//! transition relation. It deliberately knows nothing about any particular
//! domain: actions are supplied by the game form implementation.

use std::collections::HashMap;

use crate::domain::{AgentId, Coalition, StateId, StateSet};
use crate::model::Effectivity;

/// A complete action profile, ordered by dense agent ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JointActionProfile<A> {
    actions: Vec<A>,
}

impl<A> JointActionProfile<A> {
    /// Construct a profile. Entries must be ordered by dense agent ID.
    #[must_use]
    pub fn new(actions: Vec<A>) -> Self {
        Self { actions }
    }

    /// Return the action selected by `agent`, if present in this profile.
    #[must_use]
    pub fn action(&self, agent: AgentId) -> Option<&A> {
        self.actions.get(agent.index())
    }

    /// Return the number of actions in this profile.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Return whether this profile contains no actions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Iterate over actions in dense agent-ID order.
    pub fn iter(&self) -> impl Iterator<Item = &A> {
        self.actions.iter()
    }
}

impl<A> From<Vec<A>> for JointActionProfile<A> {
    fn from(actions: Vec<A>) -> Self {
        Self::new(actions)
    }
}

/// Actions selected by one coalition, used as a strategy witness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoalitionStrategy<A> {
    actions: Vec<(AgentId, A)>,
}

impl<A> CoalitionStrategy<A> {
    fn new(actions: Vec<(AgentId, A)>) -> Self {
        Self { actions }
    }

    /// Return the action selected by `agent`, if this strategy controls it.
    #[must_use]
    pub fn action(&self, agent: AgentId) -> Option<&A> {
        self.actions
            .iter()
            .find_map(|(member, action)| (*member == agent).then_some(action))
    }

    /// Iterate over `(agent, action)` pairs in ascending agent-ID order.
    pub fn iter(&self) -> impl Iterator<Item = (AgentId, &A)> {
        self.actions.iter().map(|(agent, action)| (*agent, action))
    }

    /// Return number of controlled agents represented by this strategy.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Return whether this strategy controls no agents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// One coalition strategy and every outcome it permits against outsiders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyWitness<A> {
    /// Source state at which the strategy is selected.
    pub state: StateId,
    /// Acting coalition.
    pub coalition: Coalition,
    /// Coalition's fixed action choices.
    pub strategy: CoalitionStrategy<A>,
    /// Outcomes over all completions by agents outside `coalition`.
    pub outcomes: StateSet,
}

/// An effectivity function together with executable strategy witnesses.
#[derive(Clone, Debug)]
pub struct DerivedEffectivity<A> {
    /// Minimal outcome sets derived from the transition function.
    pub effectivity: Effectivity,
    witnesses: HashMap<(StateId, Coalition), Vec<StrategyWitness<A>>>,
}

impl<A> DerivedEffectivity<A> {
    /// Return the underlying explicit effectivity function.
    #[must_use]
    pub const fn as_effectivity(&self) -> &Effectivity {
        &self.effectivity
    }

    /// Return all strategy witnesses enumerated for a state and coalition.
    #[must_use]
    pub fn witnesses(
        &self,
        state: StateId,
        coalition: &Coalition,
    ) -> Option<&[StrategyWitness<A>]> {
        self.witnesses
            .get(&(state, coalition.clone()))
            .map(Vec::as_slice)
    }

    /// Return one witness whose outcome set is contained in `target`.
    #[must_use]
    pub fn find_witness(
        &self,
        state: StateId,
        coalition: &Coalition,
        target: &StateSet,
    ) -> Option<&StrategyWitness<A>> {
        self.witnesses(state, coalition)?
            .iter()
            .find(|witness| witness.outcomes.is_subset(target))
    }

    /// Test effectivity using the derived minimal outcome antichains.
    #[must_use]
    pub fn can_enforce(&self, state: StateId, coalition: &Coalition, target: &StateSet) -> bool {
        self.effectivity.can_enforce(state, coalition, target)
    }
}

/// A finite deterministic concurrent game form.
pub trait FiniteGameForm {
    /// Action type chosen by each agent.
    type Action: Clone;

    /// Number of agents in the game form.
    fn agent_count(&self) -> usize;

    /// Number of states in the game form.
    fn state_count(&self) -> usize;

    /// Enumerate actions available to one agent at one state.
    fn actions(&self, state: StateId, agent: AgentId) -> Vec<Self::Action>;

    /// Apply one complete joint action profile.
    fn transition(&self, state: StateId, profile: &JointActionProfile<Self::Action>) -> StateId;
}

/// Derive effectivity and executable strategy witnesses from `game`.
///
/// # Panics
///
/// Panics only if the internal profile assembly invariant is violated. Every
/// profile assembled by this function covers each agent, so a well-formed
/// [`FiniteGameForm`] cannot trigger this panic.
pub fn derive_effectivity<G>(game: &G) -> DerivedEffectivity<G::Action>
where
    G: FiniteGameForm,
{
    let mut derived = DerivedEffectivity {
        effectivity: Effectivity::new(),
        witnesses: HashMap::new(),
    };

    for state in (0..game.state_count()).map(StateId::new) {
        for coalition in Coalition::all(game.agent_count()) {
            let members: Vec<_> = coalition.iter().collect();
            let outsiders: Vec<_> = (0..game.agent_count())
                .map(AgentId::new)
                .filter(|agent| !coalition.contains(*agent))
                .collect();
            let coalition_profiles = enumerate_assignments(game, state, &members);
            let witness_entries = derived
                .witnesses
                .entry((state, coalition.clone()))
                .or_default();

            for coalition_actions in coalition_profiles {
                let outsider_profiles = enumerate_assignments(game, state, &outsiders);
                let mut outcomes = StateSet::empty();
                for outsider_actions in outsider_profiles {
                    let mut profile = vec![None; game.agent_count()];
                    for (agent, action) in coalition_actions.iter().chain(&outsider_actions) {
                        profile[agent.index()] = Some(action.clone());
                    }
                    let profile = JointActionProfile::new(
                        profile
                            .into_iter()
                            .map(|action| action.expect("complete profile must cover every agent"))
                            .collect::<Vec<_>>(),
                    );
                    outcomes.insert(game.transition(state, &profile));
                }

                let strategy = CoalitionStrategy::new(coalition_actions);
                derived
                    .effectivity
                    .insert(state, coalition.clone(), outcomes.clone());
                witness_entries.push(StrategyWitness {
                    state,
                    coalition: coalition.clone(),
                    strategy,
                    outcomes,
                });
            }
        }
    }

    derived
}

fn enumerate_assignments<G>(
    game: &G,
    state: StateId,
    agents: &[AgentId],
) -> Vec<Vec<(AgentId, G::Action)>>
where
    G: FiniteGameForm,
{
    fn visit<G>(
        game: &G,
        state: StateId,
        agents: &[AgentId],
        index: usize,
        partial: &mut Vec<(AgentId, G::Action)>,
        result: &mut Vec<Vec<(AgentId, G::Action)>>,
    ) where
        G: FiniteGameForm,
    {
        if index == agents.len() {
            result.push(partial.clone());
            return;
        }
        let agent = agents[index];
        for action in game.actions(state, agent) {
            partial.push((agent, action));
            visit(game, state, agents, index + 1, partial, result);
            partial.pop();
        }
    }

    let mut result = Vec::new();
    visit(game, state, agents, 0, &mut Vec::new(), &mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Action {
        Stay,
        Flip,
    }

    struct ToggleGame;

    impl FiniteGameForm for ToggleGame {
        type Action = Action;

        fn agent_count(&self) -> usize {
            2
        }

        fn state_count(&self) -> usize {
            2
        }

        fn actions(&self, _state: StateId, _agent: AgentId) -> Vec<Self::Action> {
            vec![Action::Stay, Action::Flip]
        }

        fn transition(
            &self,
            state: StateId,
            profile: &JointActionProfile<Self::Action>,
        ) -> StateId {
            let flips = profile
                .iter()
                .filter(|action| **action == Action::Flip)
                .count();
            StateId::new((state.index() + flips) % 2)
        }
    }

    #[test]
    fn derives_outcomes_and_keeps_strategy_witnesses() {
        let derived = derive_effectivity(&ToggleGame);
        let coalition = Coalition::singleton(AgentId::new(0));
        let target = StateSet::singleton(StateId::new(0));

        assert!(!derived.can_enforce(StateId::new(0), &coalition, &target));
        let witnesses = derived
            .witnesses(StateId::new(0), &coalition)
            .expect("strategies for coalition");
        assert_eq!(witnesses.len(), 2);
        assert!(witnesses.iter().any(|witness| {
            witness.strategy.action(AgentId::new(0)) == Some(&Action::Stay)
                && witness.outcomes == StateSet::from_states([StateId::new(0), StateId::new(1)])
        }));
    }

    #[test]
    fn antichain_matches_bruteforce_definition() {
        let game = ToggleGame;
        let derived = derive_effectivity(&game);
        for state in (0..game.state_count()).map(StateId::new) {
            for coalition in Coalition::all(game.agent_count()) {
                for target_mask in 0..(1_usize << game.state_count()) {
                    let target = StateSet::from_states(
                        (0..game.state_count())
                            .filter(|index| target_mask & (1 << index) != 0)
                            .map(StateId::new),
                    );
                    let brute_force =
                        derived
                            .witnesses(state, &coalition)
                            .is_some_and(|witnesses| {
                                witnesses
                                    .iter()
                                    .any(|witness| witness.outcomes.is_subset(&target))
                            });
                    assert_eq!(
                        derived.can_enforce(state, &coalition, &target),
                        brute_force,
                        "state {state:?}, coalition {coalition:?}, target {target:?}"
                    );
                }
            }
        }
    }
}
