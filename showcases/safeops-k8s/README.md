# safeops-k8s

`safeops-k8s` is an optional, unpublished SafeOps application showcase. It
connects a local [kind](https://kind.sigs.k8s.io/) Kubernetes cluster to a
QCL safety kernel: an LLM proposes an operation, the kernel checks the
coalition and snapshot-bound invariants, and a restricted `kubectl` adapter
executes only an approved grant.

The intended LLM adapters are:

- Codex CLI, authenticated through a user's subscription;
- DeepSeek API, authenticated with a user-provided API key.

The repository currently contains the deterministic Rust kernel and the
declarative Kubernetes fixture under [`k8s/`](k8s/). Live cluster execution is
opt-in and remains a future adapter; the fixture is not applied by building or
testing this package.

## Run the local kernel scenario

```bash
cargo run -p safeops-k8s
```

The scenario demonstrates an allowed restart, stale snapshot-grant rejection,
human-gated deployment, and deletion blocked by an unsafe outcome. It does not
call an LLM or a cluster and prints no chain-of-thought.

## Kubernetes fixture

The manifests describe a `safeops-demo` namespace, a two-replica demo service,
a least-privilege executor service account/RBAC role, and a native
`ValidatingAdmissionPolicy` rejecting workloads with `spec.replicas < 2`.
Review them before applying to a disposable kind cluster. The admission policy
requires a Kubernetes version that supports the native policy APIs.

## Trust boundary and limits

The QCL kernel, policy/model, snapshot reader, grant verifier, and executor
adapter are trusted. LLM output, Kubernetes observations, and tool arguments
are untrusted. Snapshot binding prevents replay against a changed observation;
it does not prove that Kubernetes, the LLM, the host, or credentials are
honest. This example has no production hardening, cryptographic attestation,
durable audit store, distributed locking, secret-management boundary, or
guarantee against failures between an adapter write and its observation.

The published `qcl` package remains domain-neutral and does not contain this
showcase.
