# qcl-safeops

`qcl-safeops` is an optional, unpublished local showcase of a deterministic
safety gateway built on the published `qcl` package. It demonstrates typed tool
intents, QCL coalition checks, outcome safety checks, version-bound grants, and
an audit trail against a local simulator.

Run it from the repository root:

```bash
cargo run -p qcl-safeops
```

The scenario shows a canary restart that is allowed, rejection of a stale grant,
deployment denied without a human approval and then executed with one, and
resource deletion denied because its declared outcome is unsafe.

This package does not call a real LLM or external tool provider. It has no
credential or secret boundary, cryptographic authorization, durable audit log,
or distributed/concurrent execution. The published `qcl` package does not
contain this showcase.
