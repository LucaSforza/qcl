//! Source-level abstract syntax for quantified coalition logic.

use crate::domain::{AtomId, Coalition};
use std::ops::Not;

/// A predicate over the coalition currently being tested.
///
/// Modalities keep predicates as syntax here.  Name resolution and compilation
/// happen in later layers, so this type remains a faithful representation of
/// the source expression.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CoalitionPredicate {
    /// The tested coalition is contained in `coalition`.
    SubsetEq(Coalition),
    /// The tested coalition contains `coalition`.
    SupersetEq(Coalition),
    /// The tested coalition has at least `minimum` members.
    Geq(usize),
    /// The logical negation of a coalition predicate.
    Not(Box<Self>),
    /// The conjunction of two coalition predicates.
    And(Box<Self>, Box<Self>),
    /// The disjunction of two coalition predicates.
    Or(Box<Self>, Box<Self>),
}

impl CoalitionPredicate {
    /// Construct a predicate requiring the tested coalition to be a subset of
    /// `coalition`.
    #[must_use]
    pub fn subseteq(coalition: Coalition) -> Self {
        Self::SubsetEq(coalition)
    }

    /// Spelling variant of [`Self::subseteq`].
    #[must_use]
    pub fn subset_eq(coalition: Coalition) -> Self {
        Self::subseteq(coalition)
    }

    /// Construct a predicate requiring the tested coalition to contain
    /// `coalition`.
    #[must_use]
    pub fn supseteq(coalition: Coalition) -> Self {
        Self::SupersetEq(coalition)
    }

    /// Spelling variant of [`Self::supseteq`].
    #[must_use]
    pub fn superset_eq(coalition: Coalition) -> Self {
        Self::supseteq(coalition)
    }

    /// Construct a cardinality predicate requiring at least `minimum` agents.
    #[must_use]
    pub const fn geq(minimum: usize) -> Self {
        Self::Geq(minimum)
    }

    /// Construct the negation of `predicate`.
    #[must_use]
    pub fn negate(predicate: Self) -> Self {
        Self::Not(Box::new(predicate))
    }

    /// Construct the conjunction of two predicates.
    #[must_use]
    pub fn and(left: Self, right: Self) -> Self {
        Self::And(Box::new(left), Box::new(right))
    }

    /// Construct the disjunction of two predicates.
    #[must_use]
    pub fn or(left: Self, right: Self) -> Self {
        Self::Or(Box::new(left), Box::new(right))
    }
}

/// A QCL formula.  Universal ability is primitive and is deliberately not
/// represented as the negation of existential ability.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Formula {
    /// A formula that is true at every state.
    True,
    /// A formula that is false at every state.
    False,
    /// A proposition identified in a resolved model.
    Atom(AtomId),
    /// The logical negation of a formula.
    Not(Box<Self>),
    /// The conjunction of two formulas.
    And(Box<Self>, Box<Self>),
    /// The disjunction of two formulas.
    Or(Box<Self>, Box<Self>),
    /// Material implication from the left formula to the right formula.
    Implies(Box<Self>, Box<Self>),
    /// Existential ability: some coalition satisfying `predicate` can enforce
    /// `formula`.
    Exists {
        /// Predicate selecting the coalitions quantified by this modality.
        predicate: CoalitionPredicate,
        /// Formula to be enforced by a selected coalition.
        formula: Box<Self>,
    },
    /// Universal ability: every coalition satisfying `predicate` can enforce
    /// `formula`. This is a primitive modality, not a dual rewrite.
    Forall {
        /// Predicate selecting the coalitions quantified by this modality.
        predicate: CoalitionPredicate,
        /// Formula to be enforced by every selected coalition.
        formula: Box<Self>,
    },
}

impl Formula {
    /// Construct a proposition atom.
    #[must_use]
    pub fn atom(atom: AtomId) -> Self {
        Self::Atom(atom)
    }

    /// Construct the negation of `formula`.
    #[must_use]
    pub fn negate(formula: Self) -> Self {
        Self::Not(Box::new(formula))
    }

    /// Construct the conjunction of two formulas.
    #[must_use]
    pub fn and(left: Self, right: Self) -> Self {
        Self::And(Box::new(left), Box::new(right))
    }

    /// Construct the disjunction of two formulas.
    #[must_use]
    pub fn or(left: Self, right: Self) -> Self {
        Self::Or(Box::new(left), Box::new(right))
    }

    /// Construct a material implication from `left` to `right`.
    #[must_use]
    pub fn implies(left: Self, right: Self) -> Self {
        Self::Implies(Box::new(left), Box::new(right))
    }

    /// Construct an existential ability modality.
    #[must_use]
    pub fn exists(predicate: CoalitionPredicate, formula: Self) -> Self {
        Self::Exists {
            predicate,
            formula: Box::new(formula),
        }
    }

    /// Construct a universal ability modality.
    #[must_use]
    pub fn forall(predicate: CoalitionPredicate, formula: Self) -> Self {
        Self::Forall {
            predicate,
            formula: Box::new(formula),
        }
    }
}

impl Not for CoalitionPredicate {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self::negate(self)
    }
}

impl Not for Formula {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self::negate(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AgentId;

    #[test]
    fn modalities_are_preserved_as_distinct_source_nodes() {
        let predicate = CoalitionPredicate::geq(1);
        let atom = Formula::atom(AtomId::new(0));

        assert_eq!(
            Formula::exists(predicate.clone(), atom.clone()),
            Formula::Exists {
                predicate: predicate.clone(),
                formula: Box::new(atom.clone()),
            }
        );
        assert_ne!(
            Formula::exists(predicate.clone(), atom.clone()),
            Formula::forall(predicate, atom)
        );
    }

    #[test]
    fn predicate_constructors_build_composable_ast() {
        let singleton = Coalition::singleton(AgentId::new(0));
        let predicate = CoalitionPredicate::and(
            CoalitionPredicate::subset_eq(singleton.clone()),
            !CoalitionPredicate::superset_eq(singleton),
        );

        assert!(matches!(predicate, CoalitionPredicate::And(_, _)));
    }
}
