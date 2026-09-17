use std::collections::HashSet;
use std::fmt;

use qcl::ast::{CoalitionPredicate, Formula};
use qcl::checker::ModelChecker;
use qcl::domain::{Coalition, StateId};
use qcl::model::{ModelValidator, QclModel};
use qcl::parser::{parse_formula, parse_model};

const POLICY: &str = r"
model {
  agents { operator_llm, human_operator, executor };
  states { ready, unready, image_changed, deleted };
  props { safe, image_change, data_deleted };
  valuation {
    ready: { safe };
    unready: { safe };
    image_changed: { safe, image_change };
    deleted: { data_deleted };
  };
  effectivity {
    ready, {} -> { ready, unready, image_changed, deleted };
    ready, { operator_llm } -> { ready, unready, image_changed, deleted };
    ready, { human_operator } -> { ready, unready, image_changed, deleted };
    ready, { executor } -> { ready, unready, image_changed, deleted };
    ready, { operator_llm, human_operator } -> { ready, unready, image_changed, deleted };
    ready, { operator_llm, executor } -> { ready, unready, image_changed, deleted };
    ready, { human_operator, executor } -> { ready, unready, image_changed, deleted };
    ready, { operator_llm, executor } -> { ready };
    ready, { operator_llm, human_operator, executor } -> { ready };
    ready, { operator_llm, human_operator, executor } -> { unready };
    ready, { operator_llm, human_operator, executor } -> { image_changed };
    ready, { operator_llm, human_operator, executor } -> { deleted };

    unready, {} -> { ready, unready, image_changed, deleted };
    unready, { operator_llm } -> { ready, unready, image_changed, deleted };
    unready, { human_operator } -> { ready, unready, image_changed, deleted };
    unready, { executor } -> { ready, unready, image_changed, deleted };
    unready, { operator_llm, human_operator } -> { ready, unready, image_changed, deleted };
    unready, { operator_llm, executor } -> { ready, unready, image_changed, deleted };
    unready, { human_operator, executor } -> { ready, unready, image_changed, deleted };
    unready, { operator_llm, executor } -> { ready };
    unready, { operator_llm, human_operator, executor } -> { ready };
    unready, { operator_llm, human_operator, executor } -> { unready };
    unready, { operator_llm, human_operator, executor } -> { image_changed };
    unready, { operator_llm, human_operator, executor } -> { deleted };

    image_changed, {} -> { ready, unready, image_changed, deleted };
    image_changed, { operator_llm } -> { ready, unready, image_changed, deleted };
    image_changed, { human_operator } -> { ready, unready, image_changed, deleted };
    image_changed, { executor } -> { ready, unready, image_changed, deleted };
    image_changed, { operator_llm, human_operator } -> { ready, unready, image_changed, deleted };
    image_changed, { operator_llm, executor } -> { ready, unready, image_changed, deleted };
    image_changed, { human_operator, executor } -> { ready, unready, image_changed, deleted };
    image_changed, { operator_llm, executor } -> { ready };
    image_changed, { operator_llm, human_operator, executor } -> { ready };
    image_changed, { operator_llm, human_operator, executor } -> { unready };
    image_changed, { operator_llm, human_operator, executor } -> { image_changed };
    image_changed, { operator_llm, human_operator, executor } -> { deleted };

    deleted, {} -> { ready, unready, image_changed, deleted };
    deleted, { operator_llm } -> { ready, unready, image_changed, deleted };
    deleted, { human_operator } -> { ready, unready, image_changed, deleted };
    deleted, { executor } -> { ready, unready, image_changed, deleted };
    deleted, { operator_llm, human_operator } -> { ready, unready, image_changed, deleted };
    deleted, { operator_llm, executor } -> { ready, unready, image_changed, deleted };
    deleted, { human_operator, executor } -> { ready, unready, image_changed, deleted };
    deleted, { operator_llm, executor } -> { ready };
    deleted, { operator_llm, human_operator, executor } -> { ready };
    deleted, { operator_llm, human_operator, executor } -> { unready };
    deleted, { operator_llm, human_operator, executor } -> { image_changed };
    deleted, { operator_llm, human_operator, executor } -> { deleted };
  };
}
";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Principal {
    OperatorLlm,
    HumanOperator,
    Executor,
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OperatorLlm => "operator_llm",
            Self::HumanOperator => "human_operator",
            Self::Executor => "executor",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RolloutStrategy {
    Rolling,
    Recreate,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Action {
    Inspect,
    RestartRollout { strategy: RolloutStrategy },
    RollbackRollout,
    Scale { replicas: u32 },
    UpdateImage { image: String },
    DeleteNamespace,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inspect => f.write_str("inspect"),
            Self::RestartRollout { .. } => f.write_str("restart_rollout"),
            Self::RollbackRollout => f.write_str("rollback_rollout"),
            Self::Scale { replicas } => write!(f, "scale({replicas})"),
            Self::UpdateImage { .. } => f.write_str("update_image"),
            Self::DeleteNamespace => f.write_str("delete_namespace"),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DeploymentSnapshot {
    namespace: String,
    name: String,
    resource_version: String,
    desired_replicas: u32,
    ready_replicas: u32,
    image: String,
}

impl DeploymentSnapshot {
    /// Validate and capture the fields read from one Kubernetes Deployment.
    ///
    /// # Errors
    ///
    /// Returns a validation error for empty identity fields, an empty image,
    /// fewer than two desired replicas, or impossible readiness counts.
    pub fn try_new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        resource_version: impl Into<String>,
        desired_replicas: u32,
        ready_replicas: u32,
        image: impl Into<String>,
    ) -> Result<Self, SnapshotError> {
        let snapshot = Self {
            namespace: namespace.into(),
            name: name.into(),
            resource_version: resource_version.into(),
            desired_replicas,
            ready_replicas,
            image: image.into(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn validate(&self) -> Result<(), SnapshotError> {
        if self.namespace.is_empty() {
            return Err(SnapshotError::EmptyNamespace);
        }
        if self.name.is_empty() {
            return Err(SnapshotError::EmptyName);
        }
        if self.resource_version.is_empty() {
            return Err(SnapshotError::EmptyResourceVersion);
        }
        if self.image.is_empty() {
            return Err(SnapshotError::EmptyImage);
        }
        if self.desired_replicas < 2 {
            return Err(SnapshotError::DesiredReplicasTooLow {
                actual: self.desired_replicas,
                minimum: 2,
            });
        }
        if self.ready_replicas > self.desired_replicas {
            return Err(SnapshotError::ReadyReplicasExceedDesired {
                ready: self.ready_replicas,
                desired: self.desired_replicas,
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn resource_version(&self) -> &str {
        &self.resource_version
    }
    #[must_use]
    pub const fn desired_replicas(&self) -> u32 {
        self.desired_replicas
    }
    #[must_use]
    pub const fn ready_replicas(&self) -> u32 {
        self.ready_replicas
    }
    #[must_use]
    pub fn image(&self) -> &str {
        &self.image
    }
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.ready_replicas == self.desired_replicas
    }
}

impl fmt::Display for DeploymentSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}@{}",
            self.namespace, self.name, self.resource_version
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    EmptyNamespace,
    EmptyName,
    EmptyResourceVersion,
    EmptyImage,
    DesiredReplicasTooLow { actual: u32, minimum: u32 },
    ReadyReplicasExceedDesired { ready: u32, desired: u32 },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyNamespace => f.write_str("namespace is empty"),
            Self::EmptyName => f.write_str("deployment name is empty"),
            Self::EmptyResourceVersion => f.write_str("resourceVersion is empty"),
            Self::EmptyImage => f.write_str("image is empty"),
            Self::DesiredReplicasTooLow { actual, minimum } => {
                write!(f, "desired replicas {actual} is below minimum {minimum}")
            }
            Self::ReadyReplicasExceedDesired { ready, desired } => {
                write!(
                    f,
                    "ready replicas {ready} exceeds desired replicas {desired}"
                )
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolIntent {
    actor: Principal,
    action: Action,
    snapshot: DeploymentSnapshot,
}

impl ToolIntent {
    #[must_use]
    pub fn new(actor: Principal, action: Action, snapshot: DeploymentSnapshot) -> Self {
        Self {
            actor,
            action,
            snapshot,
        }
    }
    #[must_use]
    pub const fn actor(&self) -> Principal {
        self.actor
    }
    #[must_use]
    pub fn action(&self) -> &Action {
        &self.action
    }
    #[must_use]
    pub fn snapshot(&self) -> &DeploymentSnapshot {
        &self.snapshot
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Approval {
    principal: Principal,
}

impl Approval {
    #[must_use]
    pub const fn human_operator() -> Self {
        Self {
            principal: Principal::HumanOperator,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Denial {
    UnauthorizedActor { actor: Principal },
    MissingApproval { required: Principal },
    Precondition { action: Action, reason: String },
    PolicyRejected { action: Action },
    PolicyFailure { action: Action, message: String },
    UnsafeOutcome { action: Action },
}

impl fmt::Display for Denial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnauthorizedActor { actor } => write!(f, "unauthorized actor {actor}"),
            Self::MissingApproval { required } => write!(f, "missing approval from {required}"),
            Self::Precondition { action, reason } => {
                write!(f, "{action} precondition failed: {reason}")
            }
            Self::PolicyRejected { action } => write!(f, "QCL policy rejects {action}"),
            Self::PolicyFailure { action, message } => {
                write!(f, "QCL policy check failed for {action}: {message}")
            }
            Self::UnsafeOutcome { action } => write!(f, "{action} has an unsafe possible outcome"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Allow(ExecutionGrant),
    Deny(Denial),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionGrant {
    intent: ToolIntent,
    grant_id: u64,
}

impl ExecutionGrant {
    #[must_use]
    pub fn intent(&self) -> &ToolIntent {
        &self.intent
    }
    #[must_use]
    pub fn snapshot(&self) -> &DeploymentSnapshot {
        self.intent.snapshot()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPermit {
    intent: ToolIntent,
}

impl ExecutionPermit {
    #[must_use]
    pub fn action(&self) -> &Action {
        self.intent.action()
    }
    #[must_use]
    pub fn snapshot(&self) -> &DeploymentSnapshot {
        self.intent.snapshot()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditEvent {
    Proposal {
        intent: ToolIntent,
    },
    Decision {
        allowed: bool,
        denial: Option<Denial>,
    },
    GrantConsumed {
        action: Action,
        snapshot: DeploymentSnapshot,
    },
    GrantRejected {
        action: Action,
        snapshot: DeploymentSnapshot,
        error: SafeOpsError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SafeOpsError {
    Snapshot(SnapshotError),
    PolicyParse(String),
    PolicyInvalid(String),
    PolicyFormula(String),
    StaleGrant {
        expected: Box<DeploymentSnapshot>,
        actual: Box<DeploymentSnapshot>,
    },
    ReplayedGrant {
        grant_id: u64,
    },
}

impl fmt::Display for SafeOpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot(error) => write!(f, "invalid snapshot: {error}"),
            Self::PolicyParse(error) => write!(f, "policy parse failed: {error}"),
            Self::PolicyInvalid(error) => write!(f, "policy validation failed: {error}"),
            Self::PolicyFormula(error) => write!(f, "policy formula failed: {error}"),
            Self::StaleGrant { expected, actual } => {
                write!(f, "grant for {expected} is stale at {actual}")
            }
            Self::ReplayedGrant { grant_id } => write!(f, "grant {grant_id} was already used"),
        }
    }
}

impl std::error::Error for SafeOpsError {}

impl From<SnapshotError> for SafeOpsError {
    fn from(error: SnapshotError) -> Self {
        Self::Snapshot(error)
    }
}

pub struct SafetyKernel {
    model: QclModel,
    next_grant_id: u64,
    used_grants: HashSet<u64>,
    audit: Vec<AuditEvent>,
}

impl SafetyKernel {
    /// Parse and validate the embedded coalition policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded model cannot be parsed or validated.
    pub fn new() -> Result<Self, SafeOpsError> {
        let model =
            parse_model(POLICY).map_err(|error| SafeOpsError::PolicyParse(error.to_string()))?;
        ModelValidator::validate(&model)
            .map_err(|error| SafeOpsError::PolicyInvalid(error.to_string()))?;
        Ok(Self {
            model,
            next_grant_id: 0,
            used_grants: HashSet::new(),
            audit: Vec::new(),
        })
    }

    #[must_use]
    pub fn audit(&self) -> &[AuditEvent] {
        &self.audit
    }

    pub fn authorize(&mut self, intent: &ToolIntent, approvals: &[Approval]) -> Decision {
        self.audit.push(AuditEvent::Proposal {
            intent: intent.clone(),
        });
        let decision = self.authorize_inner(intent, approvals);
        self.audit.push(AuditEvent::Decision {
            allowed: matches!(decision, Decision::Allow(_)),
            denial: match &decision {
                Decision::Deny(denial) => Some(denial.clone()),
                Decision::Allow(_) => None,
            },
        });
        decision
    }

    /// Consume a grant into a side-effect-free permit for a future Kubernetes adapter.
    ///
    /// # Errors
    ///
    /// Returns an error if the grant is stale or has already been consumed.
    pub fn consume_grant(
        &mut self,
        grant: ExecutionGrant,
        current: &DeploymentSnapshot,
    ) -> Result<ExecutionPermit, SafeOpsError> {
        let action = grant.intent.action.clone();
        let expected = grant.intent.snapshot.clone();
        if self.used_grants.contains(&grant.grant_id) {
            let error = SafeOpsError::ReplayedGrant {
                grant_id: grant.grant_id,
            };
            self.audit.push(AuditEvent::GrantRejected {
                action,
                snapshot: expected,
                error: error.clone(),
            });
            return Err(error);
        }
        if &expected != current {
            let error = SafeOpsError::StaleGrant {
                expected: Box::new(expected.clone()),
                actual: Box::new(current.clone()),
            };
            self.audit.push(AuditEvent::GrantRejected {
                action,
                snapshot: expected,
                error: error.clone(),
            });
            return Err(error);
        }
        self.used_grants.insert(grant.grant_id);
        self.audit.push(AuditEvent::GrantConsumed {
            action,
            snapshot: expected,
        });
        Ok(ExecutionPermit {
            intent: grant.intent,
        })
    }

    fn authorize_inner(&mut self, intent: &ToolIntent, approvals: &[Approval]) -> Decision {
        if intent.actor != Principal::OperatorLlm {
            return Decision::Deny(Denial::UnauthorizedActor {
                actor: intent.actor,
            });
        }
        if let Some(reason) = precondition_failure(&intent.action, &intent.snapshot) {
            return Decision::Deny(Denial::Precondition {
                action: intent.action.clone(),
                reason,
            });
        }
        if matches!(intent.action, Action::DeleteNamespace) {
            return Decision::Deny(Denial::UnsafeOutcome {
                action: intent.action.clone(),
            });
        }
        if matches!(intent.action, Action::UpdateImage { .. })
            && !approvals
                .iter()
                .any(|approval| approval.principal == Principal::HumanOperator)
        {
            return Decision::Deny(Denial::MissingApproval {
                required: Principal::HumanOperator,
            });
        }
        let coalition = match self.coalition_for(&intent.action, approvals) {
            Ok(coalition) => coalition,
            Err(error) => return policy_failure(&intent.action, error),
        };
        match self.coalition_can_enforce(&intent.action, &intent.snapshot, &coalition) {
            Ok(true) => {}
            Ok(false) => {
                return Decision::Deny(Denial::PolicyRejected {
                    action: intent.action.clone(),
                });
            }
            Err(error) => return policy_failure(&intent.action, error),
        }
        if let Err(error) = self.outcomes_are_safe(&intent.action) {
            return policy_failure(&intent.action, error);
        }
        let grant = ExecutionGrant {
            intent: intent.clone(),
            grant_id: self.next_grant_id,
        };
        self.next_grant_id = self.next_grant_id.saturating_add(1);
        Decision::Allow(grant)
    }

    fn coalition_for(
        &self,
        action: &Action,
        approvals: &[Approval],
    ) -> Result<Coalition, SafeOpsError> {
        let agent = |principal: Principal| {
            self.model
                .agents
                .lookup(&principal.to_string())
                .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))
        };
        let mut coalition =
            Coalition::from_agents([agent(Principal::OperatorLlm)?, agent(Principal::Executor)?]);
        if matches!(action, Action::UpdateImage { .. })
            && approvals
                .iter()
                .any(|approval| approval.principal == Principal::HumanOperator)
        {
            coalition.insert(agent(Principal::HumanOperator)?);
        }
        Ok(coalition)
    }

    fn coalition_can_enforce(
        &self,
        action: &Action,
        snapshot: &DeploymentSnapshot,
        coalition: &Coalition,
    ) -> Result<bool, SafeOpsError> {
        let predicate = CoalitionPredicate::and(
            CoalitionPredicate::superset_eq(coalition.clone()),
            CoalitionPredicate::subset_eq(coalition.clone()),
        );
        let formula = Formula::exists(predicate, self.formula_for(action)?);
        ModelChecker::new(&self.model)
            .check(self.state_id(snapshot)?, &formula)
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))
    }

    fn outcomes_are_safe(&self, action: &Action) -> Result<(), SafeOpsError> {
        let safe = self.parse_formula("safe")?;
        let checker = ModelChecker::new(&self.model);
        let outcomes = if matches!(action, Action::UpdateImage { .. }) {
            vec!["image_changed"]
        } else {
            vec!["ready", "unready"]
        };
        for outcome in outcomes {
            let state = self
                .model
                .states
                .lookup(outcome)
                .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))?;
            if !checker
                .check(state, &safe)
                .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))?
            {
                return Err(SafeOpsError::PolicyFormula(format!(
                    "unsafe outcome {outcome}"
                )));
            }
        }
        Ok(())
    }

    fn formula_for(&self, action: &Action) -> Result<Formula, SafeOpsError> {
        self.parse_formula(if matches!(action, Action::UpdateImage { .. }) {
            "image_change"
        } else {
            "safe"
        })
    }

    fn parse_formula(&self, source: &str) -> Result<Formula, SafeOpsError> {
        parse_formula(source)
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))?
            .resolve(&self.model.agents, &self.model.atoms)
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))
    }

    fn state_id(&self, snapshot: &DeploymentSnapshot) -> Result<StateId, SafeOpsError> {
        self.model
            .states
            .lookup(if snapshot.is_ready() {
                "ready"
            } else {
                "unready"
            })
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))
    }
}

fn precondition_failure(action: &Action, snapshot: &DeploymentSnapshot) -> Option<String> {
    match action {
        Action::RestartRollout { .. } if !snapshot.is_ready() => {
            Some("deployment is not ready".to_owned())
        }
        Action::RestartRollout {
            strategy: RolloutStrategy::Recreate,
        } => Some("only rolling strategy is permitted".to_owned()),
        Action::Scale { replicas } if *replicas < 2 => {
            Some("desired replicas must remain at least 2".to_owned())
        }
        Action::UpdateImage { image } if image.is_empty() => Some("image is empty".to_owned()),
        _ => None,
    }
}

fn policy_failure(action: &Action, error: impl fmt::Display) -> Decision {
    Decision::Deny(Denial::PolicyFailure {
        action: action.clone(),
        message: error.to_string(),
    })
}
