//! A side-effect-free Kubernetes `SafeOps` policy gateway built on QCL.

mod kernel;
mod kubernetes;
mod llm;

pub use kernel::{
    Action, Approval, AuditEvent, Decision, Denial, DeploymentSnapshot, ExecutionGrant,
    ExecutionPermit, Principal, RolloutStrategy, SafeOpsError, SafetyKernel, SnapshotError,
    ToolIntent,
};
pub use kubernetes::{AdapterError, ExecutionResult, KubectlAdapter};
pub use llm::{ActionProvider, CodexProvider, DeepSeekProvider, LlmError, provider_from_env};

/// Returns the package name used by the showcase.
pub const PACKAGE_NAME: &str = "safeops-k8s";

/// Run a policy-only scenario against a Deployment snapshot.
///
/// No Kubernetes API is called and no rollout is claimed to have happened.
///
/// # Errors
///
/// Returns an error if the embedded policy or demo snapshot is invalid.
pub fn run_demo() -> Result<String, SafeOpsError> {
    let snapshot = DeploymentSnapshot::try_new(
        "payments",
        "api",
        "42",
        3,
        3,
        "registry.example/payments:2.4.1",
    )?;
    let mut kernel = SafetyKernel::new()?;
    let mut lines = vec![format!("Deployment: {snapshot}")];
    let cases = [
        (
            "inspect",
            ToolIntent::new(Principal::OperatorLlm, Action::Inspect, snapshot.clone()),
            &[][..],
        ),
        (
            "restart",
            ToolIntent::new(
                Principal::OperatorLlm,
                Action::RestartRollout {
                    strategy: RolloutStrategy::Rolling,
                },
                snapshot.clone(),
            ),
            &[][..],
        ),
        (
            "update_image_without_human",
            ToolIntent::new(
                Principal::OperatorLlm,
                Action::UpdateImage {
                    image: "registry.example/payments:2.4.2".to_owned(),
                },
                snapshot.clone(),
            ),
            &[][..],
        ),
        (
            "delete_namespace_with_human",
            ToolIntent::new(
                Principal::OperatorLlm,
                Action::DeleteNamespace,
                snapshot.clone(),
            ),
            &[Approval::human_operator()][..],
        ),
    ];
    for (label, intent, approvals) in cases {
        lines.push(format_decision(label, kernel.authorize(&intent, approvals)));
    }
    lines.push(format!("Audit events: {}", kernel.audit().len()));
    Ok(lines.join("\n"))
}

