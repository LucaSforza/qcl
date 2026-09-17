//! A local, deterministic `SafeOps` showcase built on QCL.

mod kernel;

pub use kernel::{
    Approval, AuditEvent, Decision, Denial, ExecutionGrant, ExecutionReceipt, Principal,
    SafeOpsError, SafetyKernel, Simulator, SystemState, Tool, ToolIntent, WorldState,
};

/// Returns the package name used by the showcase.
pub const PACKAGE_NAME: &str = "qcl-safeops";

/// Run the deterministic local scenario used by the `SafeOps` showcase.
///
/// The returned text is deliberately limited to policy decisions, state
/// transitions, and audit counts. It never includes agent reasoning.
///
/// # Errors
///
/// Returns an error if the embedded policy cannot be initialized or if the
/// scripted scenario does not produce its expected safety decisions.
pub fn run_demo() -> Result<String, SafeOpsError> {
    let mut kernel = SafetyKernel::new()?;
    let initial = WorldState::new(SystemState::Degraded, 0);
    let mut simulator = Simulator::new(initial);
    let mut output = vec![format!(
        "Initial state: {} (version {})",
        initial.state(),
        initial.version()
    )];

    run_restart(&mut kernel, &mut simulator, &mut output)?;
    run_deploy(&mut kernel, &mut simulator, &mut output)?;
    run_delete(&mut kernel, &simulator, &mut output)?;

    let proposals = kernel
        .audit()
        .iter()
        .filter(|event| matches!(event, AuditEvent::Proposal { .. }))
        .count();
    let allowed = kernel
        .audit()
        .iter()
        .filter(|event| matches!(event, AuditEvent::Decision { allowed: true, .. }))
        .count();
    let denied = kernel
        .audit()
        .iter()
        .filter(|event| matches!(event, AuditEvent::Decision { allowed: false, .. }))
        .count();
    let executions = kernel
        .audit()
        .iter()
        .filter(|event| matches!(event, AuditEvent::Execution { .. }))
        .count();
    let rejected_executions = kernel
        .audit()
        .iter()
        .filter(|event| matches!(event, AuditEvent::ExecutionRejected { .. }))
        .count();
    output.push(format!(
        "Audit summary: {proposals} proposals, {allowed} allowed, {denied} denied, {executions} executions, execution rejections: {rejected_executions}"
    ));

    Ok(output.join("\n"))
}

fn run_restart(
    kernel: &mut SafetyKernel,
    simulator: &mut Simulator,
    output: &mut Vec<String>,
) -> Result<(), SafeOpsError> {
    let snapshot = simulator.current();
    let intent = ToolIntent::new(
        Principal::OperatorLlm,
        Tool::RestartCanary,
        snapshot.version(),
    );
    let first = expect_grant(kernel.authorize(&intent, &snapshot, &[]), "restart_canary")?;
    output.push("restart_canary: ALLOW".to_owned());
    let second = expect_grant(kernel.authorize(&intent, &snapshot, &[]), "restart_canary")?;
    output.push("restart_canary: ALLOW (second grant from same snapshot)".to_owned());

    let receipt = kernel.execute(simulator, first)?;
    output.push(format!(
        "restart_canary: EXECUTED -> {} (version {})",
        receipt.to.state(),
        receipt.to.version()
    ));
    match kernel.execute(simulator, second) {
        Err(error @ SafeOpsError::StaleGrant { .. }) => {
            output.push(format!("restart_canary: DENY (stale grant) - {error}"));
            Ok(())
        }
        Ok(_) => Err(SafeOpsError::PolicyFormula(
            "demo expected the second restart grant to be stale".to_owned(),
        )),
        Err(error) => Err(error),
    }
}

fn run_deploy(
    kernel: &mut SafetyKernel,
    simulator: &mut Simulator,
    output: &mut Vec<String>,
) -> Result<(), SafeOpsError> {
    let snapshot = simulator.current();
    let intent = ToolIntent::new(
        Principal::OperatorLlm,
        Tool::DeployRelease,
        snapshot.version(),
    );
    match kernel.authorize(&intent, &snapshot, &[]) {
        Decision::Deny(denial) => output.push(format!("deploy_release: DENY ({denial})")),
        Decision::Allow(_) => {
            return Err(SafeOpsError::PolicyFormula(
                "demo expected deploy without human approval to be denied".to_owned(),
            ));
        }
    }
    let grant = expect_grant(
        kernel.authorize(&intent, &snapshot, &[Approval::human_operator()]),
        "deploy_release",
    )?;
    output.push("deploy_release: ALLOW".to_owned());
    let receipt = kernel.execute(simulator, grant)?;
    output.push(format!(
        "deploy_release: EXECUTED -> {} (version {})",
        receipt.to.state(),
        receipt.to.version()
    ));
    Ok(())
}

