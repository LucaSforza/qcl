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
  states { stable, degraded, restarted, deployed, deleted };
  props { safe, production_modified, data_deleted };
  valuation {
    stable: { safe };
    degraded: { safe };
    restarted: { safe };
    deployed: { safe, production_modified };
    deleted: { data_deleted };
  };
  effectivity {
    stable, {} -> { stable, degraded, restarted, deployed, deleted };
    stable, { operator_llm } -> { stable, degraded, restarted, deployed, deleted };
    stable, { human_operator } -> { stable, degraded, restarted, deployed, deleted };
    stable, { executor } -> { stable, degraded, restarted, deployed, deleted };
    stable, { operator_llm, human_operator } -> { stable, degraded, restarted, deployed, deleted };
    stable, { operator_llm, executor } -> { stable, degraded, restarted, deployed, deleted };
    stable, { human_operator, executor } -> { stable, degraded, restarted, deployed, deleted };
    stable, { operator_llm, executor } -> { restarted };
    stable, { operator_llm, human_operator, executor } -> { stable };
    stable, { operator_llm, human_operator, executor } -> { degraded };
    stable, { operator_llm, human_operator, executor } -> { restarted };
    stable, { operator_llm, human_operator, executor } -> { deployed };
    stable, { operator_llm, human_operator, executor } -> { deleted };

    degraded, {} -> { stable, degraded, restarted, deployed, deleted };
    degraded, { operator_llm } -> { stable, degraded, restarted, deployed, deleted };
    degraded, { human_operator } -> { stable, degraded, restarted, deployed, deleted };
    degraded, { executor } -> { stable, degraded, restarted, deployed, deleted };
    degraded, { operator_llm, human_operator } -> { stable, degraded, restarted, deployed, deleted };
    degraded, { operator_llm, executor } -> { stable, degraded, restarted, deployed, deleted };
    degraded, { human_operator, executor } -> { stable, degraded, restarted, deployed, deleted };
    degraded, { operator_llm, executor } -> { restarted };
    degraded, { operator_llm, human_operator, executor } -> { stable };
    degraded, { operator_llm, human_operator, executor } -> { degraded };
    degraded, { operator_llm, human_operator, executor } -> { restarted };
    degraded, { operator_llm, human_operator, executor } -> { deployed };
    degraded, { operator_llm, human_operator, executor } -> { deleted };

    restarted, {} -> { stable, degraded, restarted, deployed, deleted };
    restarted, { operator_llm } -> { stable, degraded, restarted, deployed, deleted };
    restarted, { human_operator } -> { stable, degraded, restarted, deployed, deleted };
    restarted, { executor } -> { stable, degraded, restarted, deployed, deleted };
    restarted, { operator_llm, human_operator } -> { stable, degraded, restarted, deployed, deleted };
    restarted, { operator_llm, executor } -> { stable, degraded, restarted, deployed, deleted };
    restarted, { human_operator, executor } -> { stable, degraded, restarted, deployed, deleted };
    restarted, { operator_llm, executor } -> { restarted };
    restarted, { operator_llm, human_operator, executor } -> { stable };
    restarted, { operator_llm, human_operator, executor } -> { degraded };
    restarted, { operator_llm, human_operator, executor } -> { restarted };
    restarted, { operator_llm, human_operator, executor } -> { deployed };
    restarted, { operator_llm, human_operator, executor } -> { deleted };

    deployed, {} -> { stable, degraded, restarted, deployed, deleted };
    deployed, { operator_llm } -> { stable, degraded, restarted, deployed, deleted };
    deployed, { human_operator } -> { stable, degraded, restarted, deployed, deleted };
    deployed, { executor } -> { stable, degraded, restarted, deployed, deleted };
    deployed, { operator_llm, human_operator } -> { stable, degraded, restarted, deployed, deleted };
    deployed, { operator_llm, executor } -> { stable, degraded, restarted, deployed, deleted };
    deployed, { human_operator, executor } -> { stable, degraded, restarted, deployed, deleted };
    deployed, { operator_llm, executor } -> { restarted };
    deployed, { operator_llm, human_operator, executor } -> { stable };
    deployed, { operator_llm, human_operator, executor } -> { degraded };
    deployed, { operator_llm, human_operator, executor } -> { restarted };
    deployed, { operator_llm, human_operator, executor } -> { deployed };
    deployed, { operator_llm, human_operator, executor } -> { deleted };

    deleted, {} -> { stable, degraded, restarted, deployed, deleted };
    deleted, { operator_llm } -> { stable, degraded, restarted, deployed, deleted };
    deleted, { human_operator } -> { stable, degraded, restarted, deployed, deleted };
    deleted, { executor } -> { stable, degraded, restarted, deployed, deleted };
    deleted, { operator_llm, human_operator } -> { stable, degraded, restarted, deployed, deleted };
    deleted, { operator_llm, executor } -> { stable, degraded, restarted, deployed, deleted };
    deleted, { human_operator, executor } -> { stable, degraded, restarted, deployed, deleted };
    deleted, { operator_llm, executor } -> { restarted };
    deleted, { operator_llm, human_operator, executor } -> { stable };
    deleted, { operator_llm, human_operator, executor } -> { degraded };
    deleted, { operator_llm, human_operator, executor } -> { restarted };
    deleted, { operator_llm, human_operator, executor } -> { deployed };
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
pub enum Tool {
    InspectLogs,
    RestartCanary,
    DeployRelease,
    DeleteResource,
}

impl fmt::Display for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InspectLogs => "inspect_logs",
            Self::RestartCanary => "restart_canary",
            Self::DeployRelease => "deploy_release",
            Self::DeleteResource => "delete_resource",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ToolIntent {
    actor: Principal,
    tool: Tool,
    snapshot_version: u64,
}

impl ToolIntent {
    #[must_use]
    pub const fn new(actor: Principal, tool: Tool, snapshot_version: u64) -> Self {
        Self {
            actor,
            tool,
            snapshot_version,
        }
    }

    #[must_use]
    pub const fn actor(self) -> Principal {
        self.actor
    }

    #[must_use]
    pub const fn tool(self) -> Tool {
        self.tool
    }

    #[must_use]
    pub const fn snapshot_version(self) -> u64 {
        self.snapshot_version
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SystemState {
    Stable,
    Degraded,
    CanaryRestarted,
    ProductionModified,
    DataDeleted,
}

impl fmt::Display for SystemState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stable => "stable",
            Self::Degraded => "degraded",
            Self::CanaryRestarted => "restarted",
            Self::ProductionModified => "deployed",
            Self::DataDeleted => "deleted",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WorldState {
    state: SystemState,
    version: u64,
}

impl WorldState {
    #[must_use]
    pub const fn new(state: SystemState, version: u64) -> Self {
        Self { state, version }
    }

    #[must_use]
    pub const fn state(self) -> SystemState {
        self.state
    }

    #[must_use]
    pub const fn version(self) -> u64 {
        self.version
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

    #[must_use]
    pub const fn principal(self) -> Principal {
        self.principal
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Denial {
    UnauthorizedActor { actor: Principal },
    VersionMismatch { expected: u64, actual: u64 },
    Precondition { tool: Tool, state: SystemState },
    PolicyRejected { tool: Tool, state: SystemState },
    PolicyFailure { tool: Tool, message: String },
    UnsafeOutcome { tool: Tool, state: SystemState },
}

impl fmt::Display for Denial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnauthorizedActor { actor } => write!(f, "unauthorized actor {actor}"),
            Self::VersionMismatch { expected, actual } => {
                write!(
                    f,
                    "snapshot version {actual} does not match intent version {expected}"
                )
            }
            Self::Precondition { tool, state } => write!(f, "{tool} is not valid in state {state}"),
            Self::PolicyRejected { tool, state } => {
                write!(f, "QCL policy rejects {tool} in state {state}")
            }
            Self::PolicyFailure { tool, message } => {
                write!(f, "QCL policy check failed for {tool}: {message}")
            }
            Self::UnsafeOutcome { tool, state } => {
                write!(f, "{tool} declares unsafe outcome state {state}")
            }
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
    snapshot: WorldState,
    grant_id: u64,
}

impl ExecutionGrant {
    #[must_use]
    pub const fn intent(&self) -> ToolIntent {
        self.intent
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
    Execution {
        tool: Tool,
        from: WorldState,
        to: WorldState,
    },
    ExecutionRejected {
        tool: Tool,
        at: WorldState,
        error: SafeOpsError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SafeOpsError {
    PolicyParse(String),
    PolicyInvalid(String),
    PolicyFormula(String),
    StaleGrant {
        expected: WorldState,
        actual: WorldState,
    },
    ReplayedGrant {
        grant_id: u64,
    },
}

impl fmt::Display for SafeOpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyParse(error) => write!(f, "policy parse failed: {error}"),
            Self::PolicyInvalid(error) => write!(f, "policy validation failed: {error}"),
            Self::PolicyFormula(error) => write!(f, "policy formula failed: {error}"),
            Self::StaleGrant { expected, actual } => {
                write!(
                    f,
                    "grant for {} version {} is stale at {} version {}",
                    expected.state, expected.version, actual.state, actual.version
                )
            }
            Self::ReplayedGrant { grant_id } => write!(f, "grant {grant_id} was already used"),
        }
    }
}

impl std::error::Error for SafeOpsError {}

pub struct SafetyKernel {
    model: QclModel,
    next_grant_id: u64,
    audit: Vec<AuditEvent>,
}

impl SafetyKernel {
    /// Construct a kernel after parsing and validating the embedded QCL policy.
    ///
    /// # Errors
    ///
    /// Returns a structured error if the embedded policy cannot be parsed or
    /// fails QCL model validation.
    pub fn new() -> Result<Self, SafeOpsError> {
        let model =
            parse_model(POLICY).map_err(|error| SafeOpsError::PolicyParse(error.to_string()))?;
        ModelValidator::validate(&model)
            .map_err(|error| SafeOpsError::PolicyInvalid(error.to_string()))?;
        Ok(Self {
            model,
            next_grant_id: 0,
            audit: Vec::new(),
        })
    }

    #[must_use]
    pub fn audit(&self) -> &[AuditEvent] {
        &self.audit
    }

    pub fn authorize(
        &mut self,
        intent: &ToolIntent,
        snapshot: &WorldState,
        approvals: &[Approval],
    ) -> Decision {
        self.audit.push(AuditEvent::Proposal { intent: *intent });
        let decision = self.authorize_inner(intent, snapshot, approvals);
        self.audit.push(AuditEvent::Decision {
            allowed: matches!(decision, Decision::Allow(_)),
            denial: match &decision {
                Decision::Deny(denial) => Some(denial.clone()),
                Decision::Allow(_) => None,
            },
        });
        decision
    }

    /// Execute a previously issued grant through the simulator and append an
    /// execution event to the audit trail.
    ///
    /// # Errors
    ///
    /// Returns a stale or replay error when the grant is no longer current.
    pub fn execute(
        &mut self,
        simulator: &mut Simulator,
        grant: ExecutionGrant,
    ) -> Result<ExecutionReceipt, SafeOpsError> {
        let tool = grant.intent.tool;
        match simulator.execute(grant) {
            Ok(receipt) => {
                self.audit.push(AuditEvent::Execution {
                    tool: receipt.tool,
                    from: receipt.from,
                    to: receipt.to,
                });
                Ok(receipt)
            }
            Err(error) => {
                self.audit.push(AuditEvent::ExecutionRejected {
                    tool,
                    at: simulator.current(),
                    error: error.clone(),
                });
                Err(error)
            }
        }
    }

    fn authorize_inner(
        &mut self,
        intent: &ToolIntent,
        snapshot: &WorldState,
        approvals: &[Approval],
    ) -> Decision {
        if intent.actor != Principal::OperatorLlm {
            return Decision::Deny(Denial::UnauthorizedActor {
                actor: intent.actor,
            });
        }
        if intent.snapshot_version != snapshot.version {
            return Decision::Deny(Denial::VersionMismatch {
                expected: intent.snapshot_version,
                actual: snapshot.version,
            });
        }
        if intent.tool == Tool::DeleteResource && snapshot.state == SystemState::DataDeleted {
            return Decision::Deny(Denial::Precondition {
                tool: intent.tool,
                state: snapshot.state,
            });
        }
        let coalition = match coalition_for(&self.model, intent.tool, approvals) {
            Ok(coalition) => coalition,
            Err(error) => return policy_failure(intent.tool, error),
        };
        match self.coalition_can_enforce(intent.tool, snapshot.state, &coalition) {
            Ok(true) => {}
            Ok(false) => {
                return Decision::Deny(Denial::PolicyRejected {
                    tool: intent.tool,
                    state: snapshot.state,
                });
            }
            Err(error) => return policy_failure(intent.tool, error),
        }
        match self.first_unsafe_outcome(intent.tool, snapshot.state) {
            Ok(Some(state)) => {
                return Decision::Deny(Denial::UnsafeOutcome {
                    tool: intent.tool,
                    state,
                });
            }
            Ok(None) => {}
            Err(error) => return policy_failure(intent.tool, error),
        }

        let grant = ExecutionGrant {
            intent: *intent,
            snapshot: *snapshot,
            grant_id: self.next_grant_id,
        };
        self.next_grant_id = self.next_grant_id.saturating_add(1);
        Decision::Allow(grant)
    }

    fn state_id(&self, state: SystemState) -> Result<StateId, SafeOpsError> {
        self.model
            .states
            .lookup(&state.to_string())
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))
    }

    fn formula_for(&self, tool: Tool) -> Result<Formula, SafeOpsError> {
        let source = match tool {
            Tool::InspectLogs | Tool::RestartCanary => "safe",
            Tool::DeployRelease => "production_modified",
            Tool::DeleteResource => "data_deleted",
        };
        parse_formula(source)
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))?
            .resolve(&self.model.agents, &self.model.atoms)
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))
    }

    fn safe_formula(&self) -> Result<Formula, SafeOpsError> {
        self.formula_for(Tool::RestartCanary)
    }

    fn coalition_can_enforce(
        &self,
        tool: Tool,
        state: SystemState,
        coalition: &Coalition,
    ) -> Result<bool, SafeOpsError> {
        let predicate = CoalitionPredicate::and(
            CoalitionPredicate::superset_eq(coalition.clone()),
            CoalitionPredicate::subset_eq(coalition.clone()),
        );
        let policy = Formula::exists(predicate, self.formula_for(tool)?);
        ModelChecker::new(&self.model)
            .check(self.state_id(state)?, &policy)
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))
    }

    fn first_unsafe_outcome(
        &self,
        tool: Tool,
        current: SystemState,
    ) -> Result<Option<SystemState>, SafeOpsError> {
        let safe = self.safe_formula()?;
        let checker = ModelChecker::new(&self.model);
        for outcome in outcomes(tool, current) {
            let is_safe = checker
                .check(self.state_id(outcome)?, &safe)
                .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))?;
            if !is_safe {
                return Ok(Some(outcome));
            }
        }
        Ok(None)
    }
}

