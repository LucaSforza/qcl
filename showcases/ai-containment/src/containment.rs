use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use qcl::checker::ModelChecker;
use qcl::domain::{AgentId, Coalition, StateId, StateSet};
use qcl::game_form::{self, FiniteGameForm, JointActionProfile};
use qcl::model::QclModel;
use qcl::parser::parse_formula;
use qcl::symbols::SymbolTable;

/// Components whose actions form the finite concurrent system.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Agent {
    AgentA,
    AgentB,
    SharedService,
    Sandbox,
    EgressGateway,
    CredentialBroker,
    SafetyMonitor,
}

impl Agent {
    /// Stable order used by profiles and coalitions.
    pub const ALL: [Self; 7] = [
        Self::AgentA,
        Self::AgentB,
        Self::SharedService,
        Self::Sandbox,
        Self::EgressGateway,
        Self::CredentialBroker,
        Self::SafetyMonitor,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AgentA => "agent_a",
            Self::AgentB => "agent_b",
            Self::SharedService => "shared_service",
            Self::Sandbox => "sandbox",
            Self::EgressGateway => "egress_gateway",
            Self::CredentialBroker => "credential_broker",
            Self::SafetyMonitor => "safety_monitor",
        }
    }

    #[must_use]
    pub const fn id(self) -> AgentId {
        AgentId::new(self as usize)
    }
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Outcome classes and source state in the executable model.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ContainmentState {
    Start,
    Contained,
    ChannelOpen,
    InternetAccess,
    CredentialsObtained,
    ExternalCompromise,
}

impl ContainmentState {
    pub const ALL: [Self; 6] = [
        Self::Start,
        Self::Contained,
        Self::ChannelOpen,
        Self::InternetAccess,
        Self::CredentialsObtained,
        Self::ExternalCompromise,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Contained => "contained",
            Self::ChannelOpen => "channel_open",
            Self::InternetAccess => "internet_access",
            Self::CredentialsObtained => "credentials_obtained",
            Self::ExternalCompromise => "external_compromise",
        }
    }

    #[must_use]
    pub const fn id(self) -> StateId {
        StateId::new(self as usize)
    }
}

impl fmt::Display for ContainmentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Small action vocabulary. `ContainmentSystem::available_actions` filters it
/// by agent, so invalid role/action combinations cannot be selected there.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Action {
    Idle,
    Attack,
    Isolate,
    Relay,
    Enforce,
    Bypass,
    Deny,
    Allow,
    Protect,
    Expose,
    Block,
    Miss,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Idle => "idle",
            Self::Attack => "attack",
            Self::Isolate => "isolate",
            Self::Relay => "relay",
            Self::Enforce => "enforce",
            Self::Bypass => "bypass",
            Self::Deny => "deny",
            Self::Allow => "allow",
            Self::Protect => "protect",
            Self::Expose => "expose",
            Self::Block => "block",
            Self::Miss => "miss",
        };
        f.write_str(name)
    }
}

/// A complete, role-checked action profile.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JointAction {
    pub agent_a: Action,
    pub agent_b: Action,
    pub shared_service: Action,
    pub sandbox: Action,
    pub egress_gateway: Action,
    pub credential_broker: Action,
    pub safety_monitor: Action,
}

impl JointAction {
    /// Build a profile, rejecting role/action mismatches.
    ///
    /// # Errors
    ///
    /// Returns an error when any action is not valid for its role.
    pub fn new(actions: [Action; 7]) -> Result<Self, String> {
        let profile = Self {
            agent_a: actions[0],
            agent_b: actions[1],
            shared_service: actions[2],
            sandbox: actions[3],
            egress_gateway: actions[4],
            credential_broker: actions[5],
            safety_monitor: actions[6],
        };
        profile
            .is_valid()
            .then_some(profile)
            .ok_or_else(|| "action is invalid for its agent".to_owned())
    }

