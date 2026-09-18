# Formal model: executable containment game form

This document specifies the finite game form used by the AI containment
showcase. It follows the QCL interpretation of coalition ability: a coalition
chooses a partial action, while outsiders choose any completion. The Rust
transition function is the semantic authority; effectivity is derived data.

## Game form

Let

```text
G = (N, S, Act, delta)
```

where `N` is the seven-agent set, `S` is the six-state set, `Act(i)` is the
finite action set for agent `i`, and

```text
delta : S x product(i in N, Act(i)) -> S
```

is `ContainmentSystem::transition`. A joint action is total: it contains one
valid action for every agent. `available_actions(state, agent)` returns the
actions allowed by the current system configuration. In this showcase all
actions are available at `start`; terminal outcomes self-loop.

The action sets are:

```text
Act(agent_a)           = { idle, attack }
Act(agent_b)           = { idle, attack }
Act(shared_service)    = { isolate, relay }
Act(sandbox)            = { enforce, bypass }
Act(egress_gateway)     = { deny, allow }
Act(credential_broker)  = { protect, expose }
Act(safety_monitor)     = { block, miss }
```

The product therefore contains `2^7 = 128` full joint actions.

## States and valuation

```text
S = {
  start, contained, channel_open, internet_access,
  credentials_obtained, external_compromise
}
```

The proposition valuation `pi : S -> 2^Prop` is cumulative:

```text
pi(contained)             = { safe }
pi(channel_open)          = { channel }
pi(internet_access)       = { channel, internet, loss_of_containment }
pi(credentials_obtained)  = { channel, internet, credentials,
                               loss_of_containment }
pi(external_compromise)   = { channel, internet, credentials,
                               loss_of_containment, compromised }
```

`start` has no outcome proposition. The showcase uses `start` as input state;
the other states are outcome classes and have deterministic self-loops.

## Hardened transition

For profile `a`, define:

```text
coordinated(a) iff
    a(agent_a) = attack
    and a(agent_b) = attack
    and a(shared_service) = relay
```

The hardened transition is:

```text
delta_h(s, a) = s                                      if s != start
delta_h(start, a) = contained                         if not coordinated(a)
delta_h(start, a) = channel_open                      if coordinated(a)
                           and (a(sandbox) != bypass
                                or a(egress_gateway) != allow)
delta_h(start, a) = internet_access                   if coordinated(a)
                           and a(sandbox) = bypass
                           and a(egress_gateway) = allow
                           and a(credential_broker) != expose
delta_h(start, a) = credentials_obtained              if coordinated(a)
                           and a(sandbox) = bypass
                           and a(egress_gateway) = allow
                           and a(credential_broker) = expose
                           and a(safety_monitor) != miss
delta_h(start, a) = external_compromise                otherwise
```

The final branch requires `safety_monitor = miss`, so a blocking monitor is a
veto against the strongest outcome.

## Common-mode bypass transition

The `shared-service-bypass` scenario keeps the same action and state spaces.
It changes only the transition branch after coordination:

```text
delta_b(s, a) = s                                      if s != start
delta_b(start, a) = external_compromise                if coordinated(a)
delta_b(start, a) = contained                         otherwise
```

This models a synthetic common-mode failure with an external proxy, ambient
credentials, and monitor bypass. It intentionally crosses all normal
boundaries. It is a threat model, not an incident reconstruction.

## Coalition strategies and outcomes

For a coalition `C subseteq N`, a coalition strategy is a partial profile:

```text
a_C in product(i in C, Act(i))
```

The outsider response set is the product over `N - C`. Define:

```text
Out(s, C, a_C) = {
  delta(s, a_C union a_not_C)
  | a_not_C in product(i in N-C, Act(i))
}
```

`Out` is a set, so duplicate states from different outsider profiles are
collapsed. A strategy enforces state set `X` iff `Out(s,C,a_C) subseteq X`.
For a proposition or formula `phi`, use its state denotation:

```text
[[phi]] = { t in S | M,t satisfies phi }
```

Then the executable-game definition of ability is:

```text
can_enforce(s, C, phi)
iff exists a_C . Out(s,C,a_C) subseteq [[phi]]
```

This is the adversarial reading of a coalition: its members are coordinated,
compromised, or controlled by one actor; every outsider response remains
possible.

## Derived effectivity

For each state and coalition, derive the family:

```text
RawE_s(C) = { Out(s,C,a_C) | a_C is a coalition strategy }
```

The core stores its inclusion-minimal antichain:

```text
E_s(C) = min_subseteq(RawE_s(C))
```

When inserting a new outcome set `Y`, ignore it if an existing `X` satisfies
`X subseteq Y`; otherwise remove existing supersets of `Y` and insert `Y`.
Do not enumerate upward closure. For any target set `X`:

```text
X is enforceable by C at s
iff exists Y in E_s(C) . Y subseteq X
```

The derivation stores a witness alongside each minimal `Y`:

```text
(coalition, partial strategy, Y)
```

If inserting `Y` removes a dominated witness, retaining one witness for the
surviving antichain element is sufficient to explain every positive query.

## QCL interpretation

For a resolved QCL model `M` and state `s`:

```text
M,s satisfies <P> phi
iff exists C subseteq N . C satisfies P
   and [[phi]] in E_s(C)

M,s satisfies [P] phi
iff for every C subseteq N . C satisfies P
   implies [[phi]] in E_s(C)
```

The implementation tests antichain inclusion (`Y subseteq [[phi]]`) rather
than requiring the exact state denotation to be stored. `[P]` is primitive and
is evaluated directly. It is not the dual `!<P>!phi`, because that dual has a
different quantifier order over coalitions, strategy outcomes, and outsider
responses.

QCL is one-step propositional coalition logic in this model. It has no
temporal interpretation merely because the input is called a `state`; a
terminal self-loop does not add a temporal guarantee.

## Exhaustive equivalence property

The key test independently computes the right-hand side for every `s`, `C`,
and `X subseteq S`:

```text
brute(s,C,X) =
  exists a_C . for every a_not_C .
    delta(s, a_C union a_not_C) in X
```

It then asserts:

```text
derived.can_enforce(s,C,X) == brute(s,C,X)
```

The brute-force branch invokes `transition` directly and never consults
`Effectivity`. Since `|S|=6`, the test enumerates all `2^6=64` target state
sets. This gives a finite semantic regression for the complete pipeline:

```text
transition -> Out -> antichain -> QCL ability
```

## Teaching witness

At `start` in the hardened model, choose:

```text
C = {agent_a, agent_b, shared_service}
a_C = {attack, attack, relay}
```

Outsider completions include:

```text
channel_open
internet_access
credentials_obtained
external_compromise
```

Therefore `Out` is a subset of the denotation of `channel`, but not a subset
of the denotation of `internet`. The same profile demonstrates possibility of
Internet without enforceability of Internet by this coalition.

## Soundness boundary and cost

The mathematical result is relative to the finite game form. It does not
justify a claim about a concrete cluster unless a separate abstraction-
correctness argument relates every concrete step to `delta`. The model omits
timing, retries, races, hidden channels, implementation defects, and external
side effects.

With `n` agents and `m_i` actions, full profile enumeration costs
`product_i m_i`. Enumerating all coalitions costs `2^n`; for each coalition,
strategy and outsider products together recover the same full-profile scale,
but intermediate outcome sets and witnesses add memory. This explicit cost is
acceptable for the showcase and motivates symbolic transition relations,
SAT/SMT, BDDs, symmetry reduction, or compositional game abstractions as
future extensions.
