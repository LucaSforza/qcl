//! Compiled coalition predicates and their Tseitin CNF representation.
//!
//! [`PredicateProgram`] evaluates the full predicate semantics. [`TseitinCnf`]
//! is compiled for a finite agent universe and shares one membership variable
//! per agent across every primitive occurrence. Consequently, asserting its
//! root is satisfiable exactly for the coalitions satisfying the source AST.

use std::collections::HashMap;

use crate::ast::CoalitionPredicate;
use crate::domain::{AgentId, Coalition};
use thiserror::Error;

/// An index into a [`PredicateProgram`] node array.
pub type PredicateNodeId = usize;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PredicateNodeKey {
    SubsetEq(Coalition),
    SupersetEq(Coalition),
    Geq(usize),
    Not(PredicateNodeId),
    And(PredicateNodeId, PredicateNodeId),
    Or(PredicateNodeId, PredicateNodeId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PredicateNode {
    SubsetEq(Coalition),
    SupersetEq(Coalition),
    Geq(usize),
    Not(PredicateNodeId),
    And(PredicateNodeId, PredicateNodeId),
    Or(PredicateNodeId, PredicateNodeId),
}

/// A structurally interned executable DAG for one coalition predicate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PredicateProgram {
    nodes: Vec<PredicateNode>,
    root: PredicateNodeId,
}

impl PredicateProgram {
    /// Compile a predicate into a structurally interned executable DAG.
    #[must_use]
    pub fn compile(predicate: &CoalitionPredicate) -> Self {
        let mut compiler = Compiler {
            nodes: Vec::new(),
            interned: HashMap::new(),
        };
        let root = compiler.compile(predicate);
        Self {
            nodes: compiler.nodes,
            root,
        }
    }

    /// Evaluate the compiled predicate for `coalition`.
    #[must_use]
    pub fn evaluate(&self, coalition: &Coalition) -> bool {
        let mut values = vec![false; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            values[index] = match node {
                PredicateNode::SubsetEq(required) => coalition.is_subset(required),
                PredicateNode::SupersetEq(required) => required.is_subset(coalition),
                PredicateNode::Geq(minimum) => coalition.len() >= *minimum,
                PredicateNode::Not(child) => !values[*child],
                PredicateNode::And(left, right) => values[*left] && values[*right],
                PredicateNode::Or(left, right) => values[*left] || values[*right],
            };
        }
        values[self.root]
    }

    /// Return the root node ID of this program.
    #[must_use]
    pub const fn root(&self) -> PredicateNodeId {
        self.root
    }

    /// Return the number of unique nodes in the compiled DAG.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

struct Compiler {
    nodes: Vec<PredicateNode>,
    interned: HashMap<PredicateNodeKey, PredicateNodeId>,
}

impl Compiler {
    fn compile(&mut self, predicate: &CoalitionPredicate) -> PredicateNodeId {
        let key = match predicate {
            CoalitionPredicate::SubsetEq(coalition) => {
                PredicateNodeKey::SubsetEq(coalition.clone())
            }
            CoalitionPredicate::SupersetEq(coalition) => {
                PredicateNodeKey::SupersetEq(coalition.clone())
            }
            CoalitionPredicate::Geq(minimum) => PredicateNodeKey::Geq(*minimum),
            CoalitionPredicate::Not(child) => PredicateNodeKey::Not(self.compile(child)),
            CoalitionPredicate::And(left, right) => {
                PredicateNodeKey::And(self.compile(left), self.compile(right))
            }
            CoalitionPredicate::Or(left, right) => {
                PredicateNodeKey::Or(self.compile(left), self.compile(right))
            }
        };

        if let Some(&id) = self.interned.get(&key) {
            return id;
        }
        let node = match &key {
            PredicateNodeKey::SubsetEq(coalition) => PredicateNode::SubsetEq(coalition.clone()),
            PredicateNodeKey::SupersetEq(coalition) => PredicateNode::SupersetEq(coalition.clone()),
            PredicateNodeKey::Geq(minimum) => PredicateNode::Geq(*minimum),
            PredicateNodeKey::Not(child) => PredicateNode::Not(*child),
            PredicateNodeKey::And(left, right) => PredicateNode::And(*left, *right),
            PredicateNodeKey::Or(left, right) => PredicateNode::Or(*left, *right),
        };
        let id = self.nodes.len();
        self.nodes.push(node);
        self.interned.insert(key, id);
        id
    }
}

/// A signed reference to a Tseitin variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Literal {
    variable: usize,
    positive: bool,
}

impl Literal {
    /// Construct a positive occurrence of a variable.
    #[must_use]
    pub const fn positive(variable: usize) -> Self {
        Self {
            variable,
            positive: true,
        }
    }

    /// Construct a negative occurrence of a variable.
    #[must_use]
    pub const fn negative(variable: usize) -> Self {
        Self {
            variable,
            positive: false,
        }
    }

    /// Return this literal's variable index.
    #[must_use]
    pub const fn variable(self) -> usize {
        self.variable
    }

    /// Return whether this literal is positive.
    #[must_use]
    pub const fn is_positive(self) -> bool {
        self.positive
    }

    #[must_use]
    const fn negated(self) -> Self {
        Self {
            variable: self.variable,
            positive: !self.positive,
        }
    }

    /// Evaluate the literal against a Boolean assignment.
    #[must_use]
    pub fn evaluate(self, assignment: &[bool]) -> bool {
        assignment.get(self.variable).copied() == Some(self.positive)
    }
}

/// One disjunction in a Tseitin CNF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause(Vec<Literal>);

impl Clause {
    /// Construct a disjunction from its literals.
    #[must_use]
    pub fn new(literals: Vec<Literal>) -> Self {
        Self(literals)
    }

    /// Borrow the literals in this clause.
    #[must_use]
    pub fn literals(&self) -> &[Literal] {
        &self.0
    }

    /// Return whether at least one literal is true in `assignment`.
    #[must_use]
    pub fn is_satisfied(&self, assignment: &[bool]) -> bool {
        self.0.iter().any(|literal| literal.evaluate(assignment))
    }
}

/// A linear-size CNF with an explicit asserted root variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TseitinCnf {
    clauses: Vec<Clause>,
    variable_count: usize,
    primitive_count: usize,
    root: usize,
    agent_count: usize,
}

