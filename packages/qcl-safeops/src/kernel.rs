use std::collections::HashSet;
use std::fmt;

use qcl::ast::{CoalitionPredicate, Formula};
use qcl::checker::ModelChecker;
use qcl::domain::{AgentId, Coalition, StateId};
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
    MissingApproval { required: Principal },
    Precondition { tool: Tool, state: SystemState },
    PolicyRejected { tool: Tool, state: SystemState },
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
            Self::MissingApproval { required } => write!(f, "missing approval from {required}"),
            Self::Precondition { tool, state } => write!(f, "{tool} is not valid in state {state}"),
            Self::PolicyRejected { tool, state } => {
                write!(f, "QCL policy rejects {tool} in state {state}")
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
    coalition: Coalition,
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
}

#[derive(Debug)]
pub enum SafeOpsError {
    PolicyParse(String),
    PolicyInvalid(String),
    PolicyFormula(String),
    GrantMismatch { expected: u64, actual: u64 },
    StaleGrant { expected: u64, actual: u64 },
    ReplayedGrant { grant_id: u64 },
}

impl fmt::Display for SafeOpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyParse(error) => write!(f, "policy parse failed: {error}"),
            Self::PolicyInvalid(error) => write!(f, "policy validation failed: {error}"),
            Self::PolicyFormula(error) => write!(f, "policy formula failed: {error}"),
            Self::GrantMismatch { expected, actual } => {
                write!(
                    f,
                    "grant version {actual} does not match intent version {expected}"
                )
            }
            Self::StaleGrant { expected, actual } => {
                write!(
                    f,
                    "grant version {expected} is stale at simulator version {actual}"
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
        let receipt = simulator.execute(grant)?;
        self.audit.push(AuditEvent::Execution {
            tool: receipt.tool,
            from: receipt.from,
            to: receipt.to,
        });
        Ok(receipt)
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
        if matches!(intent.tool, Tool::DeployRelease | Tool::DeleteResource)
            && !approvals
                .iter()
                .any(|approval| approval.principal == Principal::HumanOperator)
        {
            return Decision::Deny(Denial::MissingApproval {
                required: Principal::HumanOperator,
            });
        }

        let coalition = coalition_for(intent.tool, approvals);
        let Ok(source) = self.state_id(snapshot.state) else {
            return Decision::Deny(Denial::PolicyRejected {
                tool: intent.tool,
                state: snapshot.state,
            });
        };
        let Ok(target) = self.formula_for(intent.tool) else {
            return Decision::Deny(Denial::PolicyRejected {
                tool: intent.tool,
                state: snapshot.state,
            });
        };
        let predicate = CoalitionPredicate::and(
            CoalitionPredicate::superset_eq(coalition.clone()),
            CoalitionPredicate::subset_eq(coalition.clone()),
        );
        let policy = Formula::exists(predicate, target);
        if !ModelChecker::new(&self.model)
            .check(source, &policy)
            .unwrap_or(false)
        {
            return Decision::Deny(Denial::PolicyRejected {
                tool: intent.tool,
                state: snapshot.state,
            });
        }

        for outcome in outcomes(intent.tool, snapshot.state) {
            let Ok(outcome_id) = self.state_id(outcome) else {
                return Decision::Deny(Denial::UnsafeOutcome {
                    tool: intent.tool,
                    state: outcome,
                });
            };
            let Ok(safe) = self.safe_formula() else {
                return Decision::Deny(Denial::UnsafeOutcome {
                    tool: intent.tool,
                    state: outcome,
                });
            };
            if !ModelChecker::new(&self.model)
                .check(outcome_id, &safe)
                .unwrap_or(false)
            {
                return Decision::Deny(Denial::UnsafeOutcome {
                    tool: intent.tool,
                    state: outcome,
                });
            }
        }

        let grant = ExecutionGrant {
            intent: *intent,
            coalition,
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
}

fn coalition_for(tool: Tool, approvals: &[Approval]) -> Coalition {
    let mut coalition = Coalition::from_agents([AgentId::new(0), AgentId::new(2)]);
    if matches!(tool, Tool::DeployRelease | Tool::DeleteResource)
        && approvals
            .iter()
            .any(|approval| approval.principal == Principal::HumanOperator)
    {
        coalition.insert(AgentId::new(1));
    }
    coalition
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
    pub fn execute(&mut self, grant: ExecutionGrant) -> Result<ExecutionReceipt, SafeOpsError> {
        let ExecutionGrant {
            intent,
            coalition,
            grant_id,
        } = grant;
        drop(coalition);
        if self.used_grants.contains(&grant_id) {
            return Err(SafeOpsError::ReplayedGrant { grant_id });
        }
        if intent.snapshot_version != self.current.version {
            return Err(SafeOpsError::StaleGrant {
                expected: intent.snapshot_version,
                actual: self.current.version,
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
