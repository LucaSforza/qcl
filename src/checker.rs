//! Explicit-state model checking for quantified coalition logic.

use std::cell::RefCell;
use std::collections::HashMap;

use thiserror::Error;

use crate::ast::{CoalitionPredicate, Formula};
use crate::domain::{AgentId, AtomId, Coalition, StateId, StateSet};
use crate::model::QclModel;
use crate::predicate::PredicateProgram;

/// Errors raised while checking a formula against a model.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ModelCheckerError {
    /// A requested state is outside the model's state table.
    #[error("state id {state:?} is out of range")]
    StateOutOfRange {
        /// The invalid state ID.
        state: StateId,
    },
    /// A formula refers to an atom outside the model's atom table.
    #[error("atom id {atom:?} is out of range")]
    AtomOutOfRange {
        /// The invalid atom ID.
        atom: AtomId,
    },
    /// A predicate refers to an agent outside the model's agent table.
    #[error("agent id {agent:?} is out of range")]
    AgentOutOfRange {
        /// The invalid agent ID.
        agent: AgentId,
    },
}

/// Backwards-friendly short name for [`ModelCheckerError`].
pub type CheckerError = ModelCheckerError;

/// Computes the states satisfying QCL formulas in an explicit finite model.
///
/// Formula and predicate results are memoized.  Interior mutability keeps the
/// read-only checking API ergonomic while retaining the cache between calls.
pub struct ModelChecker<'model> {
    model: &'model QclModel,
    formula_cache: RefCell<HashMap<Formula, StateSet>>,
    predicate_cache: RefCell<HashMap<CoalitionPredicate, PredicateProgram>>,
}

