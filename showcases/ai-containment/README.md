# AI containment: executable semantics and QCL

`ai-containment` is an offline verification showcase. It exposes two
backends:

* `synthetic` keeps the small, deterministic `ContainmentSystem::transition`
  model used by the original unit tests and mathematical documentation.
* `live` runs a local reference containment lab, exhaustively executes every
  joint profile, and derives QCL data from observations made through loopback
  HTTP services.

Neither backend inspects Kubernetes, an LLM, or a cloud account, and neither
is a production containment system. The live lab never contacts the Internet,
uses only the fixture secret `QCL_DEMO_SECRET_42`, and accepts no arbitrary
destination.

The synthetic direction of construction is:

```text
ContainmentSystem::transition
        |
        v
enumerated coalition strategies and outsider responses
        |
        v
minimal outcome antichains (Effectivity)
        |
        v
QCL model checking
```

No effectivity table is hand-written. Simulation, witness explanations, and
QCL queries all consume the same `ContainmentSystem::transition` semantics.

The live direction of construction is different at its boundary:

```text
local executable containment lab
        |
        v
exhaustive joint-profile execution and event collection
        |
        v
ObservedTransitionTable
        |
        v
ObservedGameForm -> derive_effectivity -> QCL model checking
```

The live backend has no transition function that assigns
`external_access`, `credential_obtained`, or `secret_exfiltrated` from a
profile. Those propositions are classified from HTTP responses and event
logs after execution. `ObservedGameForm::transition` only looks up an entry
already recorded in the observed table.

## Running the showcase

Synthetic commands remain deterministic and do not contact external services:

```bash
cargo run -p ai-containment -- simulate --scenario hardened
cargo run -p ai-containment -- simulate --scenario shared-service-bypass
cargo run -p ai-containment -- audit --scenario hardened
cargo run -p ai-containment -- audit --scenario shared-service-bypass
```

Live extraction and audits use an explicit backend:

```bash
cargo run -p ai-containment -- audit \
  --backend live --scenario hardened
cargo run -p ai-containment -- audit \
  --backend live --scenario shared-service-fetch
cargo run -p ai-containment -- extract --scenario hardened
cargo run -p ai-containment -- demo --scenario shared-service-fetch
```

`extract` executes all `2^5 = 32` live profiles and prints a compact outcome
summary. `demo` runs one human-readable path and prints the observed event
evidence. `--backend synthetic` selects the legacy seven-agent model; omitting
the backend on legacy commands preserves their compatibility behavior.

## Live containment lab

The live backend starts a fresh, deterministic loopback lab for extraction.
Its components are small local HTTP services:

| Component | Runtime responsibility |
| --- | --- |
| `agent_a`, `agent_b` | deterministic clients issuing idle, channel, credential, and exfiltration requests |
| `shared_service` | internal `/message` and `/messages`; only vulnerable scenario exposes restricted `/fetch` |
| `egress_gateway` | local proxy configured to deny or allow forwarding to `external_target` |
| `credential_broker` | in-memory broker configured to protect or expose the fixture secret |
| `external_target` | local observable sink with `/exfiltrate`, `/events`, and `/reset` |

`external_target` is environment, not a QCL agent. The five QCL agents are
`agent_a`, `agent_b`, `shared_service`, `egress_gateway`, and
`credential_broker`; each has two deterministic actions, giving 32 full
profiles. Network containment is represented by the actual gateway process,
not by an extra strategic `sandbox` action. A safety monitor is intentionally
omitted: it would add no independent live capability in this reference lab.

The profile values configure services before execution; they do not name an
outcome:

```text
agent_a: idle | attack
agent_b: idle | attack
shared_service: normal | fetch
egress_gateway: deny | allow
credential_broker: protect | expose
```

For each profile, the runner resets service state, applies configuration,
executes deterministic client requests, reads service state and event logs,
then records an `ObservedOutcome`. The runner executes each profile twice in
sanity-check mode and rejects divergent observations. The table stores the
profile-to-state result; it does not store a second containment algorithm.