    #[must_use]
    pub fn action(self, agent: Agent) -> Action {
        match agent {
            Agent::AgentA => self.agent_a,
            Agent::AgentB => self.agent_b,
            Agent::SharedService => self.shared_service,
            Agent::Sandbox => self.sandbox,
            Agent::EgressGateway => self.egress_gateway,
            Agent::CredentialBroker => self.credential_broker,
            Agent::SafetyMonitor => self.safety_monitor,
        }
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        Agent::ALL
            .into_iter()
            .all(|agent| ContainmentSystem::actions_for(agent).contains(&self.action(agent)))
    }
}

/// Selectable executable semantics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Scenario {
    Hardened,
    SharedServiceBypass,
}

impl Scenario {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hardened => "hardened",
            Self::SharedServiceBypass => "shared-service-bypass",
        }
    }
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Scenario {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "hardened" => Ok(Self::Hardened),
            "shared-service-bypass" => Ok(Self::SharedServiceBypass),
            _ => Err(format!("unknown scenario `{value}`")),
        }
    }
}

/// Executable containment system. This struct owns authoritative transition semantics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContainmentSystem {
    pub scenario: Scenario,
}

impl ContainmentSystem {
    #[must_use]
    pub const fn new(scenario: Scenario) -> Self {
        Self { scenario }
    }

    #[must_use]
    pub const fn agents(&self) -> &[Agent; 7] {
        &Agent::ALL
    }

    #[must_use]
    pub fn available_actions(&self, _state: ContainmentState, agent: Agent) -> Vec<Action> {
        Self::actions_for(agent).to_vec()
    }

    const fn actions_for(agent: Agent) -> &'static [Action] {
        match agent {
            Agent::AgentA | Agent::AgentB => &[Action::Idle, Action::Attack],
            Agent::SharedService => &[Action::Isolate, Action::Relay],
            Agent::Sandbox => &[Action::Enforce, Action::Bypass],
            Agent::EgressGateway => &[Action::Deny, Action::Allow],
            Agent::CredentialBroker => &[Action::Protect, Action::Expose],
            Agent::SafetyMonitor => &[Action::Block, Action::Miss],
        }
    }

    /// Authoritative one-step transition function. Terminal classes self-loop.
    #[must_use]
    pub fn transition(&self, state: ContainmentState, action: &JointAction) -> ContainmentState {
        if state != ContainmentState::Start {
            return state;
        }
        if !action.is_valid() {
            return ContainmentState::Contained;
        }
        let coordinated = action.agent_a == Action::Attack
            && action.agent_b == Action::Attack
            && action.shared_service == Action::Relay;
        if !coordinated {
            return ContainmentState::Contained;
        }
        if self.scenario == Scenario::SharedServiceBypass {
            return ContainmentState::ExternalCompromise;
        }
        if action.sandbox != Action::Bypass || action.egress_gateway != Action::Allow {
            return ContainmentState::ChannelOpen;
        }
        if action.credential_broker != Action::Expose {
            return ContainmentState::InternetAccess;
        }
        if action.safety_monitor != Action::Miss {
            return ContainmentState::CredentialsObtained;
        }
        ContainmentState::ExternalCompromise
    }

    #[must_use]
    pub fn all_joint_actions(&self) -> Vec<JointAction> {
        fn go(
            system: ContainmentSystem,
            index: usize,
            actions: &mut [Action; 7],
            output: &mut Vec<JointAction>,
        ) {
            if index == Agent::ALL.len() {
                output.push(JointAction::new(*actions).expect("role action tables are valid"));
                return;
            }
            for action in system.available_actions(ContainmentState::Start, Agent::ALL[index]) {
                actions[index] = action;
                go(system, index + 1, actions, output);
            }
        }
        let mut actions = [Action::Idle; 7];
        let mut output = Vec::new();
        go(*self, 0, &mut actions, &mut output);
        output
    }

    /// Derive all minimal outcome sets directly from `transition`.
    #[must_use]
    pub fn derive_effectivity(&self) -> DerivedEffectivity {
        DerivedEffectivity {
            inner: game_form::derive_effectivity(self),
            full_joint_action_profiles: self.all_joint_actions().len(),
        }
    }

    /// Construct the QCL model using derived, never hand-authored, effectivity.
    ///
    /// # Panics
    ///
    /// Panics only if the static agent, state, or atom names are duplicated.
    #[must_use]
    pub fn qcl_model(&self) -> DerivedModel {
        let derived = self.derive_effectivity();
        let mut agents = SymbolTable::with_capacity(Agent::ALL.len());
        let mut states = SymbolTable::with_capacity(ContainmentState::ALL.len());
        let mut atoms = SymbolTable::with_capacity(5);
        for agent in Agent::ALL {
            agents.insert(agent.name()).expect("unique agent name");
        }
        for state in ContainmentState::ALL {
            states.insert(state.name()).expect("unique state name");
        }
        for atom in [
            "safe",
            "channel",
            "internet",
            "credentials",
            "loss_of_containment",
            "compromised",
        ] {
            atoms.insert(atom).expect("unique atom name");
        }
        let mut valuation = HashMap::new();
        for state in ContainmentState::ALL {
            let names = match state {
                ContainmentState::Contained => vec!["safe"],
                ContainmentState::ChannelOpen => vec!["channel"],
                ContainmentState::InternetAccess => {
                    vec!["channel", "internet", "loss_of_containment"]
                }
                ContainmentState::CredentialsObtained => {
                    vec!["channel", "internet", "credentials", "loss_of_containment"]
                }
                ContainmentState::ExternalCompromise => vec![
                    "channel",
                    "internet",
                    "credentials",
                    "loss_of_containment",
                    "compromised",
                ],
                ContainmentState::Start => Vec::new(),
            };
            valuation.insert(
                state.id(),
                names
                    .into_iter()
                    .map(|name| atoms.lookup(name).expect("known atom"))
                    .collect(),
            );
        }
        DerivedModel {
            model: QclModel::new(
                agents,
                states,
                atoms,
                valuation,
                derived.effectivity().clone(),
            ),
            derived,
        }
    }
}