/// Failure while compiling a predicate for a finite agent universe.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TseitinError {
    /// A coalition mentions an agent that is not in `0..agent_count`.
    #[error("agent id {agent:?} is outside the finite universe of {agent_count} agents")]
    AgentOutOfRange {
        /// The invalid agent ID.
        agent: AgentId,
        /// Number of agents in the finite universe.
        agent_count: usize,
    },
}

impl TseitinCnf {
    /// Compile a predicate into a root-asserted CNF for `agent_count` agents.
    ///
    /// The first `agent_count` variables are shared membership variables. All
    /// primitive predicates are encoded against those variables, while the
    /// Boolean structure uses ordinary Tseitin variables. The resulting CNF
    /// is therefore equisatisfiable with the source predicate over one finite
    /// coalition, without distributive CNF expansion.
    ///
    /// # Errors
    ///
    /// Returns [`TseitinError::AgentOutOfRange`] when a source coalition
    /// contains an agent outside `0..agent_count`.
    pub fn compile(
        predicate: &CoalitionPredicate,
        agent_count: usize,
    ) -> Result<Self, TseitinError> {
        validate_agents(predicate, agent_count)?;
        let mut compiler = CnfCompiler {
            clauses: Vec::new(),
            variables: agent_count,
            primitive_count: 0,
            interned: HashMap::new(),
            agent_count,
        };
        let root = compiler.compile(predicate);
        compiler
            .clauses
            .push(Clause::new(vec![Literal::positive(root)]));
        Ok(Self {
            clauses: compiler.clauses,
            variable_count: compiler.variables,
            primitive_count: compiler.primitive_count,
            root,
            agent_count,
        })
    }

    /// Borrow the clauses, including the final unit clause asserting `root`.
    #[must_use]
    pub fn clauses(&self) -> &[Clause] {
        &self.clauses
    }

    /// Return the total number of SAT variables.
    #[must_use]
    pub const fn variable_count(&self) -> usize {
        self.variable_count
    }

    /// Return the number of primitive predicate variables.
    #[must_use]
    pub const fn primitive_count(&self) -> usize {
        self.primitive_count
    }

    /// Return the variable asserted by the root unit clause.
    #[must_use]
    pub const fn root(&self) -> usize {
        self.root
    }

    /// Return the number of agents represented by membership variables.
    #[must_use]
    pub const fn agent_count(&self) -> usize {
        self.agent_count
    }

