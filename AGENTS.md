# QCL Agent Instructions

## Goal

Build a small, correct Quantified Coalition Logic model checker and inference engine in Rust.

## Required design

- Represent coalitions with `bit_set::BitSet`, hidden behind a domain-specific newtype.
- Preserve source predicates as an AST.
- Compile predicates to a shared internal DAG for model checking.
- Compile predicates to Tseitin CNF for SAT-oriented inference without distributive CNF expansion.
- Keep parsing, name resolution, model validation, model checking, inference, and REPL separate.
- Keep public APIs narrow and domain typed (`AgentId`, `StateId`, `AtomId`).
- Treat `[P] phi` as a primitive modality. It is not the dual of `<P> phi`.
- Store explicit effectivity functions by their minimal outcome sets; upward closure is implicit.

## Development process

- Use test-driven development: add a failing test, implement minimum behavior, then refactor.
- Run focused tests after each red-green-refactor cycle and full test suite before task completion.
- Every delegated task must end with its own focused git commit.
- Use Conventional Commits messages.
- Never combine unrelated changes in one commit.
- Do not rewrite or discard another agent's work.
- Update `kb/` when implementation changes an architectural decision or DSL contract.

## Initial scope

- QCL and cardinality predicate `geq(n)`.
- Explicit finite weak-playability models.
- ASCII DSL with source spans and useful errors.
- Model checking and model-relative inference with counterexample states.
- Linenoise REPL with load, validate, check, states, infer, coalitions, help, and quit commands.

## Non-goals for MVP

- Global QCL satisfiability or validity.
- Complete proof search from QCL axioms.
- Reactive Modules Language.
- BDD-based symbolic model checking.
- Action/strategy game structures.

## Quality gates

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`