fn policy_failure(tool: Tool, error: impl fmt::Display) -> Decision {
    Decision::Deny(Denial::PolicyFailure {
        tool,
        message: error.to_string(),
    })
}

fn coalition_for(
    model: &QclModel,
    tool: Tool,
    approvals: &[Approval],
) -> Result<Coalition, SafeOpsError> {
    let agent = |principal: Principal| {
        model
            .agents
            .lookup(&principal.to_string())
            .map_err(|error| SafeOpsError::PolicyFormula(error.to_string()))
    };
    let mut coalition =
        Coalition::from_agents([agent(Principal::OperatorLlm)?, agent(Principal::Executor)?]);
    if matches!(tool, Tool::DeployRelease | Tool::DeleteResource)
        && approvals
            .iter()
            .any(|approval| approval.principal == Principal::HumanOperator)
    {
        coalition.insert(agent(Principal::HumanOperator)?);
    }
    Ok(coalition)
}

fn outcomes(tool: Tool, current: SystemState) -> impl Iterator<Item = SystemState> {
    let list = match tool {
        Tool::InspectLogs => vec![current],
        Tool::RestartCanary => vec![SystemState::CanaryRestarted, SystemState::Degraded],
        Tool::DeployRelease => vec![SystemState::ProductionModified, SystemState::Degraded],
        Tool::DeleteResource => vec![SystemState::DataDeleted],
    };
    list.into_iter()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionReceipt {
    pub tool: Tool,
    pub from: WorldState,
    pub to: WorldState,
}

pub struct Simulator {
    current: WorldState,
    used_grants: HashSet<u64>,
}

impl Simulator {
    #[must_use]
    pub fn new(initial: WorldState) -> Self {
        Self {
            current: initial,
            used_grants: HashSet::new(),
        }
    }

    #[must_use]
    pub const fn current(&self) -> WorldState {
        self.current
    }

    /// Execute a grant against the current simulated world state.
    ///
    /// # Errors
    ///
    /// Returns an error when the grant is stale or has already been used.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "execution consumes the capability even though its fields are copyable"
    )]
    pub fn execute(&mut self, grant: ExecutionGrant) -> Result<ExecutionReceipt, SafeOpsError> {
        let ExecutionGrant {
            intent,
            snapshot,
            grant_id,
        } = grant;
        if self.used_grants.contains(&grant_id) {
            return Err(SafeOpsError::ReplayedGrant { grant_id });
        }
        if snapshot != self.current {
            return Err(SafeOpsError::StaleGrant {
                expected: snapshot,
                actual: self.current,
            });
        }
        self.used_grants.insert(grant_id);
        let from = self.current;
        let next_state = match intent.tool {
            Tool::InspectLogs => from.state,
            Tool::RestartCanary => SystemState::CanaryRestarted,
            Tool::DeployRelease => SystemState::ProductionModified,
            Tool::DeleteResource => SystemState::DataDeleted,
        };
        self.current = WorldState::new(next_state, from.version.saturating_add(1));
        Ok(ExecutionReceipt {
            tool: intent.tool,
            from,
            to: self.current,
        })
    }
}
