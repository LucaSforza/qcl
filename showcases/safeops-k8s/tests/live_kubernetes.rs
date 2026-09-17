//! Real-cluster adapter tests. They are intentionally ignored by default.

use std::process::Command;
use std::thread;
use std::time::Duration;

use safeops_k8s::{
    Action, AdapterError, Approval, Decision, DeploymentSnapshot, KubectlAdapter, Principal,
    RolloutStrategy, SafeOpsError, SafetyKernel, ToolIntent,
};

const CONTEXT: &str = "kind-safeops-qcl";
const NAMESPACE: &str = "safeops-demo";
const IMPERSONATED_EXECUTOR: &str = "system:serviceaccount:safeops-demo:safeops-executor";

#[test]
#[ignore = "requires the real kind-safeops-qcl cluster and applied fixtures"]
fn live_kubernetes_snapshot_rbac_restart_and_stale_grant() {
    let adapter = KubectlAdapter::new();
    let initial = adapter.snapshot().expect("read real deployment snapshot");
    assert_eq!(initial.namespace(), NAMESPACE);
    assert_eq!(initial.name(), "safeops-demo");
    assert!(initial.desired_replicas() >= 2);

    let mut kernel = SafetyKernel::new().expect("embedded policy is valid");
    let inspect_intent = ToolIntent::new(Principal::OperatorLlm, Action::Inspect, initial.clone());
    let stale_grant = match kernel.authorize(&inspect_intent, &[]) {
        Decision::Allow(grant) => grant,
        Decision::Deny(denial) => panic!("inspect should be authorized: {denial}"),
    };

    let restart_intent = ToolIntent::new(
        Principal::OperatorLlm,
        Action::RestartRollout {
            strategy: RolloutStrategy::Rolling,
        },
        initial.clone(),
    );
    let restart_grant = match kernel.authorize(&restart_intent, &[]) {
        Decision::Allow(grant) => grant,
        Decision::Deny(denial) => panic!("restart should be authorized: {denial}"),
    };
    let restarted = adapter
        .execute(&mut kernel, restart_grant)
        .expect("restart through executor service account");
    let after_restart = adapter.snapshot().expect("read postcondition snapshot");
    assert_ne!(after_restart.resource_version(), initial.resource_version());
    assert!(matches!(
        restarted,
        safeops_k8s::ExecutionResult::Mutated { .. }
    ));
    assert!(matches!(
        adapter.execute(&mut kernel, stale_grant),
        Err(AdapterError::Kernel(SafeOpsError::StaleGrant { .. }))
    ));

    let scale_up_snapshot = stable_snapshot(&adapter);
    let scale_up_intent = ToolIntent::new(
        Principal::OperatorLlm,
        Action::Scale { replicas: 3 },
        scale_up_snapshot.clone(),
    );
    let scale_up_grant = match kernel.authorize(&scale_up_intent, &[]) {
        Decision::Allow(grant) => grant,
        Decision::Deny(denial) => panic!("scale to three should be authorized: {denial}"),
    };
    adapter
        .execute(&mut kernel, scale_up_grant)
        .expect("scale subresource patch must be permitted");
    assert_eq!(
        adapter
            .snapshot()
            .expect("read scaled snapshot")
            .desired_replicas(),
        3
    );

    let scale_down_snapshot = stable_snapshot(&adapter);
    let scale_down_intent = ToolIntent::new(
        Principal::OperatorLlm,
        Action::Scale { replicas: 2 },
        scale_down_snapshot,
    );
    let scale_down_grant = match kernel.authorize(&scale_down_intent, &[]) {
        Decision::Allow(grant) => grant,
        Decision::Deny(denial) => panic!("scale back to two should be authorized: {denial}"),
    };
    adapter
        .execute(&mut kernel, scale_down_grant)
        .expect("scale back to the invariant floor");
    assert_eq!(
        adapter
            .snapshot()
            .expect("read restored snapshot")
            .desired_replicas(),
        2
    );

    let can_delete = Command::new("kubectl")
        .args([
            "--context",
            CONTEXT,
            "--namespace",
            NAMESPACE,
            &format!("--as={IMPERSONATED_EXECUTOR}"),
            "auth",
            "can-i",
            "delete",
            "namespaces",
        ])
        .output()
        .expect("kubectl auth can-i");
    assert_eq!(String::from_utf8_lossy(&can_delete.stdout).trim(), "no");
    // `kubectl auth can-i` uses exit status 1 for a valid, denied answer.
    assert!(!can_delete.status.success());
}

fn stable_snapshot(adapter: &KubectlAdapter) -> DeploymentSnapshot {
    let mut previous = adapter.snapshot().expect("read stable-snapshot candidate");
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(100));
        let current = adapter.snapshot().expect("read stable-snapshot candidate");
        if current.resource_version() == previous.resource_version()
            && current.ready_replicas() >= current.desired_replicas()
        {
            return current;
        }
        previous = current;
    }
    panic!("deployment resourceVersion did not stabilize before grant");
}

#[test]
#[ignore = "requires the real kind-safeops-qcl cluster and applied fixtures"]
fn live_kubernetes_delete_grant_is_defensively_unexecutable() {
    let adapter = KubectlAdapter::new();
    let snapshot: DeploymentSnapshot = adapter.snapshot().expect("read real deployment snapshot");
    let mut kernel = SafetyKernel::new().expect("embedded policy is valid");
    let intent = ToolIntent::new(Principal::OperatorLlm, Action::DeleteNamespace, snapshot);
    let decision = kernel.authorize(&intent, &[Approval::human_operator()]);
    assert!(matches!(decision, Decision::Deny(_)));
}
