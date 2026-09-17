//! Typed IDs and bit-set-backed finite domains.
//!
//! [`crate::domain::Coalition`] and [`crate::domain::StateSet`] deliberately hide their `BitSet`
//! representation so callers cannot accidentally mix agents and states.

use bit_set::BitSet;

/// A dense index identifying an agent in a resolved model.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentId(usize);

/// A dense index identifying a state in a resolved model.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StateId(usize);

/// A dense index identifying an atom in a resolved model.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AtomId(usize);

macro_rules! id_impls {
    ($id:ident) => {
        impl $id {
            /// Construct an identifier from its dense zero-based index.
            #[must_use]
            pub const fn new(index: usize) -> Self {
                Self(index)
            }

            /// Return the dense zero-based index represented by this ID.
            #[must_use]
            pub const fn index(self) -> usize {
                self.0
            }
        }

        impl From<usize> for $id {
            fn from(index: usize) -> Self {
                Self::new(index)
            }
        }
    };
}

id_impls!(AgentId);
id_impls!(StateId);
id_impls!(AtomId);

/// The set of agents making up a coalition.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Coalition {
    agents: BitSet,
}

impl Coalition {
    /// Create the empty coalition.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a coalition from any iterator of agent IDs.
    ///
    /// Duplicate IDs are collapsed, as they are in a mathematical set.
    #[must_use]
    pub fn from_agents<I>(agents: I) -> Self
    where
        I: IntoIterator<Item = AgentId>,
    {
        Self {
            agents: agents.into_iter().map(AgentId::index).collect(),
        }
    }

    /// Create a coalition containing exactly one agent.
    #[must_use]
    pub fn singleton(agent: AgentId) -> Self {
        Self::from_agents([agent])
    }

    /// Insert an agent, returning whether it was not already present.
    pub fn insert(&mut self, agent: AgentId) -> bool {
        self.agents.insert(agent.index())
    }

    /// Remove an agent, returning whether it was present.
    pub fn remove(&mut self, agent: AgentId) -> bool {
        self.agents.remove(agent.index())
    }

    /// Test whether this coalition contains `agent`.
    #[must_use]
    pub fn contains(&self, agent: AgentId) -> bool {
        self.agents.contains(agent.index())
    }

    /// Return the number of agents in the coalition.
    #[must_use]
    pub fn len(&self) -> usize {
        self.agents.iter().count()
    }

    /// Return whether the coalition contains no agents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Test whether every member of this coalition is in `other`.
    #[must_use]
    pub fn is_subset(&self, other: &Self) -> bool {
        self.agents.is_subset(&other.agents)
    }

    /// Test whether this coalition contains every member of `other`.
    #[must_use]
    pub fn is_superset(&self, other: &Self) -> bool {
        other.is_subset(self)
    }

    /// Iterate over members in ascending dense-ID order.
    pub fn iter(&self) -> impl Iterator<Item = AgentId> + '_ {
        self.agents.iter().map(AgentId::new)
    }

    /// Enumerate every coalition for a finite agent universe, including empty
    /// and grand coalitions. The counter is represented by a `BitSet`, so the
    /// implementation does not depend on machine-word shifts.
    #[must_use]
    pub fn all(agent_count: usize) -> CoalitionIter {
        CoalitionIter {
            current: Self::empty(),
            agent_count,
            started: false,
            done: false,
        }
    }
}

impl FromIterator<AgentId> for Coalition {
    fn from_iter<T: IntoIterator<Item = AgentId>>(agents: T) -> Self {
        Self::from_agents(agents)
    }
}

impl Extend<AgentId> for Coalition {
    fn extend<T: IntoIterator<Item = AgentId>>(&mut self, agents: T) {
        agents.into_iter().for_each(|agent| {
            self.insert(agent);
        });
    }
}

impl IntoIterator for Coalition {
    type IntoIter = std::vec::IntoIter<AgentId>;
    type Item = AgentId;

    fn into_iter(self) -> Self::IntoIter {
        self.agents
            .iter()
            .map(AgentId::new)
            .collect::<Vec<_>>()
            .into_iter()
    }
}

/// Iterator over the power set of an agent universe.
#[derive(Clone, Debug)]
pub struct CoalitionIter {
    current: Coalition,
    agent_count: usize,
    started: bool,
    done: bool,
}

impl Iterator for CoalitionIter {
    type Item = Coalition;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        if !self.started {
            self.started = true;
            return Some(self.current.clone());
        }