    /// Return the SAT variable representing membership of `agent`.
    #[must_use]
    pub fn membership_variable(&self, agent: AgentId) -> Option<usize> {
        (agent.index() < self.agent_count).then_some(agent.index())
    }

    /// Check satisfiability after fixing all membership variables to `coalition`.
    ///
    /// Membership outside the compilation universe is ignored by the CNF, so
    /// callers should construct coalitions from the same finite universe.
    #[must_use]
    pub fn is_satisfiable_for(&self, coalition: &Coalition) -> bool {
        let mut assignment = vec![None; self.variable_count];
        for (agent, value) in assignment.iter_mut().enumerate().take(self.agent_count) {
            *value = Some(coalition.contains(AgentId::new(agent)));
        }
        dpll(&self.clauses, assignment)
    }

    /// Check whether all clauses are satisfied by a Boolean assignment.
    ///
    /// Assignments may contain trailing values, but must contain at least
    /// [`Self::variable_count`] entries.
    #[must_use]
    pub fn is_satisfied(&self, assignment: &[bool]) -> bool {
        assignment.len() >= self.variable_count
            && self
                .clauses
                .iter()
                .all(|clause| clause.is_satisfied(assignment))
    }

    /// Checks whether the root-asserted encoding has a model.
    /// This helper is intended for tests and examples; production inference
    /// can hand the clauses to a SAT solver instead.
    #[must_use]
    pub fn is_satisfiable(&self) -> bool {
        dpll(&self.clauses, vec![None; self.variable_count])
    }
}

/// Decide the root-asserted CNF using unit propagation and recursive
/// branching. This remains a small helper for tests and examples; callers
/// doing production inference should pass the clauses to a SAT solver.
fn dpll(clauses: &[Clause], mut assignment: Vec<Option<bool>>) -> bool {
    loop {
        let mut changed = false;
        let mut all_satisfied = true;
        for clause in clauses {
            let mut unassigned = 0;
            let mut candidate = None;
            let mut satisfied = false;
            for literal in clause.literals() {
                match assignment[literal.variable()] {
                    Some(value) if value == literal.is_positive() => {
                        satisfied = true;
                        break;
                    }
                    Some(_) => {}
                    None => {
                        unassigned += 1;
                        candidate = Some((literal.variable(), literal.is_positive()));
                    }
                }
            }
            if satisfied {
                continue;
            }
            all_satisfied = false;
            if unassigned == 0 {
                return false;
            }
            if unassigned == 1 {
                let (variable, value) = candidate.expect("one unassigned literal");
                match assignment[variable] {
                    Some(existing) if existing != value => return false,
                    Some(_) => {}
                    None => {
                        assignment[variable] = Some(value);
                        changed = true;
                    }
                }
            }
        }
        if all_satisfied {
            return true;
        }
        if !changed {
            break;
        }
    }

    let Some(variable) = assignment.iter().position(Option::is_none) else {
        return true;
    };
    let mut false_branch = assignment.clone();
    false_branch[variable] = Some(false);
    if dpll(clauses, false_branch) {
        return true;
    }
    assignment[variable] = Some(true);
    dpll(clauses, assignment)
}

fn validate_agents(predicate: &CoalitionPredicate, agent_count: usize) -> Result<(), TseitinError> {
    let coalition = match predicate {
        CoalitionPredicate::SubsetEq(coalition) | CoalitionPredicate::SupersetEq(coalition) => {
            Some(coalition)
        }
        CoalitionPredicate::Geq(_) => None,
        CoalitionPredicate::Not(child) => return validate_agents(child, agent_count),
        CoalitionPredicate::And(left, right) | CoalitionPredicate::Or(left, right) => {
            validate_agents(left, agent_count)?;
            return validate_agents(right, agent_count);
        }
    };
    if let Some(agent) =
        coalition.and_then(|coalition| coalition.iter().find(|agent| agent.index() >= agent_count))
    {
        return Err(TseitinError::AgentOutOfRange { agent, agent_count });
    }
    Ok(())
}

struct CnfCompiler {
    clauses: Vec<Clause>,
    variables: usize,
    primitive_count: usize,
    interned: HashMap<PredicateNodeKey, usize>,
    agent_count: usize,
}

impl CnfCompiler {
    fn fresh(&mut self) -> usize {
        let variable = self.variables;
        self.variables += 1;
        variable
    }

