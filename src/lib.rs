//! Quantified Coalition Logic (QCL) model checker and inference library.
//!
//! QCL evaluates formulas over a finite, explicit model.  The source-facing
//! [`parser`] module parses the ASCII DSL and resolves names into the typed
//! identifiers used by the semantic layers.  [`checker::ModelChecker`] then
//! computes the set of states satisfying a [`ast::Formula`], while
//! [`inference::InferenceEngine`] reports model-relative counterexamples.
//!
//! The library is intentionally layered:
//!
//! * [`ast`] contains the source-independent formula and coalition-predicate
//!   trees.
//! * [`domain`] contains typed IDs and the [`domain::Coalition`] and
//!   [`domain::StateSet`] bit-set wrappers.
//! * [`symbols`] interns names; [`parser`] keeps source spans and performs
//!   resolution.
//! * [`predicate`] provides a shared predicate DAG and a Tseitin CNF encoding.
//! * [`model`] stores explicit effectivity antichains and validates
//!   weak-playability.
//! * [`checker`] and [`inference`] implement model checking and consequence.
//!
//! The square-bracket modality is primitive: `[P] φ` is evaluated directly
//! for every coalition satisfying `P`; it is not rewritten as `!<P>!φ`.
//!
//! # Quick start
//!
//! Parse a model and formula, resolve the formula against the model's symbol
//! tables, and ask whether a state satisfies it:
//!
//! ```
//! use qcl::{checker::ModelChecker, parser::parse_formula, parser::parse_model};
//!
//! let model = parse_model(r#"
//!     model {
//!         agents { alice };
//!         states { s0 };
//!         props { ready };
//!         valuation { s0: { ready }; };
//!         effectivity { };
//!     }
//! "#).expect("valid model");
//! let source = parse_formula("ready").expect("valid formula");
//! let formula = source
//!     .resolve(&model.agents, &model.atoms)
//!     .expect("known proposition");
//! let state = model.states.lookup("s0").expect("known state");
//! assert!(ModelChecker::new(&model).check(state, &formula).expect("valid IDs"));
//! ```
//!
//! When using the companion binary, enter `:tutorial` in the REPL for a
//! guided tour of the same model-loading and checking workflow.

pub mod ast;
pub mod checker;
/// Typed identifiers and finite set wrappers used by resolved models.
pub mod domain;
pub mod inference;
/// Explicit effectivity models and weak-playability validation.
pub mod model;
pub mod parser;
pub mod predicate;
pub mod repl;
/// Insertion-ordered name tables with typed IDs.
pub mod symbols;