impl FiniteGameForm for ContainmentSystem {
    type Action = Action;

    fn agent_count(&self) -> usize {
        Agent::ALL.len()
    }

    fn state_count(&self) -> usize {
        ContainmentState::ALL.len()
    }

    fn actions(&self, state: StateId, agent: AgentId) -> Vec<Self::Action> {
        self.available_actions(
            ContainmentState::ALL[state.index()],
            Agent::ALL[agent.index()],
        )
    }

    fn transition(&self, state: StateId, profile: &JointActionProfile<Self::Action>) -> StateId {
        let actions =
            Agent::ALL.map(|agent| *profile.action(agent.id()).expect("complete game profile"));
        let profile = JointAction::new(actions).expect("game form produces role-valid profile");
        ContainmentSystem::transition(self, ContainmentState::ALL[state.index()], &profile).id()
    }
}

/// Explicit effectivity plus presentation metadata retained from enumeration.
#[derive(Clone, Debug)]
pub struct DerivedEffectivity {
    inner: game_form::DerivedEffectivity<Action>,
    full_joint_action_profiles: usize,
}

impl DerivedEffectivity {
    #[must_use]
    pub const fn effectivity(&self) -> &qcl::model::Effectivity {
        self.inner.as_effectivity()
    }

    #[must_use]
    pub fn witnesses(
        &self,
        state: ContainmentState,
        coalition: &Coalition,
    ) -> Option<&[game_form::StrategyWitness<Action>]> {
        self.inner.witnesses(state.id(), coalition)
    }

    #[must_use]
    pub fn witness_for(
        &self,
        state: ContainmentState,
        coalition: &Coalition,
        target: &StateSet,
    ) -> Option<&game_form::StrategyWitness<Action>> {
        self.inner.find_witness(state.id(), coalition, target)
    }

    #[must_use]
    pub const fn full_joint_action_profiles(&self) -> usize {
        self.full_joint_action_profiles
    }
}

/// QCL model and its derivation metadata.
#[derive(Clone, Debug)]
pub struct DerivedModel {
    pub model: QclModel,
    pub derived: DerivedEffectivity,
}

/// Names of atoms and states useful for independent exhaustive checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateSetNames;