    fn compile(&mut self, predicate: &CoalitionPredicate) -> usize {
        let key = match predicate {
            CoalitionPredicate::SubsetEq(coalition) => {
                PredicateNodeKey::SubsetEq(coalition.clone())
            }
            CoalitionPredicate::SupersetEq(coalition) => {
                PredicateNodeKey::SupersetEq(coalition.clone())
            }
            CoalitionPredicate::Geq(minimum) => PredicateNodeKey::Geq(*minimum),
            CoalitionPredicate::Not(child) => PredicateNodeKey::Not(self.compile(child)),
            CoalitionPredicate::And(left, right) => {
                PredicateNodeKey::And(self.compile(left), self.compile(right))
            }
            CoalitionPredicate::Or(left, right) => {
                PredicateNodeKey::Or(self.compile(left), self.compile(right))
            }
        };

        if let Some(&variable) = self.interned.get(&key) {
            return variable;
        }

        let root = match &key {
            PredicateNodeKey::SubsetEq(coalition) => {
                self.primitive_count += 1;
                let root = self.fresh();
                let literals = (0..self.agent_count)
                    .filter(|agent| !coalition.contains(AgentId::new(*agent)))
                    .map(Literal::negative)
                    .collect::<Vec<_>>();
                self.encode_and_equivalence(root, &literals);
                root
            }
            PredicateNodeKey::SupersetEq(coalition) => {
                self.primitive_count += 1;
                let root = self.fresh();
                let literals = coalition
                    .iter()
                    .map(|agent| Literal::positive(agent.index()));
                let literals = literals.collect::<Vec<_>>();
                self.encode_and_equivalence(root, &literals);
                root
            }
            PredicateNodeKey::Geq(minimum) => {
                self.primitive_count += 1;
                let root = self.fresh();
                self.encode_geq(root, *minimum);
                root
            }
            PredicateNodeKey::Not(child) => {
                let root = self.fresh();
                self.clauses.push(Clause::new(vec![
                    Literal::negative(root),
                    Literal::negative(*child),
                ]));
                self.clauses.push(Clause::new(vec![
                    Literal::positive(root),
                    Literal::positive(*child),
                ]));
                root
            }
            PredicateNodeKey::And(left, right) => {
                let root = self.fresh();
                self.clauses.push(Clause::new(vec![
                    Literal::negative(root),
                    Literal::positive(*left),
                ]));
                self.clauses.push(Clause::new(vec![
                    Literal::negative(root),
                    Literal::positive(*right),
                ]));
                self.clauses.push(Clause::new(vec![
                    Literal::positive(root),
                    Literal::negative(*left),
                    Literal::negative(*right),
                ]));
                root
            }
            PredicateNodeKey::Or(left, right) => {
                let root = self.fresh();
                self.clauses.push(Clause::new(vec![
                    Literal::positive(root),
                    Literal::negative(*left),
                ]));
                self.clauses.push(Clause::new(vec![
                    Literal::positive(root),
                    Literal::negative(*right),
                ]));
                self.clauses.push(Clause::new(vec![
                    Literal::negative(root),
                    Literal::positive(*left),
                    Literal::positive(*right),
                ]));
                root
            }
        };
        self.interned.insert(key, root);
        root
    }

    fn encode_and_equivalence(&mut self, output: usize, literals: &[Literal]) {
        if literals.is_empty() {
            self.clauses
                .push(Clause::new(vec![Literal::positive(output)]));
            return;
        }
        for &literal in literals {
            self.clauses
                .push(Clause::new(vec![Literal::negative(output), literal]));
        }
        self.clauses.push(Clause::new(
            std::iter::once(Literal::positive(output))
                .chain(literals.iter().map(|literal| literal.negated()))
                .collect(),
        ));
    }

    fn encode_or_equivalence(&mut self, output: usize, inputs: &[Literal]) {
        if inputs.is_empty() {
            self.clauses
                .push(Clause::new(vec![Literal::negative(output)]));
            return;
        }
        for &input in inputs {
            self.clauses.push(Clause::new(vec![
                input.negated(),
                Literal::positive(output),
            ]));
        }
        self.clauses.push(Clause::new(
            std::iter::once(Literal::negative(output))
                .chain(inputs.iter().copied())
                .collect(),
        ));
    }