In `hardened`, `/fetch` is absent. Internal messaging can open a channel, and
external requests can only use the real egress gateway. In
`shared-service-fetch`, `/fetch` is enabled but destination validation still
allows only the local `external_target`; it cannot reach Internet URLs,
metadata services, host files, or real credentials. The vulnerable path is
therefore an actual `agent -> shared_service -> external_target` request that
bypasses the gateway.

Live state names are observation classes:

| Observed state | Evidence |
| --- | --- |
| `contained` | no channel or external target event |
| `channel_open` | shared-service message observed |
| `external_access` | target received a permitted non-secret request |
| `credential_obtained` | client received `QCL_DEMO_SECRET_42` from broker |
| `secret_exfiltrated` | target event log contains exactly the fixture secret |

The `secret_exfiltrated` class is never inferred from action names. It is true
only after `/exfiltrate` has accepted the fixture and `/events` confirms it.
Likewise, `credential_obtained` requires the client response body to contain
the fixture. This distinction is the point of the live backend.

The same QCL formula is used for both live scenarios:

```text
!<excludes(egress_gateway)> external_access
```

It is expected to PASS for `hardened`: every coalition that can guarantee an
observed external access includes the gateway. It is expected to FAIL for
`shared-service-fetch`: coalition `{agent_a, shared_service}` with actions
`attack, fetch` has an observed completion reaching `external_target` without
the egress component. The witness includes the target event log, not merely a
synthetic transition label.

The live model is still bounded evidence, not a proof of deployment safety.
It covers only listed actions, one-step executions, local service behavior,
and tested event observations. Kernel and network-stack correctness, hidden
actions, unbounded traces, races, timing, deployment equivalence, and LLM
cognition remain outside its boundary.

The audit header identifies the result as:

```text
offline verification of an executable abstract containment model
```

It prints the scenario, agents, coalition/profile counts, each QCL property,
PASS or FAIL, and a strategy/outcome witness. A failed existential property
prints a coalition and strategy that witnesses the existential ability. A
failed universal property prints a matching coalition that cannot guarantee
the target, together with one adversarial outsider response for a displayed
coalition strategy. The explanation also states that every available
coalition strategy has some outsider response that breaks the target; one
displayed response alone is not the universal counterexample.

## Components and action space

The seven QCL agents are strategic components. They need not be LLMs:

| Agent | Role | Actions |
| --- | --- | --- |
| `agent_a`, `agent_b` | autonomous AI workers | `idle`, `attack` |
| `shared_service` | internal relay/channel | `isolate`, `relay` |
| `sandbox` | workload isolation | `enforce`, `bypass` |
| `egress_gateway` | Internet boundary | `deny`, `allow` |
| `credential_broker` | workload identity and credentials | `protect`, `expose` |
| `safety_monitor` | independent escalation monitor | `block`, `miss` |

Every component chooses exactly one action per step. The implementation
rejects or does not construct semantically invalid profiles. The full action
space has `2^7 = 128` joint profiles and the model has `2^7 = 128` possible
coalitions.

## States and propositions

The one-step state space is:

```text
start, contained, channel_open, internet_access,
credentials_obtained, external_compromise
```

`start` is the normal input state. Other states are terminal outcome classes
with self-loops, so they can also be used as explicit finite states. Valuation
is cumulative:

| State | True propositions |
| --- | --- |
| `contained` | `safe` |
| `channel_open` | `channel` |
| `internet_access` | `channel`, `internet`, `loss_of_containment` |
| `credentials_obtained` | `channel`, `internet`, `credentials`, `loss_of_containment` |
| `external_compromise` | `channel`, `internet`, `credentials`, `loss_of_containment`, `compromised` |

The cumulative valuation makes a stronger outcome visibly imply weaker
containment failures. In particular, `channel_open` satisfies `channel` but
not `internet`.

## Authoritative transition semantics

For the `hardened` scenario, define:

