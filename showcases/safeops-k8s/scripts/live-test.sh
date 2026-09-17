#!/usr/bin/env bash
set -eu

CONTEXT="kind-safeops-qcl"
NAMESPACE="safeops-demo"
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
K8S_DIR="$(CDPATH= cd -- "${SCRIPT_DIR}/../k8s" && pwd)"
POLICY_MESSAGE="SafeOps workloads must run at least two replicas"
POLICY_TIMEOUT_SECONDS=60

fail() {
    echo "live-test: $*" >&2
    exit 1
}

validate_target() {
    current_context="$(kubectl config current-context)"
    [ "${current_context}" = "${CONTEXT}" ] || fail "current context is ${current_context}, expected ${CONTEXT}"
    [ "${CONTEXT}" = "kind-safeops-qcl" ] || fail "context allowlist check failed"
    [ "${NAMESPACE}" = "safeops-demo" ] || fail "namespace allowlist check failed"
    kubectl --context "${CONTEXT}" get --raw=/version >/dev/null
}

apply_fixture() {
    kubectl --context "${CONTEXT}" apply --filename "${K8S_DIR}/namespace.yaml"
    kubectl --context "${CONTEXT}" wait --for=jsonpath='{.status.phase}'=Active \
        --timeout=60s "namespace/${NAMESPACE}"
    kubectl --context "${CONTEXT}" apply --filename "${K8S_DIR}/admission-policy.yaml"
    kubectl --context "${CONTEXT}" apply --filename "${K8S_DIR}/rbac.yaml"
    kubectl --context "${CONTEXT}" apply --filename "${K8S_DIR}/demo.yaml"
    kubectl --context "${CONTEXT}" rollout status \
        --namespace "${NAMESPACE}" deployment/safeops-demo --timeout=120s
}

assert_scale_is_denied() {
    deadline=$(( $(date +%s) + POLICY_TIMEOUT_SECONDS ))
    while [ "$(date +%s)" -lt "${deadline}" ]; do
        if scale_output="$(kubectl --context "${CONTEXT}" --namespace "${NAMESPACE}" \
            scale deployment/safeops-demo --replicas=1 2>&1)"; then
            kubectl --context "${CONTEXT}" --namespace "${NAMESPACE}" \
                scale deployment/safeops-demo --replicas=2 >/dev/null
            sleep 1
            continue
        fi

        case "${scale_output}" in
            *"${POLICY_MESSAGE}"*)
                replicas="$(kubectl --context "${CONTEXT}" --namespace "${NAMESPACE}" \
                    get deployment/safeops-demo -o jsonpath='{.spec.replicas}')"
                [ "${replicas}" = "2" ] || fail "policy denied scale but replicas are ${replicas}"
                echo "live-test: scale to 1 denied by ValidatingAdmissionPolicy; replicas remain 2"
                return 0
                ;;
            *)
                echo "${scale_output}" >&2
                fail "scale failed without the expected admission-policy denial"
                ;;
        esac
    done
    fail "ValidatingAdmissionPolicy did not deny scale to 1 within ${POLICY_TIMEOUT_SECONDS}s"
}

validate_target
apply_fixture
assert_scale_is_denied