    fn encode_and_gate(&mut self, left: usize, right: usize) -> usize {
        let output = self.fresh();
        self.encode_and_equivalence(output, &[Literal::positive(left), Literal::positive(right)]);
        output
    }

    fn encode_or_gate(&mut self, inputs: &[Literal]) -> usize {
        let output = self.fresh();
        self.encode_or_equivalence(output, inputs);
        output
    }

    fn encode_alias(&mut self, output: usize, input: usize) {
        self.clauses.push(Clause::new(vec![
            Literal::negative(output),
            Literal::positive(input),
        ]));
        self.clauses.push(Clause::new(vec![
            Literal::positive(output),
            Literal::negative(input),
        ]));
    }

    /// Encode `output <-> (at least minimum membership variables)`.
    ///
    /// The rows are the usual sequential threshold counter: row `j` means
    /// that at least `j` inputs seen so far are true. There are O(n*k) rows,
    /// each represented by a constant number of Tseitin clauses.
    fn encode_geq(&mut self, output: usize, minimum: usize) {
        if minimum == 0 {
            self.clauses
                .push(Clause::new(vec![Literal::positive(output)]));
            return;
        }
        if minimum > self.agent_count {
            self.clauses
                .push(Clause::new(vec![Literal::negative(output)]));
            return;
        }

        let mut previous = Vec::new();
        for agent in 0..self.agent_count {
            let membership = agent;
            let width = minimum.min(agent + 1);
            let mut current = Vec::with_capacity(width);

            let first = if let Some(&prior) = previous.first() {
                self.encode_or_gate(&[Literal::positive(membership), Literal::positive(prior)])
            } else {
                membership
            };
            current.push(first);

            for threshold in 2..=width {
                let carry = previous[threshold - 2];
                let term = self.encode_and_gate(membership, carry);
                let mut inputs = vec![Literal::positive(term)];
                if let Some(&prior) = previous.get(threshold - 1) {
                    inputs.push(Literal::positive(prior));
                }
                current.push(self.encode_or_gate(&inputs));
            }
            previous = current;
        }
        self.encode_alias(output, previous[minimum - 1]);
    }
}

impl CoalitionPredicate {
    /// Compile this predicate into an executable DAG.
    #[must_use]
    pub fn compile(&self) -> PredicateProgram {
        PredicateProgram::compile(self)
    }

