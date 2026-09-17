# QCL

QCL is a Rust model checker and inference engine for Quantified Coalition
Logic. It provides a textual model format, an interactive REPL, explicit-state
model checking, and model-relative inference with counterexample states.

## Features

- typed agents, states, propositions, coalitions, and state sets;
- QCL and QCL(>=) formulas with existential and universal modalities;
- an ASCII lexer, Pratt parser, and name resolution;
- explicit effectivity models represented by minimal outcomes;
- validation of referential integrity and weak playability;
- bottom-up model checking with formula and predicate caches;
- coalition predicates and semantic Tseitin CNF generation;
- an interactive terminal with history and command completion.

Global validity, global satisfiability, complete proof search, BDDs, and
action/strategy game structures are not implemented yet.

## Requirements

Building QCL requires:

- Rust with edition 2024 support;
- a working C++ compiler, because the vendored `linenoise-rust` dependency
  builds bundled native sources.

## Quick start

Run the interactive REPL from the repository root:

```bash
cargo run
```

Load and validate the included majority-voting example:

```text
:load examples/majority_voting.qcl
:validate
:check coffee_outcome [size >= 2] coffee
:infer coffee |- !tea
:quit
```

The complete walkthrough is available in [`kb/tutorial.md`](kb/tutorial.md).

## REPL commands

| Command | Purpose |
| --- | --- |
| `:load FILE` | Load a QCL model |
| `:validate` | Validate the loaded model |
| `:check STATE FORMULA` | Check a formula at a state |
| `:states FORMULA` | List states satisfying a formula |
| `:infer PREMISES \|- CONCLUSION` | List counterexample states to an inference |
| `:coalitions PREDICATE` | List coalitions matching a predicate |
| `:tutorial` | Print the built-in tutorial |
| `:help` | Show command help |
| `:quit` | Exit the REPL |

Use `:help` inside the REPL for the authoritative command summary. The model
syntax and language contract are documented in [`kb/dsl.md`](kb/dsl.md).

## Development

Format, lint, test, and build the documentation with:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items
```

The project knowledge base in [`kb/`](kb/) documents the architecture,
implementation status, testing strategy, and language decisions.

## Optional SafeOps showcase

The workspace also contains an unpublished `qcl-safeops` package. It is a
separate local simulator demonstrating QCL-backed tool-call authorization; it
is not included in the published `qcl` package.

```bash
cargo run -p qcl-safeops
```

## License

QCL is distributed under the GNU Affero General Public License, version 3.
See [`LICENSE`](LICENSE) for the full license text.