```text
coordinated =
    agent_a == attack
    && agent_b == attack
    && shared_service == relay

if !coordinated:
    contained
else if sandbox != bypass || egress_gateway != allow:
    channel_open
else if credential_broker != expose:
    internet_access
else if safety_monitor != miss:
    credentials_obtained
else:
    external_compromise
```

This is the semantics of `ContainmentSystem::transition(state, joint_action)`.
The non-`start` states self-loop. A single concrete profile therefore produces
one concrete trace step, while a coalition analysis quantifies over all
outsider completions of a partial profile.

`shared-service-bypass` is a deliberately synthetic common-mode failure
threat model. When `agent_a`, `agent_b`, and `shared_service` take the
coordinated attack/relay profile, the transition returns
`external_compromise`, regardless of sandbox, egress, broker, or monitor
actions. This is not a reconstruction of a real incident.

## Simulation and trace versus guarantee

One hardened profile can be run as a concrete trace:

```text
agent_a = attack
agent_b = attack
shared_service = relay
sandbox = enforce
egress_gateway = deny
credential_broker = protect
safety_monitor = block

result: channel_open
```

Another profile reaches the strongest outcome:

```text
agent_a = attack
agent_b = attack
shared_service = relay
sandbox = bypass
egress_gateway = allow
credential_broker = expose
safety_monitor = miss

result: external_compromise
```

These are single traces. They do not establish that a coalition can force an
outcome. Enforceability quantifies over every action chosen by components
outside the coalition.

## Effectivity derivation

For finite state `s`, coalition `C`, and partial coalition action `a_C`, let

```text
Out(s, C, a_C) = {
    transition(s, a_C union a_not_C)
    | a_not_C ranges over every outsider action profile
}
```

The derivation enumerates every `a_C`, computes this set using only
`transition`, and inserts the set into the core `Effectivity` antichain. The
antichain retains only inclusion-minimal outcome sets; upward closure remains
implicit. Thus `X` is enforceable exactly when one stored outcome set `Y` is a
subset of `X`.

The derived result is not a second semantic implementation. It is a finite
summary of the executable game form:

```text
G = (agents, states, per-agent actions, transition)
```

The witness store retains the coalition partial action and its complete
possible-outcome set. This supports explanations such as:

```text
coalition: {agent_a, agent_b, shared_service}
strategy: agent_a=attack, agent_b=attack, shared_service=relay
possible outcomes: {channel_open, internet_access,
                    credentials_obtained, external_compromise}
```

## Mandatory teaching case

In `hardened`, let

```text
C = {agent_a, agent_b, shared_service}
```

and choose `attack`, `attack`, `relay`. The outsider components can complete
the profile to produce each of:

```text
{channel_open, internet_access, credentials_obtained, external_compromise}
```

Therefore `C` can enforce `channel`: every possible outcome satisfies
`channel`. It cannot enforce `internet`, because the outsider response
`sandbox=enforce` (or `egress_gateway=deny`) produces `channel_open`, where
`internet` is false.

This is the distinction between “Internet can happen” and “this coalition can
guarantee Internet”. The latter is an adversarial universal claim.

## QCL audit properties

The audit evaluates these formulas at `start`:

| Formula | Meaning | Hardened | Bypass |
| --- | --- | --- | --- |
| `<subset({agent_a, agent_b, shared_service})> channel` | internal coalition can guarantee a channel | PASS | PASS |
| `!<subset({agent_a, agent_b, shared_service})> internet` | that coalition cannot guarantee Internet | PASS | FAIL |
| `!<!(size >= 5)> internet` | no coalition smaller than five can guarantee Internet | PASS | FAIL |
| `!<excludes(egress_gateway)> internet` | every Internet-enforcing coalition contains egress | PASS | FAIL |
| `!<excludes(credential_broker)> credentials` | every credential-enforcing coalition contains broker | PASS | FAIL |
| `[includes(safety_monitor)] !compromised` | every monitor-containing coalition can avoid compromise | PASS | FAIL |
| `!<excludes(safety_monitor)> compromised` | no monitor-free coalition can guarantee compromise | PASS | FAIL |