fn run_delete(
    kernel: &mut SafetyKernel,
    simulator: &Simulator,
    output: &mut Vec<String>,
) -> Result<(), SafeOpsError> {
    let snapshot = simulator.current();
    let intent = ToolIntent::new(
        Principal::OperatorLlm,
        Tool::DeleteResource,
        snapshot.version(),
    );
    match kernel.authorize(&intent, &snapshot, &[Approval::human_operator()]) {
        Decision::Deny(Denial::UnsafeOutcome { state, .. }) => {
            output.push(format!("delete_resource: DENY (unsafe outcome: {state})"));
            Ok(())
        }
        Decision::Deny(denial) => {
            output.push(format!("delete_resource: DENY ({denial})"));
            Ok(())
        }
        Decision::Allow(_) => Err(SafeOpsError::PolicyFormula(
            "demo expected deletion to be denied by the unsafe outcome check".to_owned(),
        )),
    }
}

fn expect_grant(decision: Decision, tool: &str) -> Result<ExecutionGrant, SafeOpsError> {
    match decision {
        Decision::Allow(grant) => Ok(grant),
        Decision::Deny(denial) => Err(SafeOpsError::PolicyFormula(format!(
            "demo expected {tool} to be allowed: {denial}"
        ))),
    }
}

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
    fn deploy_without_human_is_rejected_by_qcl() {
        let mut kernel = kernel();
        let intent = ToolIntent::new(Principal::OperatorLlm, Tool::DeployRelease, 0);
        let decision = kernel.authorize(&intent, &WorldState::new(SystemState::Degraded, 0), &[]);

        assert!(matches!(
            decision,
            Decision::Deny(Denial::PolicyRejected { .. })
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
        let replay = first.clone();

        let mut simulator = Simulator::new(snapshot);
        assert!(simulator.execute(first).is_ok());
        assert!(matches!(
            simulator.execute(replay),
            Err(SafeOpsError::ReplayedGrant { .. })
        ));
        assert!(matches!(
            simulator.execute(second),
            Err(SafeOpsError::StaleGrant { .. })
        ));
    }

    #[test]
    fn grant_is_bound_to_the_complete_world_snapshot() {
        let mut kernel = kernel();
        let authorized_snapshot = WorldState::new(SystemState::Degraded, 0);
        let intent = ToolIntent::new(Principal::OperatorLlm, Tool::RestartCanary, 0);
        let grant = match kernel.authorize(&intent, &authorized_snapshot, &[]) {
            Decision::Allow(grant) => grant,
            other @ Decision::Deny(_) => panic!("expected grant, got {other:?}"),
        };
        let mut different_world = Simulator::new(WorldState::new(SystemState::Stable, 0));

        assert!(matches!(
            kernel.execute(&mut different_world, grant),
            Err(SafeOpsError::StaleGrant { .. })
        ));
        assert!(matches!(
            kernel.audit().last(),
            Some(AuditEvent::ExecutionRejected { .. })
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

    #[test]
    fn demo_renders_the_complete_safeops_story() {
        let output = run_demo().expect("demo scenario should be deterministic");

        for expected in [
            "Initial state: degraded (version 0)",
            "restart_canary: ALLOW",
            "restart_canary: EXECUTED -> restarted (version 1)",
            "restart_canary: DENY (stale grant)",
            "deploy_release: DENY (QCL policy rejects deploy_release in state restarted)",
            "deploy_release: ALLOW",
            "deploy_release: EXECUTED -> deployed (version 2)",
            "delete_resource: DENY (unsafe outcome: deleted)",
            "Audit summary: 5 proposals, 3 allowed, 2 denied, 2 executions, execution rejections: 1",
        ] {
            assert!(
                output.contains(expected),
                "missing {expected:?} in:\n{output}"
            );
        }
    }
}