fn format_decision(label: &str, decision: Decision) -> String {
    match decision {
        Decision::Allow(_) => format!("{label}: ALLOW (grant ready for adapter)"),
        Decision::Deny(denial) => format!("{label}: DENY ({denial})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> DeploymentSnapshot {
        DeploymentSnapshot::try_new(
            "payments",
            "api",
            "42",
            3,
            3,
            "registry.example/payments:2.4.1",
        )
        .expect("valid deployment snapshot")
    }

    fn kernel() -> SafetyKernel {
        SafetyKernel::new().expect("embedded policy is valid")
    }

    #[test]
    fn snapshot_rejects_empty_identity_and_under_replicated_configuration() {
        assert!(matches!(
            DeploymentSnapshot::try_new("", "api", "42", 3, 3, "image"),
            Err(SnapshotError::EmptyNamespace)
        ));
        assert!(matches!(
            DeploymentSnapshot::try_new("payments", "api", "42", 1, 1, "image"),
            Err(SnapshotError::DesiredReplicasTooLow { minimum: 2, .. })
        ));
    }

    #[test]
    fn inspect_restart_rollback_and_scale_are_allowed() {
        let mut kernel = kernel();
        let current = snapshot();
        for action in [
            Action::Inspect,
            Action::RestartRollout {
                strategy: RolloutStrategy::Rolling,
            },
            Action::RollbackRollout,
            Action::Scale { replicas: 4 },
        ] {
            let intent = ToolIntent::new(Principal::OperatorLlm, action, current.clone());
            assert!(matches!(kernel.authorize(&intent, &[]), Decision::Allow(_)));
        }
    }

    #[test]
    fn restart_requires_ready_snapshot_and_rolling_strategy() {
        let mut kernel = kernel();
        let unready = DeploymentSnapshot::try_new(
            "payments",
            "api",
            "42",
            3,
            2,
            "registry.example/payments:2.4.1",
        )
        .expect("valid but unready snapshot");
        let intent = ToolIntent::new(
            Principal::OperatorLlm,
            Action::RestartRollout {
                strategy: RolloutStrategy::Rolling,
            },
            unready,
        );
        assert!(matches!(
            kernel.authorize(&intent, &[]),
            Decision::Deny(Denial::Precondition { .. })
        ));

        let intent = ToolIntent::new(
            Principal::OperatorLlm,
            Action::RestartRollout {
                strategy: RolloutStrategy::Recreate,
            },
            snapshot(),
        );
        assert!(matches!(
            kernel.authorize(&intent, &[]),
            Decision::Deny(Denial::Precondition { .. })
        ));
    }

    #[test]
    fn image_mutation_requires_human_approval() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(
            Principal::OperatorLlm,
            Action::UpdateImage {
                image: "registry.example/payments:2.4.2".to_owned(),
            },
            snapshot(),
        );
        assert!(matches!(
            kernel.authorize(&intent, &[]),
            Decision::Deny(Denial::MissingApproval { .. })
        ));
        assert!(matches!(
            kernel.authorize(&intent, &[Approval::human_operator()]),
            Decision::Allow(_)
        ));
    }

    #[test]
    fn delete_namespace_is_unsafe_even_with_human_approval() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(Principal::OperatorLlm, Action::DeleteNamespace, snapshot());
        assert!(matches!(
            kernel.authorize(&intent, &[Approval::human_operator()]),
            Decision::Deny(Denial::UnsafeOutcome { .. })
        ));
    }

    #[test]
    fn grant_is_bound_to_complete_snapshot_and_consumed_once() {
        let mut kernel = kernel();
        let current = snapshot();
        let intent = ToolIntent::new(Principal::OperatorLlm, Action::Inspect, current.clone());
        let grant = match kernel.authorize(&intent, &[]) {
            Decision::Allow(grant) => grant,
            Decision::Deny(denial) => panic!("expected grant, got {denial}"),
        };
        let replay = grant.clone();
        assert!(kernel.consume_grant(grant, &current).is_ok());
        assert!(matches!(
            kernel.consume_grant(replay, &current),
            Err(SafeOpsError::ReplayedGrant { .. })
        ));

        let other = DeploymentSnapshot::try_new(
            "payments",
            "api",
            "43",
            3,
            3,
            "registry.example/payments:2.4.1",
        )
        .expect("valid current snapshot");
        let stale = match kernel.authorize(
            &ToolIntent::new(Principal::OperatorLlm, Action::Inspect, other),
            &[],
        ) {
            Decision::Allow(grant) => grant,
            Decision::Deny(denial) => panic!("expected grant, got {denial}"),
        };
        assert!(matches!(
            kernel.consume_grant(stale, &current),
            Err(SafeOpsError::StaleGrant { .. })
        ));
    }

    #[test]
    fn audit_records_policy_events_without_reasoning_text() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(Principal::OperatorLlm, Action::Inspect, snapshot());
        let _ = kernel.authorize(&intent, &[]);
        assert!(matches!(kernel.audit()[0], AuditEvent::Proposal { .. }));
        assert!(matches!(kernel.audit()[1], AuditEvent::Decision { .. }));
        assert!(!format!("{:?}", kernel.audit()).contains("reasoning"));
    }

    #[test]
    fn demo_is_decision_only() {
        let output = run_demo().expect("demo should be deterministic");
        assert!(output.contains("grant ready for adapter"));
        assert!(output.contains("delete_namespace_with_human: DENY"));
        assert!(!output.contains("EXECUTED"));
    }
}
