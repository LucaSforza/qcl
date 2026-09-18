# safeops-k8s

`safeops-k8s` is an optional, unpublished SafeOps application showcase. It
connects a local [kind](https://kind.sigs.k8s.io/) Kubernetes cluster to a
QCL safety kernel: an LLM proposes an operation, the kernel checks the
coalition and snapshot-bound invariants, and a restricted `kubectl` adapter
executes only an approved grant.

The intended LLM adapters are:

- Codex CLI, authenticated through a user's subscription;
- DeepSeek API, authenticated with a user-provided API key.

The repository contains the deterministic Rust kernel, real Codex and DeepSeek
providers, a bounded `kubectl` adapter, and the declarative Kubernetes fixture
under [`k8s/`](k8s/). Live execution is opt-in; normal builds and tests never
contact a provider or cluster.

## Run the local kernel scenario

```bash
cargo run -p safeops-k8s
```

The scenario demonstrates policy decisions only: safe operations, a
human-gated image update, and deletion blocked by an unsafe outcome. It does
not claim that an operation was executed and prints no chain-of-thought.

## Offline zero-trust control-plane audit

Run synthetic, reproducible coalition-capability audits:

```bash
cargo run -p safeops-k8s -- audit --scenario secure
cargo run -p safeops-k8s -- audit --scenario privilege-escalation
cargo run -p safeops-k8s -- audit --scenario break-glass-bypass
```

This mode uses neither LLM nor Kubernetes/cloud API. It builds a deterministic
QCL model of abstract security outcomes and prints formula results plus concrete
counterexample coalitions for intentionally unsafe scenarios. It does not
inspect a real cluster or replace runtime RBAC, admission policy/OPA, grant
checks, or executor restrictions. Full model, threat assumptions, QCL
semantics, formulas, limits, and future work: [zero-trust audit contract](kb/zero-trust-audit.md).

## Interactive timeline

After applying the fixture and selecting a real provider, start the terminal:

```bash
SAFEOPS_LLM_PROVIDER=codex cargo run -p safeops-k8s -- interactive
```

Use `status`, `plan <goal>`, `approve`, `execute`, `discard`, `timeline`,
`help`, and `quit`. `plan` reads the live Deployment and asks the selected
real provider for one typed action; it never executes. `execute` is a separate
human command and is the only interactive command allowed to consume a grant
through the bounded `kubectl` adapter. `approve` only retries an
`update_image` proposal denied for missing human approval. Timeline output is
printed as each stage occurs, can be replayed with `timeline`, and redacts
goals, images, provider reasoning, credentials, and API keys. Starting a new
`plan` always discards any prior pending grant before external calls.

The kernel models a `DeploymentSnapshot` (namespace, workload name, resource
version, desired/ready replicas, and image) and typed actions such as inspect,
restart, rollback, scale, image update, and namespace deletion. Grants bind to
the complete snapshot. The safety invariant keeps desired replicas at least
two and preserves the namespace. Readiness is checked as an action
precondition/postcondition; it is an observation, not a runtime availability
guarantee.

## Kubernetes fixture

The manifests describe a `safeops-demo` namespace, a two-replica demo service,
a least-privilege executor service account/RBAC role, and a native
`ValidatingAdmissionPolicy` rejecting Deployment/StatefulSet workloads with
`spec.replicas < 2`, including `/scale` requests.
Review them before applying to a disposable kind cluster. The admission policy
requires a Kubernetes version that supports the native policy APIs.

Create the dedicated cluster, then apply and verify the fixture against its
exact allowlisted context:

```bash
kind create cluster --name safeops-qcl
showcases/safeops-k8s/scripts/live-test.sh
```

Run real integrations explicitly:

```bash
SAFEOPS_LLM_PROVIDER=codex \
  cargo test -p safeops-k8s --test live_llm -- --ignored --nocapture
SAFEOPS_LLM_PROVIDER=codex \
  cargo test -p safeops-k8s --test live_e2e -- --ignored --nocapture
cargo test -p safeops-k8s --test live_kubernetes -- --ignored --nocapture
```

For DeepSeek, select `SAFEOPS_LLM_PROVIDER=deepseek` and supply
`DEEPSEEK_API_KEY`; optional `DEEPSEEK_MODEL` overrides `deepseek-chat`.

## Trust boundary and limits

The QCL kernel, policy/model, snapshot reader, grant verifier, and executor
adapter are trusted. LLM output, Kubernetes observations, and tool arguments
are untrusted. Snapshot binding prevents replay against a changed observation;
it does not prove that Kubernetes, the LLM, the host, or credentials are
honest. This example has no production hardening, cryptographic attestation,
durable audit store, distributed locking, secret-management boundary, or
guarantee against failures between an adapter write and its observation.

The Codex subscription path starts a local Codex agent in an isolated temporary
working directory with an ephemeral session, ignored user configuration/rules,
read-only sandbox, stripped provider/Kubernetes environment variables, and
structured output. Codex CLI and its host sandbox remain trusted components;
this path is not equivalent to a tool-free model API. DeepSeek receives only
the serialized snapshot and goal over an HTTPS API request.

The published `qcl` package remains domain-neutral and does not contain this
showcase.
