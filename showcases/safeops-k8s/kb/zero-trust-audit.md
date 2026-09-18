# Kubernetes/cloud zero-trust control-plane audit

## Boundary

`safeops-k8s audit` is deterministic, offline QCL model checking. It never
contacts Kubernetes, cloud APIs, an LLM, or an executor. It models security
outcome classes, not Kubernetes API traces or a discovered cluster
configuration. Existing SafeOps runtime flow remains separate: LLM -> kernel
-> snapshot-bound grant -> restricted executor. RBAC, admission policy/OPA,
and runtime grant checks remain required there.

## Model

Principals carry local metadata: name, trust domain, automation flag, and
abstract capabilities. QCL formulas quantify explicit coalitions only; trust
domains, roles, and parameterized cloud capabilities are not core DSL terms.
Each scenario has outcome states `normal`, `safe_change`, `data_deleted`,
`secret_exfiltrated`, `cluster_root`, and `policy_bypassed`. Valuations expose
the corresponding `safe`, `deleted`, `exfiltrated`, `root`, and `bypassed`
propositions.

Effectivity is generated from capability routes. A route is enforceable only
when every listed principal belongs to coalition. Missing routes receive the
universal outcome, so they cannot guarantee a narrow dangerous state.
Effectivity entries are explicitly generated for each coalition because QCL
stores entries by exact coalition; outcome supersets are implicit in QCL's
minimal-outcome antichain.

Threat model: coalition members may collude, share capabilities, chain them,
abuse permissions, and seek dangerous outcomes. Principals outside coalition
need not cooperate. Therefore `cluster_root` is enforced only if a coalition
has a minimal outcome contained in `{cluster_root}`. A coalition with possible
`normal` and `cluster_root` has outcome `{normal, cluster_root}` and cannot
guarantee `root`.

## Audit contract

`secure` must pass automation-root, human-delete, security-approver-secret,
automated-domain-secret, and legitimate safe-change checks. It must keep
`break_glass` unable to bypass policy. `privilege-escalation` deliberately
creates an automatic root route. `break-glass-bypass` deliberately grants
break-glass root, deletion, and policy bypass routes, defeating veto checks.

Failure diagnostics enumerate a matching coalition. Existential prohibition
failures show coalition plus its guaranteed outcome. Universal failures show a
matching coalition that cannot guarantee target.

`!<P> phi` means no selected coalition can guarantee `phi`. `[P] !phi` means
every selected coalition can guarantee absence of `phi`; it is stronger and is
not substituted for former. Universal ability is primitive in QCL.

## Scale and future work

Audit enumerates coalitions, so demo targets moderate principal counts.
Future work: DSL trust-domain/capability predicates, parameterized capability
models, import from real cluster/IAM inventory with reviewable translation, and
symbolic/BDD-backed coalition search. None exists in this version.
