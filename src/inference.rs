//! Model-relative consequence for quantified coalition logic.

use thiserror::Error;

use crate::ast::Formula;
use crate::checker::{ModelChecker, ModelCheckerError};
use crate::domain::{StateId, StateSet};
use crate::model::QclModel;

/// Errors raised while computing model-relative consequence.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum InferenceError {
    /// The checker rejected an ID in a premise or conclusion.
    #[error(transparent)]
    Checking(#[from] ModelCheckerError),
}

/// Computes model-relative entailment and returns its counterexample states.
pub struct InferenceEngine<'model> {
    checker: ModelChecker<'model>,
}

impl<'model> InferenceEngine<'model> {
    /// Create an inference engine borrowing `model`.
    #[must_use]
    pub fn new(model: &'model QclModel) -> Self {
        Self {
            checker: ModelChecker::new(model),
        }
    }

    /// Return states satisfying every premise but not the conclusion.
    ///
    /// An empty result means `premises |=_M conclusion`.  With no premises,
    /// every state is considered a premise counterexample candidate.
    ///
    /// # Errors
    ///
    /// Returns a typed error if an ID in a premise or conclusion is outside
    /// the model's domain.
    pub fn infer(
        &self,
        premises: &[Formula],
        conclusion: &Formula,
    ) -> Result<StateSet, InferenceError> {
        let mut candidates = self.universe();
        for premise in premises {
            candidates = Self::intersection(&candidates, &self.checker.satisfying_states(premise)?);
        }
        let conclusion_states = self.checker.satisfying_states(conclusion)?;
        Ok(Self::difference(&candidates, &conclusion_states))
    }

    /// Alias emphasizing that the result is a set of counterexamples.
    ///
    /// # Errors
    ///
    /// Returns a typed error if an ID in a premise or conclusion is outside
    /// the model's domain.
    pub fn counterexamples(
        &self,
        premises: &[Formula],
        conclusion: &Formula,
    ) -> Result<StateSet, InferenceError> {
        self.infer(premises, conclusion)
    }

    /// Whether the conclusion follows from all premises in this model.
    ///
    /// # Errors
    ///
    /// Returns a typed error if an ID in a premise or conclusion is outside
    /// the model's domain.
    pub fn entails(
        &self,
        premises: &[Formula],
        conclusion: &Formula,
    ) -> Result<bool, InferenceError> {
        Ok(self.infer(premises, conclusion)?.is_empty())
    }

    fn universe(&self) -> StateSet {
        (0..self.checker.model().state_count())
            .map(StateId::new)
            .collect()
    }

    fn intersection(left: &StateSet, right: &StateSet) -> StateSet {
        left.iter().filter(|state| right.contains(*state)).collect()
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
    use crate::symbols::SymbolTable;

    fn model() -> QclModel {
        let mut states = SymbolTable::new();
        states.insert("s0").expect("state");
        states.insert("s1").expect("state");
        let mut atoms = SymbolTable::new();
        atoms.insert("p").expect("atom");
        let mut valuation = HashMap::new();
        valuation.insert(StateId::new(0), HashSet::from([AtomId::new(0)]));
        QclModel::new(
            SymbolTable::new(),
            states,
            atoms,
            valuation,
            crate::model::Effectivity::default(),
        )
    }

    #[test]
    fn returns_counterexample_states_for_invalid_consequence() {
        let model = model();
        let engine = InferenceEngine::new(&model);
        let p = Formula::atom(AtomId::new(0));
        assert_eq!(
            engine.infer(&[], &p).expect("valid formulas"),
            StateSet::singleton(StateId::new(1))
        );
        assert!(!engine.entails(&[], &p).expect("valid formulas"));
    }

    #[test]
    fn valid_consequence_has_no_counterexamples() {
        let model = model();
        let engine = InferenceEngine::new(&model);
        let p = Formula::atom(AtomId::new(0));
        assert!(
            engine
                .entails(std::slice::from_ref(&p), &p)
                .expect("valid formulas")
        );
        assert!(
            engine
                .counterexamples(&[p], &Formula::True)
                .expect("valid formulas")
                .is_empty()
        );
    }

    #[test]
    fn propagates_checker_errors() {
        let model = model();
        let engine = InferenceEngine::new(&model);
        let error = engine
            .infer(&[], &Formula::atom(AtomId::new(9)))
            .expect_err("invalid atom");
        assert_eq!(
            error,
            InferenceError::Checking(ModelCheckerError::AtomOutOfRange {
                atom: AtomId::new(9)
            })
        );
    }
}
