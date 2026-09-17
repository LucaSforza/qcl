//! Real-cluster plus real-provider end-to-end test. Ignored by default.

use safeops_k8s::{
    Action, Decision, KubectlAdapter, Principal, SafetyKernel, ToolIntent, provider_from_env,
};

#[test]
#[ignore = "requires kind-safeops-qcl plus authenticated Codex or DeepSeek"]
fn live_e2e_llm_proposal_is_kernel_checked_before_real_inspect() {
    let provider = provider_from_env().expect(
        "set SAFEOPS_LLM_PROVIDER=codex with codex login, or deepseek with DEEPSEEK_API_KEY",
    );
    let adapter = KubectlAdapter::new();
    let snapshot = adapter.snapshot().expect("read real deployment snapshot");
    let action = provider
        .propose_action(
            &snapshot,
            "For stability observation only, choose exactly inspect. Do not mutate or delete anything.",
        )
        .expect("real provider must return a typed action");
    assert_eq!(
        action,
        Action::Inspect,
        "provider proposed a non-observational action"
    );

    let mut kernel = SafetyKernel::new().expect("embedded policy is valid");
    let intent = ToolIntent::new(Principal::OperatorLlm, action, snapshot);
    let grant = match kernel.authorize(&intent, &[]) {
        Decision::Allow(grant) => grant,
        Decision::Deny(denial) => panic!("QCL rejected real inspect proposal: {denial}"),
    };
    let result = adapter
        .execute(&mut kernel, grant)
        .expect("real inspect must execute through the adapter");
    assert!(matches!(result, safeops_k8s::ExecutionResult::Observed(_)));
}