    /// Compile this predicate into a root-asserted Tseitin CNF for a finite
    /// universe of `agent_count` agents.
    ///
    /// # Errors
    ///
    /// Returns [`TseitinError::AgentOutOfRange`] when a source coalition
    /// contains an agent outside `0..agent_count`.
    pub fn to_tseitin_cnf(&self, agent_count: usize) -> Result<TseitinCnf, TseitinError> {
        TseitinCnf::compile(self, agent_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AgentId;

    fn coalition(agents: &[usize]) -> Coalition {
        agents.iter().copied().map(AgentId::new).collect()
    }

    #[test]
    fn dag_evaluates_predicates_and_interns_repeated_subtrees() {
        let required = coalition(&[0]);
        let atom = CoalitionPredicate::subseteq(required);
        let predicate = CoalitionPredicate::and(atom.clone(), atom);
        let program = predicate.compile();

        assert!(program.evaluate(&coalition(&[])));
        assert!(!program.evaluate(&coalition(&[0, 1])));
        assert_eq!(program.node_count(), 2);
    }

    #[test]
    fn cnf_has_explicit_root_and_linear_growth() {
        let mut predicate = CoalitionPredicate::geq(0);
        for minimum in 1..=12 {
            predicate = CoalitionPredicate::and(predicate, CoalitionPredicate::geq(minimum));
        }
        let cnf = predicate.to_tseitin_cnf(0).expect("valid predicate");
        assert_eq!(cnf.root(), cnf.variable_count() - 1);
        assert!(cnf.variable_count() > 12);
        assert!(cnf.clauses().len() > 36);
    }

    #[test]
    fn cnf_preserves_repeated_primitive_semantics() {
        let primitive = CoalitionPredicate::geq(1);
        let contradiction = CoalitionPredicate::and(primitive.clone(), !primitive.clone());
        let tautology = CoalitionPredicate::or(primitive.clone(), !primitive);

        assert!(
            !contradiction
                .to_tseitin_cnf(1)
                .expect("valid predicate")
                .is_satisfiable()
        );
        assert!(
            tautology
                .to_tseitin_cnf(1)
                .expect("valid predicate")
                .is_satisfiable()
        );
    }

    #[test]
    fn cnf_satisfiability_does_not_assume_large_encodings_are_sat() {
        let primitive = CoalitionPredicate::geq(1);
        let mut contradiction = CoalitionPredicate::and(primitive.clone(), !primitive);
        for minimum in 2..=64 {
            contradiction =
                CoalitionPredicate::and(contradiction, CoalitionPredicate::geq(minimum));
        }

        let cnf = contradiction.to_tseitin_cnf(1).expect("valid predicate");
        assert!(cnf.variable_count() >= usize::BITS as usize);
        assert!(!cnf.is_satisfiable());
    }

    #[test]
    fn cnf_uses_shared_membership_and_preserves_primitive_semantics() {
        let singleton = coalition(&[0]);
        let predicate = CoalitionPredicate::and(
            CoalitionPredicate::supseteq(singleton),
            !CoalitionPredicate::geq(1),
        );
        let cnf = predicate.to_tseitin_cnf(1).expect("valid predicate");
        assert!(!cnf.is_satisfiable());
        assert!(!cnf.is_satisfiable_for(&coalition(&[])));
        assert!(!cnf.is_satisfiable_for(&coalition(&[0])));
        assert_eq!(cnf.membership_variable(AgentId::new(0)), Some(0));
        assert_eq!(cnf.membership_variable(AgentId::new(1)), None);
    }

    #[test]
    fn cnf_handles_empty_grand_and_impossible_cardinality() {
        let empty = CoalitionPredicate::and(
            CoalitionPredicate::subseteq(coalition(&[])),
            CoalitionPredicate::supseteq(coalition(&[])),
        );
        let grand = CoalitionPredicate::and(
            CoalitionPredicate::subseteq(coalition(&[0, 1])),
            CoalitionPredicate::supseteq(coalition(&[0, 1])),
        );
        assert!(
            empty
                .to_tseitin_cnf(2)
                .expect("valid")
                .is_satisfiable_for(&coalition(&[]))
        );
        assert!(
            !empty
                .to_tseitin_cnf(2)
                .expect("valid")
                .is_satisfiable_for(&coalition(&[0]))
        );
        assert!(
            grand
                .to_tseitin_cnf(2)
                .expect("valid")
                .is_satisfiable_for(&coalition(&[0, 1]))
        );
        assert!(
            !grand
                .to_tseitin_cnf(2)
                .expect("valid")
                .is_satisfiable_for(&coalition(&[0]))
        );
        assert!(
            !CoalitionPredicate::geq(3)
                .to_tseitin_cnf(2)
                .expect("valid")
                .is_satisfiable()
        );
    }

    #[test]
    fn cnf_rejects_agent_ids_outside_the_universe() {
        let predicate = CoalitionPredicate::supseteq(coalition(&[2]));
        assert_eq!(
            predicate.to_tseitin_cnf(2),
            Err(TseitinError::AgentOutOfRange {
                agent: AgentId::new(2),
                agent_count: 2,
            })
        );
    }

    #[test]
    fn cnf_matches_dag_for_every_small_coalition() {
        let universes = Coalition::all(2).collect::<Vec<_>>();
        let mut predicates = Vec::new();
        for required in &universes {
            predicates.push(CoalitionPredicate::subseteq(required.clone()));
            predicates.push(CoalitionPredicate::supseteq(required.clone()));
        }
        for minimum in 0..=3 {
            predicates.push(CoalitionPredicate::geq(minimum));
        }
        predicates.push(CoalitionPredicate::and(
            CoalitionPredicate::supseteq(coalition(&[0])),
            !CoalitionPredicate::geq(2),
        ));

        for predicate in predicates {
            let cnf = predicate.to_tseitin_cnf(2).expect("valid predicate");
            let program = predicate.compile();
            for coalition in &universes {
                assert_eq!(
                    cnf.is_satisfiable_for(coalition),
                    program.evaluate(coalition),
                    "predicate {predicate:?}, coalition {coalition:?}"
                );
            }
        }
    }

    #[test]
    fn cardinality_encoding_has_polynomial_size() {
        let cnf = CoalitionPredicate::geq(6)
            .to_tseitin_cnf(12)
            .expect("valid predicate");
        assert!(cnf.variable_count() < 12 * 6 * 4);
        assert!(cnf.clauses().len() < 12 * 6 * 12);
    }
}
