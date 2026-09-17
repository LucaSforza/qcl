# SafeOps Showcase Contract

## Purpose

Demonstrate how an LLM-facing application can place a deterministic safety
kernel between proposed tool calls and execution. The showcase is educational:
it executes only against a local simulator and does not claim that an LLM,
cloud API, or credential boundary is secure.

## Distribution boundary

The repository is one Cargo workspace with separate packages:

- root package `qcl`: publishable model-checking library and REPL;
- `packages/qcl-safeops`: unpublished companion binary and library depending
  on `qcl` through a path dependency.

The root package is the only default installable product. Its Cargo package
must exclude `packages/`, so installing or downloading the packaged `qcl`
crate does not include the showcase. A Git checkout still contains the whole
workspace.

## Trust boundary

Tool intents are untrusted input. The safety kernel, QCL policy, tool registry,
world-state snapshot, approval set, grant verifier, and executor are trusted
for the purpose of the demonstration. No secret or ambient system capability
is available to a simulated agent.

The showcase uses these stages:

1. accept a typed `ToolIntent` naming an actor, tool, and expected world-state
   version;
2. resolve the active coalition from the intent, executor, and approvals;
3. ask QCL whether that exact coalition can enforce the tool's target formula
   in the current abstract state;
4. enumerate the tool contract's possible outcome states and require every one
   to satisfy the configured safety invariant;
5. emit either a structured denial or an execution grant bound to the exact
   intent and world-state version;
6. let the simulator execute only a matching, current grant;
7. append the proposal, decision, and execution result to an audit trail.

QCL answers coalition capability. The separate outcome-subset check answers
robust action safety. Ability to enforce one safe result is never treated as
proof that every possible result is safe.

## Demonstration domain

Agents:

- `operator_llm` proposes operational actions;
- `human_operator` approves production changes;
- `executor` is the only execution principal.

Abstract states include degraded service, restarted canary, modified
production, and deleted data. Proposition `safe` is the invariant. Tool
contracts cover:

- log inspection, which is read-only;
- canary restart, executable by `operator_llm` and `executor`;
- production deployment, executable only with `human_operator` added;
- resource deletion, denied because at least one declared outcome violates
  `safe`, even when its coalition is otherwise authorized.

The scripted scenario must also retain a grant, advance the simulator version,
and prove that the stale grant is rejected.

## Public surface

The companion library exposes narrow domain types for intents, decisions,
denials, grants, audit events, and world state. Callers cannot construct a
valid grant without the safety kernel. Execution consumes or otherwise marks a
grant so it cannot be replayed.

Errors are structured and displayed with stable human-readable explanations.
The command-line binary prints each proposal, policy result, and state change
without exposing chain-of-thought.

## Explicit non-goals

- calling a real LLM or external tool provider;
- handling credentials or secrets;
- cryptographic signatures or remote attestation;
- durable audit storage;
- concurrent execution or distributed state synchronization;
- treating QCL ability semantics as action-transition semantics;
- adding SafeOps concepts to the `qcl` public API.

These features require separate threat models and become future work only when
a real integration needs them.
