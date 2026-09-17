# DSL Contract

## Design principles

- ASCII-first; Unicode aliases may follow later.
- Declarations terminate with semicolons.
- Names are identifiers and resolved after parsing.
- Boolean precedence, strongest to weakest: negation, conjunction, disjunction, implication.
- Parser errors include byte span, expected token, and found token.

## Model declarations

Executable declarations define agents, states, propositions, state valuations, and minimal effectivity outcomes. A complete model has this form (names are ASCII identifiers):

```text
model {
  agents { alice, bob };
  states { s0, s1 };
  props { ready, done };
  valuation {
    s0: { ready };
    s1: { done };
  };
  effectivity {
    s0, { alice }: { s1 };
    s0, {} -> { s0, s1 };
  };
}
```

Declarations terminate with semicolons. Empty sets are valid. `properties` and
`atoms` are accepted aliases for `props`; `->` and `:` are equivalent in an
effectivity entry. Repeated entries for one state/coalition pair are inserted
as minimal outcomes and the model's antichain drops dominated sets. Unknown
state, proposition, or agent names and duplicate declaration names are errors.

## Coalition predicates

Primitive surface forms:

- `subset({agent, ...})`
- `superset({agent, ...})`
- `size >= integer`
- negation, conjunction, disjunction, parentheses

Derived forms desugar during resolution:

- `equals({agent, ...})`
- `includes(agent)`
- `excludes(agent)`
- `any`

Predicate operators use `!`, `&`, and `|` (in that precedence order). Formula
operators use `!`, `&`, `|`, and right-associative `->`; angle brackets denote
existential ability and square brackets universal ability, for example
`<subset({alice})> ready` and `[any] !done`.

## QCL formulas

Formula atoms are `true`, `false`, proposition names, negation, conjunction, disjunction, implication, existential ability `<predicate> formula`, and universal ability `[predicate] formula`.

## REPL commands

- `:load FILE`
- `:validate`
- `:check STATE FORMULA`
- `:states FORMULA`
- `:infer PREMISES |- CONCLUSION`
- `:coalitions PREDICATE`
- `:tutorial`
- `:help`
- `:quit`

TAB completes these command names, including `:q`. Completion currently does
not inspect loaded model symbols, formulas, predicates, or filesystem paths.
UP and DOWN navigate current and persisted command history.

History stores at most 100 non-empty commands. It uses
`$XDG_STATE_HOME/qcl/history` when configured, otherwise
`$HOME/.local/state/qcl/history` (or `USERPROFILE` fallback). History I/O
failures are warnings rather than command failures.

`:tutorial` takes no arguments and prints the executable, step-by-step
majority-voting walkthrough in [`kb/tutorial.md`](tutorial.md). It constructs a
model, validates it, and demonstrates predicates, model checking, and valid and
invalid model-relative inferences. It is available before a model is loaded.

The parser exposes source AST nodes with half-open byte spans. Resolution is
explicit: parsed formulas and predicates resolve against model symbol tables to
the typed IDs used by the checker.
