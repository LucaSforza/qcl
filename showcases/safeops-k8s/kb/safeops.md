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

1. A Kubernetes snapshot reader records a `DeploymentSnapshot`: namespace,
   workload name, resource version, desired replicas, ready replicas, and
   image.
2. Codex CLI or DeepSeek proposes a typed action from that snapshot and the
   operator goal; the CLI wraps it in a `ToolIntent`.
3. The safety kernel resolves the coalition and asks QCL whether that
   coalition can enforce the tool's target formula.
4. The tool contract enumerates possible abstract outcomes. Every outcome must
   satisfy the safety invariant; a single unsafe outcome denies the request.
5. The kernel emits a grant bound to the exact intent and complete snapshot.
6. A restricted `kubectl` adapter verifies the grant, scope, namespace, and
   current snapshot before applying the exact operation.
7. The adapter observes the resulting cluster state and records the decision,
   execution result, and postcondition status.

The Kubernetes adapter is the only component intentionally given an execution
path. The DeepSeek model receives no Kubernetes credentials. The Codex
subscription adapter starts a local agent in an isolated temporary working
directory with an ephemeral session, ignored user configuration/rules, a read-only sandbox,
and sensitive provider/Kubernetes environment variables removed. Codex CLI
and its sandbox remain part of the trusted computing base; this path cannot be
claimed equivalent to a tool-free API model. The safety kernel never treats an
LLM claim about execution as evidence.

The core invariant requires desired replicas to remain at least two and the
target namespace to remain unchanged. Readiness is a precondition for restart
and an observed postcondition of an execution. It does not guarantee runtime
availability, traffic health, or absence of failures after observation.

## Interactive CLI contract

The showcase CLI is a thin session viewer and orchestrator. It must not become
an alternative policy engine. It keeps one in-memory timeline for the current
session; durable audit storage remains out of scope. Interactive mode is
read-only until the operator explicitly starts an execution cycle.

Minimum commands:

- `status`: read and render the current allowlisted Deployment snapshot;
- `plan <goal>`: perform one bounded cycle through LLM proposal, typed parsing, QCL
  decision, and grant creation, without mutating Kubernetes;
- `approve`: add human approval only to a pending image update previously
  denied because that approval was missing;
- `execute`: explicitly confirm execution of the current valid grant, then
  read and render the post-snapshot;
- `discard`: remove the current pending proposal or grant;
- `timeline`: render the ordered events collected in the current session;
- `help` and `quit`.

`execute` must refuse when no current grant exists, when the grant is stale or
already consumed, or when any precondition fails. The separate `execute`
command is the operator confirmation; planning can never execute implicitly.
Starting another `plan` discards the previous pending grant before reading the
cluster or contacting a provider.
There is no command that accepts arbitrary shell text or arbitrary `kubectl`
arguments. A non-interactive invocation may select the provider and timeout,
but must preserve the same allowlist and confirmation boundary.

## Timeline event contract

Each event has a monotonic sequence number, a stage, a status, and a bounded
human-readable summary. Events appear live and remain available for replay.
The renderer shows safe typed metadata; it never prints raw prompts, raw provider responses,
HTTP headers, environment variables, kubeconfig contents, credentials, or
model chain-of-thought.

Expected stages for one cycle are:

1. `snapshot` or `snapshot.read` — snapshot obtained or rejected;
2. `llm.request` — provider request started, without prompt or secret values;
3. `llm.proposal` — typed action parsed, or provider/parse failure;
4. `qcl.decision` — allow/deny and compact policy reason;
5. `grant` — snapshot-bound in-memory grant result;
6. `human approval` — recorded only for approval-gated image changes;
7. `execution` — started and completed/failed adapter operation, only after
   the explicit `execute` command;
8. `post-snapshot` or `snapshot.post` — observed postcondition or failure.

Denied actions and errors remain visible in the timeline. They must be
represented by typed status and redacted error summaries, not by printing
provider explanations. The timeline is explanatory UI, not evidence that an
operation succeeded: only the adapter result and post-snapshot establish
execution outcome.

## Interactive safety flow

`plan` follows this exact order: discard old pending grant → read snapshot →
ask selected LLM for one typed intent → parse and validate intent →
evaluate QCL contract → issue a snapshot-bound grant or deny. `execute` then
re-reads the snapshot → verifies grant identity, scope, and snapshot equality →
consumes grant once →
invokes the restricted `kubectl` adapter → reads post-snapshot → records
postcondition result. The LLM never receives a grant and never executes a
Kubernetes command.

Every failure is fail-closed. Provider timeout or malformed output, unknown
action, snapshot read error, QCL denial, grant mismatch/replay, operator
decline, adapter timeout/non-zero exit, or postcondition mismatch produces no
further mutation and a terminal `cycle.failed`/denied event. A successful
`kubectl` process without a valid post-snapshot is not reported as success.

## CLI acceptance criteria

The interactive showcase is complete only when all criteria hold:

- a fresh session exposes the minimum commands above and starts with
  no execution grant;
- `plan` renders ordered LLM → parse → QCL → grant events and performs no
  Kubernetes mutation;
- `execute` requires explicit confirmation, executes only a valid current
  grant, and renders adapter plus post-snapshot events;
- an unsafe proposal (for example scaling below two replicas) is visibly
  denied before `kubectl` starts, while direct admission-policy rejection is
  also visible in live tests;
- replaying a consumed grant, changing the Deployment resource version, or
  losing snapshot/postcondition access ends cycle without mutation;
- provider secrets, Kubernetes credentials, raw model output, and
  chain-of-thought are absent from terminal output and timeline summaries;
- timeline output stays bounded and remains usable when provider or cluster is
  unavailable; no failure falls through to an unguarded shell command.

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

The package ships the LLM and `kubectl` adapters as opt-in integrations. The
`KubectlAdapter` is bounded and allowlisted to the fixture context, namespace,
and Deployment, and always supplies explicit context, namespace, and executor
impersonation. It rejects namespace deletion before any command. It rereads
the snapshot before consuming a grant and observes it after mutations, but
this is not transactional: a concurrent Kubernetes change can still occur
between validation, grant consumption, and the `kubectl` mutation. Stronger
server-side concurrency preconditions remain a production concern.

Durable audit storage, cryptographic grant signatures, distributed locking,
secret-management integration, and production rollout controls remain out of
scope for this showcase.