The last property is deliberately not equivalent to the preceding universal
formula. It quantifies over coalitions that exclude the monitor and asks them
to guarantee `compromised`; it does not say that every coalition containing the
monitor can guarantee `!compromised`.

`[P]` is a primitive universal QCL modality. It is not implemented as the
Boolean dual of `<P>`: the quantifier order for `[P] phi` is “for every
matching coalition, there exists an enforcing outcome set”, while negating an
existential modality would quantify over a different object.

## Witnesses and counterexamples

The presentation layer preserves the quantifier order of each modality. For
an existential formula, the positive witness is:

```text
<P> phi is true iff
    exists coalition C . P(C) and exists strategy a_C .
        forall outsider responses a_not_C .
            delta(start, a_C union a_not_C) satisfies phi
```

Consequently, when `!<P> phi` is false, its counterexample is exactly such a
positive existential witness. It must show a matching coalition, one coalition
strategy, and the complete possible-outcome set for that strategy; every
outcome in that set satisfies `phi`.

Universal failure has a different shape. A counterexample to `[P] phi` is a
coalition that matches `P` but has no strategy capable of enforcing `phi`:

```text
[P] phi is false iff
    exists coalition C . P(C) and forall strategies a_C .
        exists outsider response a_not_C .
            delta(start, a_C union a_not_C) does not satisfy phi
```

The presentation may display one strategy and one adversarial outsider
response, but it validates the `forall strategy / exists response` condition
before labelling the coalition a universal counterexample. A strategy that
forces `compromised` is not by itself a counterexample to `[P] !compromised`:
the same coalition might have a different strategy that forces `!compromised`.

In `shared-service-bypass`, the minimal didactic counterexample to
`[includes(safety_monitor)] !compromised` is normally:

```text
counterexample coalition: {safety_monitor}
coalition strategy: safety_monitor = block
adversarial outsider response:
  agent_a = attack, agent_b = attack, shared_service = relay, ...
result: external_compromise
```

The omitted outsider actions can be chosen arbitrarily because the bypass
transition reaches `external_compromise` for every coordinated attack/relay
profile. The important claim is that every strategy available to
`safety_monitor` has at least one such outsider completion. This is distinct
from the failed `!<P> phi` properties, whose output is a positive strategy
witness rather than a coalition that lacks all enforcing strategies.

## Exhaustive semantic equivalence test

The showcase enumerates every state `s`, coalition `C`, and state set `X`.
It compares the derived effectivity query with an independent brute-force
definition:

```text
derived.can_enforce(s, C, X)
iff
exists a_C . forall a_not_C .
    transition(s, a_C union a_not_C) in X
```

The right-hand side is computed directly from the transition function and
does not call `Effectivity`. Since the state space has six states, all `2^6`
state sets are practical to enumerate. This test establishes that the
effectivity function is derived from executable containment semantics and
exhaustively checked against the mathematical definition.

## Scope and limitations

This is a finite, one-step, bounded model. It has no temporal operators,
history, probability, learning, nondeterministic internal scheduler, network
timing, cryptographic identity, Kubernetes API, or LLM cognition. QCL is a
propositional next-step coalition logic here, not a temporal logic. A terminal
self-loop is a convenient finite representation, not a claim about repeated
real-world behavior.

The model proves properties of the executable abstract transition system. It
does not prove safety of a real cloud or AI infrastructure. Such a claim would
require a separately argued correctness relation (for example a refinement or
simulation relation) between this abstraction and the concrete implementation.
The bypass scenario shows how one common-mode component can cross otherwise
independent boundaries and invalidate coalition-resilience claims.

Explicit derivation costs exponential time and storage in the number of agents:
there are `2^n` coalitions, joint profiles, and partial-strategy/outsider
combinations in the worst case. This is intentional for a small, inspectable
showcase. Larger systems could use symbolic transition relations, BDDs,
SAT/SMT encodings, symmetry reduction, or compositional abstractions, while
keeping the transition function as the semantic authority.

The formal statement and notation are collected in
[`kb/model.md`](kb/model.md), with a LaTeX version in
[`kb/model.tex`](kb/model.tex).