        // Increment a binary counter stored in the bit set. This is linear in
        // the number of agents per step and avoids shifting a machine integer.
        for index in 0..self.agent_count {
            let agent = AgentId::new(index);
            if self.current.remove(agent) {
                continue;
            }
            self.current.insert(agent);
            return Some(self.current.clone());
        }

        self.done = true;
        None
    }
}

/// A set of states, kept distinct from an agent coalition at the type level.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct StateSet {
    states: BitSet,
}

impl StateSet {
    /// Create an empty state set.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a state set from any iterator of state IDs.
    ///
    /// Duplicate IDs are collapsed, as they are in a mathematical set.
    #[must_use]
    pub fn from_states<I>(states: I) -> Self
    where
        I: IntoIterator<Item = StateId>,
    {
        Self {
            states: states.into_iter().map(StateId::index).collect(),
        }
    }

    /// Create a state set containing exactly one state.
    #[must_use]
    pub fn singleton(state: StateId) -> Self {
        Self::from_states([state])
    }

    /// Insert a state, returning whether it was not already present.
    pub fn insert(&mut self, state: StateId) -> bool {
        self.states.insert(state.index())
    }

    /// Remove a state, returning whether it was present.
    pub fn remove(&mut self, state: StateId) -> bool {
        self.states.remove(state.index())
    }

    /// Test whether this set contains `state`.
    #[must_use]
    pub fn contains(&self, state: StateId) -> bool {
        self.states.contains(state.index())
    }

    /// Return the number of states in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.states.iter().count()
    }

    /// Return whether the set contains no states.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Test whether every state in this set is in `other`.
    #[must_use]
    pub fn is_subset(&self, other: &Self) -> bool {
        self.states.is_subset(&other.states)
    }

    /// Test whether this set contains every state in `other`.
    #[must_use]
    pub fn is_superset(&self, other: &Self) -> bool {
        other.is_subset(self)
    }

    /// Iterate over members in ascending dense-ID order.
    pub fn iter(&self) -> impl Iterator<Item = StateId> + '_ {
        self.states.iter().map(StateId::new)
    }
}

impl FromIterator<StateId> for StateSet {
    fn from_iter<T: IntoIterator<Item = StateId>>(states: T) -> Self {
        Self::from_states(states)
    }
}

impl Extend<StateId> for StateSet {
    fn extend<T: IntoIterator<Item = StateId>>(&mut self, states: T) {
        states.into_iter().for_each(|state| {
            self.insert(state);
        });
    }
}

impl IntoIterator for StateSet {
    type IntoIter = std::vec::IntoIter<StateId>;
    type Item = StateId;

    fn into_iter(self) -> Self::IntoIter {
        self.states
            .iter()
            .map(StateId::new)
            .collect::<Vec<_>>()
            .into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_typed_dense_indices() {
        assert_eq!(AgentId::new(3).index(), 3);
        assert_eq!(StateId::from(3).index(), 3);
        assert_ne!(AgentId::new(3), AgentId::new(4));
    }

    #[test]
    fn coalition_supports_basic_set_operations() {
        let mut coalition = Coalition::singleton(AgentId::new(1));
        assert!(coalition.insert(AgentId::new(4)));
        assert!(!coalition.insert(AgentId::new(4)));
        assert!(coalition.contains(AgentId::new(1)));
        assert_eq!(coalition.len(), 2);
        assert!(Coalition::singleton(AgentId::new(1)).is_subset(&coalition));
        assert!(coalition.remove(AgentId::new(4)));
    }

    #[test]
    fn coalition_iterator_handles_empty_and_large_universes() {
        assert_eq!(Coalition::all(0).count(), 1);
        let coalitions: Vec<_> = Coalition::all(3).collect();
        assert_eq!(coalitions.len(), 8);
        assert!(coalitions.first().is_some_and(Coalition::is_empty));
        assert_eq!(coalitions.last().map(Coalition::len), Some(3));

        // This used to be a common source of overflow/shift bugs.
        assert_eq!(
            Coalition::all(usize::BITS as usize + 1)
                .next()
                .map(|c| c.len()),
            Some(0)
        );
    }

    #[test]
    fn state_set_is_not_a_coalition() {
        let states = StateSet::from_states([StateId::new(0), StateId::new(2)]);
        assert!(states.contains(StateId::new(2)));
        assert_eq!(
            states.iter().collect::<Vec<_>>(),
            vec![StateId::new(0), StateId::new(2)]
        );
    }
}
