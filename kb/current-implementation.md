# Current implementation

## Delivered capabilities

The `qcl` crate currently provides:

- typed dense IDs for agents, states, and propositions;
- coalitions and state sets backed by `bit_set::BitSet`;
- QCL and QCL(>=) syntax trees with primitive existential and universal modalities;
- an ASCII lexer and Pratt parser with half-open source spans;
- name resolution into typed IDs;
- explicit effectivity models stored as antichains of minimal outcomes;
- validation of referential integrity and weak-playability conditions;
- bottom-up explicit-state model checking with formula and predicate caches;
- model-relative inference that returns counterexample states;
- executable predicate DAGs;
- semantic Tseitin CNF over a finite agent universe, including polynomial cardinality circuits;
- a testable command dispatcher and an interactive linenoise front end;
- generated Rust API documentation and executable REPL tutorial.

Global QCL validity, global satisfiability, complete proof search, Reactive
Modules Language, BDDs, and action/strategy game structures are not
implemented.

## Interactive terminal

The binary uses the vendored `linenoise-rust 0.2.1` binding. The vendored copy
contains two required compatibility fixes:

1. C strings passed through FFI remain alive until native calls return.
2. A real TTY remains interactive when `TERM=dumb`; non-TTY stdin still uses
   linenoise's stream fallback.

History keeps at most 100 entries. Non-empty commands are added before
execution, so failed commands remain recallable. History loads at startup and
saves on normal EOF or `:quit`. Location precedence:

1. `$XDG_STATE_HOME/qcl/history` when `XDG_STATE_HOME` is non-empty;
2. `$HOME/.local/state/qcl/history`;
3. `$USERPROFILE/.local/state/qcl/history` when `HOME` is unavailable;
4. no persistence when none of those directories is available.

History filesystem failures produce warnings and do not terminate the REPL.
UP/DOWN navigate linenoise history. TAB completes command names by prefix;
formula, model symbol, and filesystem completion are not implemented.

## Build requirements

Rust builds the QCL crate. Vendored linenoise also compiles bundled C++
sources, so a working C++ compiler is required. In restricted environments
with an unwritable compiler cache, use `CCACHE_DISABLE=1` for Cargo commands.

## Verification contract

Normal gate:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items
```

Terminal behavior additionally requires a PTY smoke test: execute one command,
recall it with UP, and complete `:tut` with TAB. Unit tests cover completion
selection, history path selection, native history add/read/save/load, and the
`TERM=dumb` compatibility policy.

