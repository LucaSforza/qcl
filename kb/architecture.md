# Architecture

## Objective

Provide a small command-line tool and reusable Rust library for finite QCL models. MVP performs explicit-state model checking and model-relative inference. Global satisfiability and proof search remain outside scope.

## Semantic layers

1. Syntax preserves formulas, coalition predicates, source spans, and unresolved names.
2. Resolution interns names into dense domain IDs and builds coalitions from `BitSet`.
3. Predicate compilation produces:
   - shared executable DAG for repeated evaluation against concrete coalitions;
   - Tseitin CNF for SAT-oriented reasoning without exponential distributive expansion.
     CNF compilation receives the finite agent count, shares one membership
     variable per agent across primitive predicates, and returns a structured
     error when a source coalition names an agent outside that universe.
4. Model validation checks referential integrity and weak-playability constraints.
5. Model checking computes satisfying state sets bottom-up and caches subformula results.
6. Model-relative inference reduces premises and conclusion to state sets and reports counterexamples.
7. `repl` owns model context, command parsing, command completion candidates,
   and history path selection. `main` owns linenoise lifecycle, native history
   load/add/save, terminal input, and output.

## Core invariants

- Agent, state, and atom IDs are dense and stable within one resolved model.
- `Coalition` contains only agent indices below model agent count.
- `StateSet` contains only state indices below model state count.
- Effectivity entries contain minimal enforceable outcome sets. Superset closure is semantic, not materialized.
- A coalition enforces target `T` iff some stored minimal outcome `X` satisfies `X subseteq T`.
- Universal coalition modality is evaluated directly, never through modal duality.
- Predicate CNF uses shared membership variables plus Tseitin variables for
  Boolean structure and cardinality circuits. With the root asserted, it is
  equisatisfiable with the source AST over one finite coalition; source AST
  remains semantic authority.

## Explicit model cost

Coalitions may be exponential in agent count. MVP accepts this because explicit effectivity input already has exponential worst-case size. Coalition enumeration must avoid shifting a machine integer by agent count; use a dedicated iterator over `BitSet`.

## Effectivity representation

Effectivity is indexed by state and coalition. Each value is an antichain of minimal state sets. Inserting an outcome removes dominated supersets and ignores outcomes dominated by an existing subset.

Outcome monotonicity follows by construction. Remaining weak-playability properties require explicit validation.

## Intended module boundaries

- `ast`: source-independent syntax types.
- `parser`: lexer, Pratt parser, spans, unresolved model syntax.
- `symbols`: typed interning and resolution.
- `predicate`: DAG evaluator and Tseitin CNF.
- `model`: resolved model, effectivity antichains, validation.
- `checker`: bottom-up QCL labeling.
- `inference`: model-relative consequence.
- `repl`: linenoise adapter and commands.

## Companion packages

The repository may contain independent workspace packages that consume `qcl`,
but the published `qcl` package remains the domain-neutral model checker and
REPL. Companion applications must not add LLM, tool-execution, approval, or
credential concepts to the core crate.

`safeops-k8s` is an optional, unpublished showcase package. It translates typed
tool intents into QCL coalition checks and separately verifies that every
declared tool outcome satisfies a safety invariant before a Kubernetes adapter
can execute a snapshot-bound grant. It owns the adapter boundary, tool
contracts, grants, audit records, local scenario, and Kubernetes fixtures. The
core crate owns parsing, validation, model checking, and inference only.

The workspace and release boundary are distinct: a source checkout may contain
both packages, while `cargo install qcl` and the packaged `qcl` crate must not
contain or build the showcase. See the [SafeOps Kubernetes contract](../showcases/safeops-k8s/kb/safeops.md)
for the complete contract.

`ai-containment` is a separate, unpublished offline showcase for an executable
finite concurrent game form. Its `ContainmentSystem::transition` is the sole
operational semantics; a domain-neutral `game_form` module enumerates coalition
action choices and outsider completions to derive QCL effectivity antichains.
The showcase owns containment agents, actions, states, valuations, CLI rendering,
and strategy explanations. Core owns only typed finite game-form mechanics and
contains no containment, LLM, credential, or cloud concepts.

## Runtime and native boundary

`Repl` and its command dispatcher contain no terminal dependency and remain
fully testable. Binary adapter registers `command_completions` with linenoise,
loads persistent history, reads lines, adds non-empty lines to history, and
saves history during shutdown.

Project patches `linenoise-rust 0.2.1` to a vendored copy. Upstream binding
constructed raw pointers from temporary `CString` values in history and
completion functions. Vendored binding keeps each `CString` alive through its
FFI call. Native linenoise normally disables editing for `TERM=dumb`; vendored
copy permits raw editing whenever stdin is an actual TTY, while retaining
stream fallback for non-TTY input.

## Implemented boundary

See [`current-implementation.md`](current-implementation.md) for current
capabilities, operational behavior, and explicit non-goals.
