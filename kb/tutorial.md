# Tutorial: majority voting

This tutorial builds and queries a complete model. The example adapts the
majority-voting scenario discussed in section 6.1 of the *Quantified Coalition
Logic* paper: three voters choose between `coffee` and `tea`; every majority,
that is, every coalition of at least two voters, can enforce either outcome.

The ready-to-use file is `examples/majority_voting.qcl`. Start the REPL from the
project root with:

```text
cargo run
```

## 1. Declare the vocabulary and states

A model starts with agents, states, and propositions:

```text
model {
  agents { alice, bob, carol };
  states { coffee_outcome, tea_outcome };
  props { coffee, tea };
```

Agents form coalitions. States are possible outcomes. Propositions describe
what is true in each state.

## 2. Assign propositions to states

The two outcomes are mutually exclusive:

```text
  valuation {
    coffee_outcome: { coffee };
    tea_outcome: { tea };
  };
```

Thus `coffee` is true only in `coffee_outcome`, while `tea` is true only in
`tea_outcome`.

## 3. Define what coalitions can enforce

A line `state, coalition -> set_of_states` says that, from the given state, the
coalition can enforce an outcome in the set on the right. The model stores only
minimal sets; larger sets follow by monotonicity.

Coalitions with fewer than two agents can only guarantee that one of the two
outcomes occurs. Every majority can instead guarantee `coffee` or `tea`
separately:

```text
  effectivity {
    coffee_outcome, {} -> { coffee_outcome, tea_outcome };
    coffee_outcome, { alice } -> { coffee_outcome, tea_outcome };
    coffee_outcome, { bob } -> { coffee_outcome, tea_outcome };
    coffee_outcome, { carol } -> { coffee_outcome, tea_outcome };
    coffee_outcome, { alice, bob } -> { coffee_outcome };
    coffee_outcome, { alice, bob } -> { tea_outcome };
    coffee_outcome, { alice, carol } -> { coffee_outcome };
    coffee_outcome, { alice, carol } -> { tea_outcome };
    coffee_outcome, { bob, carol } -> { coffee_outcome };
    coffee_outcome, { bob, carol } -> { tea_outcome };
    coffee_outcome, { alice, bob, carol } -> { coffee_outcome };
    coffee_outcome, { alice, bob, carol } -> { tea_outcome };

    tea_outcome, {} -> { coffee_outcome, tea_outcome };
    tea_outcome, { alice } -> { coffee_outcome, tea_outcome };
    tea_outcome, { bob } -> { coffee_outcome, tea_outcome };
    tea_outcome, { carol } -> { coffee_outcome, tea_outcome };
    tea_outcome, { alice, bob } -> { coffee_outcome };
    tea_outcome, { alice, bob } -> { tea_outcome };
    tea_outcome, { alice, carol } -> { coffee_outcome };
    tea_outcome, { alice, carol } -> { tea_outcome };
    tea_outcome, { bob, carol } -> { coffee_outcome };
    tea_outcome, { bob, carol } -> { tea_outcome };
    tea_outcome, { alice, bob, carol } -> { coffee_outcome };
    tea_outcome, { alice, bob, carol } -> { tea_outcome };
  };
}
```

The abilities are the same in both states: the voting protocol does not depend
on the current outcome.

## 4. Load and validate

In the REPL:

```text
:load examples/majority_voting.qcl
:validate
```

Expected result:

```text
loaded 2 states, 3 agents, 2 properties
model valid
```

`:validate` also checks the weak-playability conditions. If you edit the file,
run `:load` and `:validate` again.

## 5. Explore coalition predicates

List all majorities:

```text
:coalitions size >= 2
```

Result:

```text
{alice, bob}
{alice, carol}
{bob, carol}
{alice, bob, carol}
```

More examples:

```text
:coalitions includes(alice)
:coalitions subset({alice, bob})
:coalitions excludes(carol) & size >= 2
```

Available predicates are `any`, `size >= n`, `includes(agent)`,
`excludes(agent)`, `subset({...})`, `superset({...})`, and `equals({...})`; you
can combine them with `!`, `&`, `|`, and parentheses.

## 6. Check formulas in the model

First check propositional evaluation:

```text
:check coffee_outcome coffee
:check tea_outcome coffee
:states tea
```

Results:

```text
true
false
tea_outcome
```

Now use QCL modalities. `[P] phi` means that every coalition satisfying `P` can
guarantee `phi`; `<P> phi` means that at least one such coalition can.

```text
:check coffee_outcome [size >= 2] coffee
:check coffee_outcome [size >= 2] tea
:check coffee_outcome <!(size >= 2)> coffee
:check tea_outcome <includes(alice)> coffee
```

Expected results:

```text
true
true
false
true
```

The first two queries formalize a requirement from the paper: every majority can
choose either outcome. The third shows that no minority can enforce `coffee`.
The last finds at least one coalition containing Alice that can enforce `coffee`.

Important: a coalition can guarantee `coffee` even when the current state is
`tea_outcome`. A modality describes ability to reach outcomes, not the current
truth of a proposition.

## 7. Try inferences

`:infer PREMISES |- CONCLUSION` checks semantic consequence in the loaded model.
The output is not a Boolean: it lists counterexample states. `(none)` means
that the inference holds in every state of the model.

Mutual exclusion of outcomes:

```text
:infer coffee |- !tea
```

Result:

```text
(none)
```

Intentionally false inference:

```text
:infer coffee |- tea
```

Result:

```text
coffee_outcome
```

`coffee_outcome` is a counterexample: the premise is true, but the conclusion is
false.

Ability does not imply current truth:

```text
:infer <any> coffee |- coffee
```

Result:

```text
tea_outcome
```

In `tea_outcome`, a majority can obtain `coffee`, but `coffee` is not true yet.

A QCL consequence that is valid in the model:

```text
:infer [size >= 2] coffee |- <includes(alice)> coffee
```

Result:

```text
(none)
```

If every majority can enforce `coffee`, then in particular one containing Alice
exists.

Separate multiple premises with a comma or semicolon:

```text
:infer coffee; [size >= 2] tea |- <any> tea
```

## 8. Modify and explore

Try these changes, one at a time:

1. remove the ability of `{ alice, bob }` to enforce `tea` and observe which
   universal formula becomes false;
2. change the threshold in queries from `size >= 2` to `size >= 3`;
3. add a fourth agent and decide whether majority should mean at least 2 or at
   least 3 agents;
4. use `:states FORMULA` to find all states satisfying a QCL formula.

Use the UP/DOWN arrows for history, TAB to complete command names, `:help` for
a summary, and `:quit` to exit.
