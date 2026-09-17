//! A local, deterministic `SafeOps` showcase built on QCL.

mod kernel;

pub use kernel::{
    Approval, AuditEvent, Decision, Denial, ExecutionGrant, ExecutionReceipt, Principal,
    SafeOpsError, SafetyKernel, Simulator, SystemState, Tool, ToolIntent, WorldState,
};

/// Returns the package name used by the showcase.
pub const PACKAGE_NAME: &str = "qcl-safeops";

#[cfg(test)]
mod tests {
    use super::*;

    fn kernel() -> SafetyKernel {
        SafetyKernel::new().expect("embedded policy is valid")
    }

    #[test]
    fn restart_is_allowed_without_human_approval() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(Principal::OperatorLlm, Tool::RestartCanary, 0);
        let decision = kernel.authorize(&intent, &WorldState::new(SystemState::Degraded, 0), &[]);

        assert!(matches!(decision, Decision::Allow(_)));
    }

    #[test]
    fn deploy_requires_human_coalition_member() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(Principal::OperatorLlm, Tool::DeployRelease, 0);
        let decision = kernel.authorize(&intent, &WorldState::new(SystemState::Degraded, 0), &[]);

        assert!(matches!(
            decision,
            Decision::Deny(Denial::MissingApproval { .. })
        ));
    }

    #[test]
    fn deploy_with_human_approval_is_allowed() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(Principal::OperatorLlm, Tool::DeployRelease, 0);
        let decision = kernel.authorize(
            &intent,
            &WorldState::new(SystemState::Degraded, 0),
            &[Approval::human_operator()],
        );

        assert!(matches!(decision, Decision::Allow(_)));
    }

    #[test]
    fn deletion_is_denied_even_with_human_approval_when_an_outcome_is_unsafe() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(Principal::OperatorLlm, Tool::DeleteResource, 0);
        let decision = kernel.authorize(
            &intent,
            &WorldState::new(SystemState::Degraded, 0),
            &[Approval::human_operator()],
        );

        assert!(matches!(
            decision,
            Decision::Deny(Denial::UnsafeOutcome { .. })
        ));
    }

    #[test]
    fn grants_are_bound_to_versions_and_cannot_replay_after_execution() {
        let mut kernel = kernel();
        let snapshot = WorldState::new(SystemState::Degraded, 0);
        let intent = ToolIntent::new(Principal::OperatorLlm, Tool::RestartCanary, 0);
        let first = match kernel.authorize(&intent, &snapshot, &[]) {
            Decision::Allow(grant) => grant,
            other @ Decision::Deny(_) => panic!("expected first grant, got {other:?}"),
        };
        let second = match kernel.authorize(&intent, &snapshot, &[]) {
            Decision::Allow(grant) => grant,
            other @ Decision::Deny(_) => panic!("expected second grant, got {other:?}"),
        };

        let mut simulator = Simulator::new(snapshot);
        assert!(simulator.execute(first).is_ok());
        assert!(matches!(
            simulator.execute(second),
            Err(SafeOpsError::StaleGrant { .. })
        ));
    }

    #[test]
    fn audit_contains_decisions_but_never_reasoning_text() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(Principal::OperatorLlm, Tool::RestartCanary, 0);
        let _ = kernel.authorize(&intent, &WorldState::new(SystemState::Degraded, 0), &[]);

        assert!(matches!(kernel.audit()[0], AuditEvent::Proposal { .. }));
        assert!(matches!(kernel.audit()[1], AuditEvent::Decision { .. }));
    }
}
