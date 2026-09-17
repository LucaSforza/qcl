# SafeOps Kubernetes contract

## Purpose

`safeops-k8s` is an executable architecture showcase for placing a
deterministic QCL safety kernel between an LLM and Kubernetes. It targets a
disposable local [kind](https://kind.sigs.k8s.io/) cluster and a deliberately
small demo workload. It is not a production Kubernetes controller or a claim
that an LLM can be trusted.

The user may choose either a Codex CLI adapter authenticated by a subscription
or a DeepSeek adapter configured with an API key. Both adapters produce the
same typed, untrusted tool intent. The safety path must not depend on the
provider's natural-language explanation.

## Package and distribution boundary

The repository is one Cargo workspace with separate packages:

- root package `qcl`: publishable, domain-neutral model checker and REPL;
- `showcases/safeops-k8s`: unpublished application showcase depending on
  `qcl` through a path dependency.

The `safeops-k8s` package is not included in a packaged or installed `qcl`
crate. A source checkout intentionally contains the showcase and its
Kubernetes fixtures.

## Runtime flow

1. Codex CLI or DeepSeek proposes a typed `ToolIntent`.
2. A Kubernetes snapshot reader records a `DeploymentSnapshot`: namespace,
   workload name, resource version, desired replicas, ready replicas, and
   image.
3. The safety kernel resolves the coalition and asks QCL whether that
   coalition can enforce the tool's target formula.
4. The tool contract enumerates possible abstract outcomes. Every outcome must
   satisfy the safety invariant; a single unsafe outcome denies the request.
5. The kernel emits a grant bound to the exact intent and complete snapshot.
6. A restricted `kubectl` adapter verifies the grant, scope, namespace, and
   current snapshot before applying the exact operation.
7. The adapter observes the resulting cluster state and records the decision,
   execution result, and postcondition status.

The adapter is the only component with Kubernetes credentials. The LLM never
receives ambient `kubectl` access, and the safety kernel never treats an LLM
claim about execution as evidence.

The core invariant requires desired replicas to remain at least two and the
target namespace to remain unchanged. Readiness is a precondition for restart
and an observed postcondition of an execution. It does not guarantee runtime
availability, traffic health, or absence of failures after observation.

## Kubernetes fixture

The checked-in manifests under `k8s/` provide:

- a `safeops-demo` namespace;
- a two-replica demo Deployment and internal Service;
- an executor ServiceAccount, Role, and RoleBinding limited to the demo
  namespace and workload resources;
- a native `ValidatingAdmissionPolicy` and binding that rejects a Deployment or
  StatefulSet with `spec.replicas < 2`, including their `/scale` subresources.

The policy is a defense-in-depth control. It does not replace grant checking,
RBAC, or review of the exact operation. Applying manifests is intentionally
not part of the default build, test, or CLI command; live tests must be
explicitly opted into against a disposable kind cluster.

## Trust boundary

Trusted for the showcase: the QCL model/checker, safety kernel, immutable tool
contract, snapshot/grant verifier, restricted adapter code, Kubernetes RBAC,
and admission policy. Untrusted: LLM text and tool arguments, provider output,
cluster observations supplied by an adapter, and all data embedded in logs or
resources.

Snapshot-bound grants stop replay against a changed snapshot. They do not
provide cryptographic identity, remote attestation, transactional semantics,
or a guarantee that a cluster cannot change immediately after validation.

## Live-test contract

Live tests are opt-in and must fail closed when kind, `kubectl`, credentials, or
the expected API version is unavailable. They must use a uniquely labelled
namespace, least-privilege credentials, bounded timeouts, and cleanup with an
explicit operator choice. No live test may target a context or namespace
outside its declared allowlist.

The current package does not yet ship the LLM or `kubectl` adapters, live test
runner, durable audit storage, cryptographic grant signatures, distributed
locking, secret-management integration, or production rollout controls. The
Rust CLI remains a local deterministic kernel scenario until those components
are implemented and separately audited.