impl StateSetNames {
    #[must_use]
    pub const fn all() -> [ContainmentState; 6] {
        ContainmentState::ALL
    }
}

/// One named QCL audit property and result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditCheck {
    pub formula: &'static str,
    pub passed: bool,
}

/// Machine-readable audit result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditReport {
    pub scenario: Scenario,
    pub checks: Vec<AuditCheck>,
    pub agent_count: usize,
    pub coalition_count: usize,
    pub full_joint_action_profiles: usize,
}

const FORMULAS: [&str; 7] = [
    "<subset({agent_a, agent_b, shared_service})> channel",
    "!<subset({agent_a, agent_b, shared_service})> internet",
    "!<!(size >= 5)> internet",
    "!<excludes(egress_gateway)> internet",
    "!<excludes(credential_broker)> credentials",
    "[includes(safety_monitor)] !compromised",
    "!<excludes(safety_monitor)> compromised",
];

/// Run all showcase properties and produce human-readable proof/counterexample output.
#[must_use]
pub fn run_audit(scenario: Scenario) -> String {
    let system = ContainmentSystem::new(scenario);
    let derived = system.qcl_model();
    let checker = ModelChecker::new(&derived.model);
    let state = ContainmentState::Start.id();
    let mut checks = Vec::new();
    for source in FORMULAS {
        let passed = parse_formula(source)
            .ok()
            .and_then(|parsed| {
                parsed
                    .resolve(&derived.model.agents, &derived.model.atoms)
                    .ok()
            })
            .and_then(|formula| checker.check(state, &formula).ok())
            .unwrap_or(false);
        checks.push(AuditCheck {
            formula: source,
            passed,
        });
    }
    let report = AuditReport {
        scenario,
        checks,
        agent_count: Agent::ALL.len(),
        coalition_count: Coalition::all(Agent::ALL.len()).count(),
        full_joint_action_profiles: derived.derived.full_joint_action_profiles(),
    };
    format_audit(&report, &derived)
}

fn format_audit(report: &AuditReport, derived: &DerivedModel) -> String {
    let mut lines = vec![
        "offline verification of an executable abstract containment model".to_owned(),
        format!("scenario: {}", report.scenario),
        format!("agents: {}", report.agent_count),
        format!("coalitions: {}", report.coalition_count),
        format!(
            "full joint-action profiles: {}",
            report.full_joint_action_profiles
        ),
        String::new(),
    ];
    for check in &report.checks {
        lines.push(format!(
            "{} {}",
            if check.passed { "PASS" } else { "FAIL" },
            check.formula
        ));
    }
    let coalition = Coalition::from_agents([
        Agent::AgentA.id(),
        Agent::AgentB.id(),
        Agent::SharedService.id(),
    ]);
    let target = StateSet::from_states([
        ContainmentState::ChannelOpen.id(),
        ContainmentState::InternetAccess.id(),
        ContainmentState::CredentialsObtained.id(),
        ContainmentState::ExternalCompromise.id(),
    ]);
    if let Some(witness) = derived
        .derived
        .witness_for(ContainmentState::Start, &coalition, &target)
    {
        lines.push(String::new());
        lines.push(
            "witness/counterexample coalition: {agent_a, agent_b, shared_service}".to_owned(),
        );
        lines.push(format!(
            "strategy: {}",
            format_actions(witness.strategy.iter())
        ));
        lines.push(format!(
            "possible outcomes: {}",
            format_states(&witness.outcomes)
        ));
        lines.push("explanation: channel is cumulative; Internet possibility does not imply coalition enforceability".to_owned());
    }
    for check in report.checks.iter().filter(|check| !check.passed) {
        lines.extend(format_counterexample(check.formula, derived));
    }
    lines.join("\n")
}