impl<'model> ModelChecker<'model> {
    /// Create a checker borrowing `model`.
    #[must_use]
    pub fn new(model: &'model QclModel) -> Self {
        Self {
            model,
            formula_cache: RefCell::new(HashMap::new()),
            predicate_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Return the model used by this checker.
    #[must_use]
    pub const fn model(&self) -> &'model QclModel {
        self.model
    }

    /// Return all states satisfying `formula`.
    ///
    /// # Errors
    ///
    /// Returns a typed error if the formula contains an ID not present in the
    /// model.
    pub fn satisfying_states(&self, formula: &Formula) -> Result<StateSet, ModelCheckerError> {
        self.validate_formula(formula)?;
        self.evaluate(formula)
    }

    /// Check one state against `formula`.
    ///
    /// # Errors
    ///
    /// Returns a typed error if `state` or an ID in the formula is out of
    /// range.
    pub fn check(&self, state: StateId, formula: &Formula) -> Result<bool, ModelCheckerError> {
        self.ensure_state(state)?;
        Ok(self.satisfying_states(formula)?.contains(state))
    }

    fn universe(&self) -> StateSet {
        (0..self.model.state_count())
            .map(StateId::new)
            .collect::<StateSet>()
    }

    fn evaluate(&self, formula: &Formula) -> Result<StateSet, ModelCheckerError> {
        if let Some(states) = self.formula_cache.borrow().get(formula) {
            return Ok(states.clone());
        }

        let states = match formula {
            Formula::True => self.universe(),
            Formula::False => StateSet::empty(),
            Formula::Atom(atom) => (0..self.model.state_count())
                .map(StateId::new)
                .filter(|state| self.model.is_true(*state, *atom))
                .collect(),
            Formula::Not(child) => Self::difference(&self.universe(), &self.evaluate(child)?),
            Formula::And(left, right) => {
                Self::intersection(&self.evaluate(left)?, &self.evaluate(right)?)
            }
            Formula::Or(left, right) => Self::union(&self.evaluate(left)?, &self.evaluate(right)?),
            Formula::Implies(left, right) => Self::union(
                &Self::difference(&self.universe(), &self.evaluate(left)?),
                &self.evaluate(right)?,
            ),
            Formula::Exists { predicate, formula } => {
                self.ability_states(predicate, formula, false)?
            }
            Formula::Forall { predicate, formula } => {
                self.ability_states(predicate, formula, true)?
            }
        };

        self.formula_cache
            .borrow_mut()
            .insert(formula.clone(), states.clone());
        Ok(states)
    }

    fn ability_states(
        &self,
        predicate: &CoalitionPredicate,
        formula: &Formula,
        universal: bool,
    ) -> Result<StateSet, ModelCheckerError> {
        let target = self.evaluate(formula)?;
        let program = self.predicate(predicate);
        let mut result = StateSet::empty();

        for state in (0..self.model.state_count()).map(StateId::new) {
            let mut matched = false;
            let mut holds = universal;
            for coalition in Coalition::all(self.model.agent_count()) {
                if !program.evaluate(&coalition) {
                    continue;
                }
                matched = true;
                let can_enforce = self
                    .model
                    .effectivity
                    .can_enforce(state, &coalition, &target);
                if universal {
                    if !can_enforce {
                        holds = false;
                        break;
                    }
                } else if can_enforce {
                    holds = true;
                    break;
                }
            }
            // Universal ability is vacuously true when no coalition matches;
            // existential ability is false in that case.
            if holds && (universal || matched) {
                result.insert(state);
            }
        }
        Ok(result)
    }

    fn predicate(&self, predicate: &CoalitionPredicate) -> PredicateProgram {
        if let Some(program) = self.predicate_cache.borrow().get(predicate) {
            return program.clone();
        }
        let program = PredicateProgram::compile(predicate);
        self.predicate_cache
            .borrow_mut()
            .insert(predicate.clone(), program.clone());
        program
    }

    fn validate_formula(&self, formula: &Formula) -> Result<(), ModelCheckerError> {
        match formula {
            Formula::Atom(atom) => {
                if atom.index() >= self.model.atom_count() {
                    return Err(ModelCheckerError::AtomOutOfRange { atom: *atom });
                }
            }
            Formula::Not(child) => self.validate_formula(child)?,
            Formula::And(left, right)
            | Formula::Or(left, right)
            | Formula::Implies(left, right) => {
                self.validate_formula(left)?;
                self.validate_formula(right)?;
            }
            Formula::Exists { predicate, formula } | Formula::Forall { predicate, formula } => {
                self.validate_predicate(predicate)?;
                self.validate_formula(formula)?;
            }
            Formula::True | Formula::False => {}
        }
        Ok(())
    }

    fn validate_predicate(&self, predicate: &CoalitionPredicate) -> Result<(), ModelCheckerError> {
        match predicate {
            CoalitionPredicate::SubsetEq(coalition) | CoalitionPredicate::SupersetEq(coalition) => {
                if let Some(agent) = coalition
                    .iter()
                    .find(|agent| agent.index() >= self.model.agent_count())
                {
                    return Err(ModelCheckerError::AgentOutOfRange { agent });
                }
            }
            CoalitionPredicate::Not(child) => self.validate_predicate(child)?,
            CoalitionPredicate::And(left, right) | CoalitionPredicate::Or(left, right) => {
                self.validate_predicate(left)?;
                self.validate_predicate(right)?;
            }
            CoalitionPredicate::Geq(_) => {}
        }
        Ok(())
    }

    fn ensure_state(&self, state: StateId) -> Result<(), ModelCheckerError> {
        if state.index() >= self.model.state_count() {
            Err(ModelCheckerError::StateOutOfRange { state })
        } else {
            Ok(())
        }
    }

    fn intersection(left: &StateSet, right: &StateSet) -> StateSet {
        left.iter().filter(|state| right.contains(*state)).collect()
    }

    fn union(left: &StateSet, right: &StateSet) -> StateSet {
        left.iter().chain(right.iter()).collect()
    }

    fn difference(left: &StateSet, right: &StateSet) -> StateSet {
        left.iter()
            .filter(|state| !right.contains(*state))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::domain::AtomId;
    use crate::model::Effectivity;
    use crate::symbols::SymbolTable;

    fn model() -> QclModel {
        let mut agents = SymbolTable::new();
        agents.insert("a").expect("agent");
        agents.insert("b").expect("agent");
        let mut states = SymbolTable::new();
        states.insert("s0").expect("state");
        states.insert("s1").expect("state");
        let mut atoms = SymbolTable::new();
        atoms.insert("p").expect("atom");
        let mut valuation = HashMap::new();
        valuation.insert(StateId::new(0), HashSet::from([AtomId::new(0)]));
        QclModel::new(agents, states, atoms, valuation, Effectivity::new())
    }

    #[test]
    fn evaluates_propositional_formulas_bottom_up() {
        let model = model();
        let checker = ModelChecker::new(&model);
        let formula = Formula::implies(Formula::atom(AtomId::new(0)), Formula::False);
        assert_eq!(
            checker.satisfying_states(&formula).expect("valid formula"),
            StateSet::singleton(StateId::new(1))
        );
    }

    #[test]
    fn modalities_are_direct_and_have_correct_vacuity() {
        let mut model = model();
        let a = Coalition::singleton(AgentId::new(0));
        let b = Coalition::singleton(AgentId::new(1));
        model
            .effectivity
            .insert(StateId::new(0), a, StateSet::singleton(StateId::new(0)));
        model
            .effectivity
            .insert(StateId::new(0), b, StateSet::singleton(StateId::new(1)));
        let checker = ModelChecker::new(&model);
        let atom = Formula::atom(AtomId::new(0));
        assert!(
            checker
                .check(
                    StateId::new(0),
                    &Formula::exists(CoalitionPredicate::geq(1), atom.clone())
                )
                .expect("valid formula")
        );
        assert!(
            !checker
                .check(
                    StateId::new(0),
                    &Formula::forall(CoalitionPredicate::geq(1), atom)
                )
                .expect("valid formula")
        );
        assert!(
            !checker
                .check(
                    StateId::new(0),
                    &Formula::exists(CoalitionPredicate::geq(3), Formula::True)
                )
                .expect("valid formula")
        );
        assert!(
            checker
                .check(
                    StateId::new(0),
                    &Formula::forall(CoalitionPredicate::geq(3), Formula::False)
                )
                .expect("valid formula")
        );
    }

    #[test]
    fn reports_invalid_formula_ids() {
        let model = model();
        let checker = ModelChecker::new(&model);
        assert_eq!(
            checker.satisfying_states(&Formula::atom(AtomId::new(4))),
            Err(ModelCheckerError::AtomOutOfRange {
                atom: AtomId::new(4)
            })
        );
        let invalid = Formula::exists(
            CoalitionPredicate::subset_eq(Coalition::singleton(AgentId::new(4))),
            Formula::True,
        );
        assert_eq!(
            checker.satisfying_states(&invalid),
            Err(ModelCheckerError::AgentOutOfRange {
                agent: AgentId::new(4)
            })
        );
        assert_eq!(
            checker.check(StateId::new(4), &Formula::True),
            Err(ModelCheckerError::StateOutOfRange {
                state: StateId::new(4)
            })
        );
    }
}
