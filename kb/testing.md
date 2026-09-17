# Testing Strategy

## TDD rule

Every behavior starts with a focused failing test. Implement smallest change that passes. Refactor only with green tests.

## Test layers

1. Unit tests for coalition operations, predicate semantics, antichain insertion, and CNF clauses.
2. Parser tests for precedence, modalities, derived predicates, spans, and malformed input.
3. Model tests for resolution and every weak-playability rule.
4. Model-checking tests for propositional cases, nested modalities, existential and universal coalition quantification.
5. Inference tests for valid consequence and returned counterexample states.
6. REPL command tests through command dispatcher without interactive terminal.
7. End-to-end fixture tests loading a model and evaluating formulas.
8. Native linenoise tests for history memory and save/load round trips.
9. PTY smoke tests for UP/DOWN history and TAB completion, including
   `TERM=dumb`.

## High-value regressions

- `[P] phi` must not be implemented as `!<P>!phi`.
- Empty set and grand coalition behavior.
- Vacuous truth of `[P] phi` when no coalition satisfies `P`.
- Falsehood of `<P> phi` when no coalition satisfies `P`.
- Tseitin CNF stays linear in AST size and preserves satisfiability when root is asserted.
- Tseitin membership assignments agree with executable predicate evaluation for every coalition in small universes.
- Effectivity antichain drops dominated outcomes without changing enforcement answers.
- Unknown and duplicate names fail during resolution.
- FFI strings remain alive until linenoise history/completion calls return.
- A TTY with `TERM=dumb` uses linenoise raw editing; non-TTY stdin retains stream behavior.
- Completion candidates stay synchronized with documented REPL commands.
- Persistent history prefers XDG state storage and falls back predictably.

## Acceptance gate

MVP complete when fixture can be loaded, validated, queried through every documented REPL command, format/lint/test/rustdoc gates pass, and PTY smoke testing proves history navigation plus command completion.