fn format_counterexample(formula: &str, derived: &DerivedModel) -> Vec<String> {
    let coalition = if formula.contains("includes(safety_monitor)") {
        Coalition::from_agents([
            Agent::AgentA.id(),
            Agent::AgentB.id(),
            Agent::SharedService.id(),
            Agent::SafetyMonitor.id(),
        ])
    } else {
        Coalition::from_agents([
            Agent::AgentA.id(),
            Agent::AgentB.id(),
            Agent::SharedService.id(),
        ])
    };
    let target = StateSet::singleton(ContainmentState::ExternalCompromise.id());
    let Some(witness) = derived
        .derived
        .witness_for(ContainmentState::Start, &coalition, &target)
    else {
        return vec![format!("counterexample for {formula}: no witness found")];
    };
    vec![
        format!("counterexample for {formula}"),
        format!("coalition: {}", format_coalition(&coalition)),
        format!("strategy: {}", format_actions(witness.strategy.iter())),
        format!("possible outcomes: {}", format_states(&witness.outcomes)),
        "explanation: fixed coalition actions force external_compromise against every outsider completion".to_owned(),
    ]
}

fn format_coalition(coalition: &Coalition) -> String {
    format!(
        "{{{}}}",
        coalition
            .iter()
            .map(|agent| Agent::ALL[agent.index()].name())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_actions<'a>(actions: impl Iterator<Item = (AgentId, &'a Action)>) -> String {
    actions
        .map(|(agent, action)| format!("{} = {action}", Agent::ALL[agent.index()]))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_states(states: &StateSet) -> String {
    let names = states
        .iter()
        .filter_map(|state| ContainmentState::ALL.get(state.index()).copied())
        .map(ContainmentState::name)
        .collect::<Vec<_>>();
    format!("{{{}}}", names.join(", "))
}

/// Run two concrete profiles through exactly `ContainmentSystem::transition`.
///
/// # Panics
///
/// Panics only if one of the two compile-time demo profiles violates its role's
/// action vocabulary.
#[must_use]
pub fn run_simulation(scenario: Scenario) -> String {
    let system = ContainmentSystem::new(scenario);
    let profiles = [
        JointAction::new([
            Action::Attack,
            Action::Attack,
            Action::Relay,
            Action::Enforce,
            Action::Deny,
            Action::Protect,
            Action::Block,
        ])
        .expect("valid profile"),
        JointAction::new([
            Action::Attack,
            Action::Attack,
            Action::Relay,
            Action::Bypass,
            Action::Allow,
            Action::Expose,
            Action::Miss,
        ])
        .expect("valid profile"),
    ];
    let mut lines = vec![format!(
        "offline simulation of executable abstract containment model ({scenario})"
    )];
    for profile in profiles {
        lines.push(format!("agent_a = {}, agent_b = {}, shared_service = {}, sandbox = {}, egress_gateway = {}, credential_broker = {}, safety_monitor = {}", profile.agent_a, profile.agent_b, profile.shared_service, profile.sandbox, profile.egress_gateway, profile.credential_broker, profile.safety_monitor));
        lines.push(format!(
            "result: {}",
            system.transition(ContainmentState::Start, &profile)
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn all_outcomes(
        system: ContainmentSystem,
        profiles: &[JointAction],
        state: ContainmentState,
        strategy: &[(AgentId, Action)],
    ) -> StateSet {
        StateSet::from_states(
            profiles
                .iter()
                .filter(|profile| {
                    strategy
                        .iter()
                        .all(|(agent, action)| profile.action(Agent::ALL[agent.index()]) == *action)
                })
                .map(|profile| system.transition(state, profile).id()),
        )
    }

    fn brute_strategies(
        profiles: &[JointAction],
        coalition: &Coalition,
    ) -> Vec<Vec<(AgentId, Action)>> {
        let mut seen = HashSet::new();
        profiles
            .iter()
            .filter_map(|profile| {
                let strategy: Vec<_> = coalition
                    .iter()
                    .map(|agent| (agent, profile.action(Agent::ALL[agent.index()])))
                    .collect();
                seen.insert(strategy.clone()).then_some(strategy)
            })
            .collect()
    }

    #[test]
    fn hardened_transition_has_four_escalation_classes() {
        let system = ContainmentSystem::new(Scenario::Hardened);
        let profiles = system.all_joint_actions();
        let outcomes: HashSet<_> = profiles
            .iter()
            .map(|profile| system.transition(ContainmentState::Start, profile))
            .collect();
        assert!(outcomes.contains(&ContainmentState::Contained));
        assert!(outcomes.contains(&ContainmentState::ChannelOpen));
        assert!(outcomes.contains(&ContainmentState::InternetAccess));
        assert!(outcomes.contains(&ContainmentState::CredentialsObtained));
        assert!(outcomes.contains(&ContainmentState::ExternalCompromise));
    }

    #[test]
    fn vulnerable_shared_service_bypasses_all_boundaries() {
        let system = ContainmentSystem::new(Scenario::SharedServiceBypass);
        let profile = JointAction::new([
            Action::Attack,
            Action::Attack,
            Action::Relay,
            Action::Enforce,
            Action::Deny,
            Action::Protect,
            Action::Block,
        ])
        .expect("valid profile");
        assert_eq!(
            system.transition(ContainmentState::Start, &profile),
            ContainmentState::ExternalCompromise
        );
    }

    #[test]
    fn derivation_matches_independent_bruteforce_for_every_state_coalition_and_target() {
        for scenario in [Scenario::Hardened, Scenario::SharedServiceBypass] {
            let system = ContainmentSystem::new(scenario);
            let derived = system.derive_effectivity();
            let profiles = system.all_joint_actions();
            for state in ContainmentState::ALL {
                for coalition in Coalition::all(Agent::ALL.len()) {
                    let strategies = brute_strategies(&profiles, &coalition);
                    for mask in 0..(1usize << ContainmentState::ALL.len()) {
                        let target = StateSet::from_states(
                            ContainmentState::ALL
                                .into_iter()
                                .enumerate()
                                .filter_map(|(i, s)| (mask & (1 << i) != 0).then_some(s.id())),
                        );
                        let brute = strategies.iter().any(|strategy| {
                            all_outcomes(system, &profiles, state, strategy).is_subset(&target)
                        });
                        assert_eq!(
                            derived
                                .effectivity()
                                .can_enforce(state.id(), &coalition, &target),
                            brute,
                            "scenario={scenario:?}, state={state:?}, coalition={coalition:?}, mask={mask}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn strategic_witness_exposes_hardened_channel_guarantee_without_internet_guarantee() {
        let system = ContainmentSystem::new(Scenario::Hardened);
        let derived = system.derive_effectivity();
        let coalition = Coalition::from_agents([
            Agent::AgentA.id(),
            Agent::AgentB.id(),
            Agent::SharedService.id(),
        ]);
        let channel = StateSet::from_states(
            ContainmentState::ALL
                .into_iter()
                .filter(|state| {
                    matches!(
                        state,
                        ContainmentState::ChannelOpen
                            | ContainmentState::InternetAccess
                            | ContainmentState::CredentialsObtained
                            | ContainmentState::ExternalCompromise
                    )
                })
                .map(ContainmentState::id),
        );
        let internet = StateSet::from_states(
            ContainmentState::ALL
                .into_iter()
                .filter(|state| {
                    matches!(
                        state,
                        ContainmentState::InternetAccess
                            | ContainmentState::CredentialsObtained
                            | ContainmentState::ExternalCompromise
                    )
                })
                .map(ContainmentState::id),
        );
        assert!(derived.effectivity().can_enforce(
            ContainmentState::Start.id(),
            &coalition,
            &channel
        ));
        assert!(!derived.effectivity().can_enforce(
            ContainmentState::Start.id(),
            &coalition,
            &internet
        ));
    }

    #[test]
    fn qcl_audit_differs_between_scenarios() {
        let hardened = run_audit(Scenario::Hardened);
        let bypass = run_audit(Scenario::SharedServiceBypass);
        assert!(hardened.contains("PASS !<subset({agent_a, agent_b, shared_service})> internet"));
        assert!(bypass.contains("FAIL !<subset({agent_a, agent_b, shared_service})> internet"));
        assert!(bypass.contains("FAIL !<excludes(egress_gateway)> internet"));
    }
}
