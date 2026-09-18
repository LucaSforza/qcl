//! Observed game form extracted from executable loopback lab runs.
//!
//! This module intentionally contains no containment transition rules.  It
//! configures and runs [`ContainmentLab`], records observations, then exposes
//! only table lookup to the generic game-form derivation.

#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use qcl::ast::Formula;
use qcl::checker::ModelChecker;
use qcl::domain::{AgentId, Coalition, StateId, StateSet};
use qcl::game_form::{self, FiniteGameForm, JointActionProfile};
use qcl::model::QclModel;
use qcl::parser::parse_formula;
use qcl::predicate::PredicateProgram;
use qcl::symbols::SymbolTable;

use crate::lab::{ContainmentLab, LabConfig, LabError, LabScenario, ObservedOutcome};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LiveAgent {
    AgentA,
    AgentB,
    SharedService,
    EgressGateway,
    CredentialBroker,
}

impl LiveAgent {
    pub const ALL: [Self; 5] = [
        Self::AgentA,
        Self::AgentB,
        Self::SharedService,
        Self::EgressGateway,
        Self::CredentialBroker,
    ];

    #[must_use]
    pub const fn id(self) -> AgentId {
        AgentId::new(self as usize)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AgentA => "agent_a",
            Self::AgentB => "agent_b",
            Self::SharedService => "shared_service",
            Self::EgressGateway => "egress_gateway",
            Self::CredentialBroker => "credential_broker",
        }
    }
}

impl fmt::Display for LiveAgent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LiveAction {
    Idle,
    Attack,
    Normal,
    Fetch,
    Deny,
    Allow,
    Protect,
    Expose,
}

impl fmt::Display for LiveAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Idle => "idle",
            Self::Attack => "attack",
            Self::Normal => "normal",
            Self::Fetch => "fetch",
            Self::Deny => "deny",
            Self::Allow => "allow",
            Self::Protect => "protect",
            Self::Expose => "expose",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LiveJointAction([LiveAction; 5]);

impl LiveJointAction {
    #[must_use]
    pub const fn new(actions: [LiveAction; 5]) -> Self {
        Self(actions)
    }

    #[must_use]
    pub const fn action(self, agent: LiveAgent) -> LiveAction {
        self.0[agent as usize]
    }

    #[must_use]
    fn from_profile(profile: &JointActionProfile<LiveAction>) -> Self {
        Self(LiveAgent::ALL.map(|agent| {
            *profile
                .action(agent.id())
                .expect("derived game profile is complete")
        }))
    }

    #[must_use]
    fn config(self) -> LabConfig {
        LabConfig {
            agent_a_attack: self.action(LiveAgent::AgentA) == LiveAction::Attack,
            agent_b_attack: self.action(LiveAgent::AgentB) == LiveAction::Attack,
            shared_fetch: self.action(LiveAgent::SharedService) == LiveAction::Fetch,
            egress_allow: self.action(LiveAgent::EgressGateway) == LiveAction::Allow,
            broker_expose: self.action(LiveAgent::CredentialBroker) == LiveAction::Expose,
        }
    }

    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut profiles = Vec::with_capacity(32);
        for agent_a in [LiveAction::Idle, LiveAction::Attack] {
            for agent_b in [LiveAction::Idle, LiveAction::Attack] {
                for shared in [LiveAction::Normal, LiveAction::Fetch] {
                    for egress in [LiveAction::Deny, LiveAction::Allow] {
                        for broker in [LiveAction::Protect, LiveAction::Expose] {
                            profiles.push(Self::new([agent_a, agent_b, shared, egress, broker]));
                        }
                    }
                }
            }
        }
        profiles
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LiveState {
    Start,
    Contained,
    ChannelOpen,
    ExternalAccess,
    CredentialObtained,
    SecretExfiltrated,
}

impl LiveState {
    pub const ALL: [Self; 6] = [
        Self::Start,
        Self::Contained,
        Self::ChannelOpen,
        Self::ExternalAccess,
        Self::CredentialObtained,
        Self::SecretExfiltrated,
    ];

    #[must_use]
    pub const fn id(self) -> StateId {
        StateId::new(self as usize)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Contained => "contained",
            Self::ChannelOpen => "channel_open",
            Self::ExternalAccess => "external_access",
            Self::CredentialObtained => "credential_obtained",
            Self::SecretExfiltrated => "secret_exfiltrated",
        }
    }

    fn from_observation(observed: &ObservedOutcome) -> Self {
        match observed.state_name() {
            "secret_exfiltrated" => Self::SecretExfiltrated,
            "credential_obtained" => Self::CredentialObtained,
            "external_access" => Self::ExternalAccess,
            "channel_open" => Self::ChannelOpen,
            "contained" => Self::Contained,
            _ => unreachable!("lab only emits documented observation classes"),
        }
    }
}

impl fmt::Display for LiveState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedTransition {
    pub state: LiveState,
    pub observation: ObservedOutcome,
}

/// Executions observed at `start`; terminal self-loops are finite-model
/// bookkeeping entries added after extraction, never containment decisions.
#[derive(Clone, Debug)]
pub struct ObservedTransitionTable {
    observations: HashMap<LiveJointAction, ObservedTransition>,
    transitions: HashMap<(StateId, LiveJointAction), StateId>,
}

impl ObservedTransitionTable {
    /// Exhaustively execute each full profile against one local lab instance.
    pub fn extract(scenario: LiveScenario) -> Result<Self, LabError> {
        let mut lab = ContainmentLab::start(scenario.into())?;
        let mut observations = HashMap::new();
        let mut transitions = HashMap::new();
        for profile in LiveJointAction::all() {
            let observation = lab.execute_profile(profile.config())?;
            let state = LiveState::from_observation(&observation);
            transitions.insert((LiveState::Start.id(), profile), state.id());
            observations.insert(profile, ObservedTransition { state, observation });
        }
        for state in LiveState::ALL
            .into_iter()
            .filter(|state| *state != LiveState::Start)
        {
            for profile in LiveJointAction::all() {
                transitions.insert((state.id(), profile), state.id());
            }
        }
        Ok(Self {
            observations,
            transitions,
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.observations.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    #[must_use]
    pub fn observation(&self, profile: LiveJointAction) -> Option<&ObservedTransition> {
        self.observations.get(&profile)
    }

    #[must_use]
    pub fn transition(&self, state: StateId, profile: LiveJointAction) -> StateId {
        *self
            .transitions
            .get(&(state, profile))
            .expect("observed transition table covers finite action/state space")
    }

    #[must_use]
    pub fn counts(&self) -> HashMap<LiveState, usize> {
        let mut counts = HashMap::new();
        for transition in self.observations.values() {
            *counts.entry(transition.state).or_insert(0) += 1;
        }
        counts
    }
}

#[derive(Clone, Debug)]
pub struct ObservedGameForm {
    table: ObservedTransitionTable,
}

impl ObservedGameForm {
    #[must_use]
    pub const fn new(table: ObservedTransitionTable) -> Self {
        Self { table }
    }

    #[must_use]
    pub const fn table(&self) -> &ObservedTransitionTable {
        &self.table
    }
}

impl FiniteGameForm for ObservedGameForm {
    type Action = LiveAction;

    fn agent_count(&self) -> usize {
        LiveAgent::ALL.len()
    }

    fn state_count(&self) -> usize {
        LiveState::ALL.len()
    }

    fn actions(&self, _state: StateId, agent: AgentId) -> Vec<Self::Action> {
        match LiveAgent::ALL[agent.index()] {
            LiveAgent::AgentA | LiveAgent::AgentB => vec![LiveAction::Idle, LiveAction::Attack],
            LiveAgent::SharedService => vec![LiveAction::Normal, LiveAction::Fetch],
            LiveAgent::EgressGateway => vec![LiveAction::Deny, LiveAction::Allow],
            LiveAgent::CredentialBroker => vec![LiveAction::Protect, LiveAction::Expose],
        }
    }

    fn transition(&self, state: StateId, profile: &JointActionProfile<Self::Action>) -> StateId {
        self.table
            .transition(state, LiveJointAction::from_profile(profile))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LiveScenario {
    Hardened,
    SharedServiceFetch,
}

impl From<LiveScenario> for LabScenario {
    fn from(value: LiveScenario) -> Self {
        match value {
            LiveScenario::Hardened => Self::Hardened,
            LiveScenario::SharedServiceFetch => Self::SharedServiceFetch,
        }
    }
}

impl fmt::Display for LiveScenario {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Hardened => "hardened",
            Self::SharedServiceFetch => "shared-service-fetch",
        })
    }
}

impl FromStr for LiveScenario {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "hardened" => Ok(Self::Hardened),
            "shared-service-fetch" => Ok(Self::SharedServiceFetch),
            _ => Err(format!("unknown live scenario `{value}`")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LiveDerivedModel {
    pub model: QclModel,
    pub derived: game_form::DerivedEffectivity<LiveAction>,
}

impl ObservedGameForm {
    #[must_use]
    pub fn qcl_model(&self) -> LiveDerivedModel {
        let derived = game_form::derive_effectivity(self);
        let mut agents = SymbolTable::with_capacity(LiveAgent::ALL.len());
        let mut states = SymbolTable::with_capacity(LiveState::ALL.len());
        let mut atoms = SymbolTable::with_capacity(6);
        for agent in LiveAgent::ALL {
            agents.insert(agent.name()).expect("unique live agent name");
        }
        for state in LiveState::ALL {
            states.insert(state.name()).expect("unique live state name");
        }
        for atom in [
            "safe",
            "channel",
            "external_access",
            "credential",
            "exfiltrated",
            "loss_of_containment",
        ] {
            atoms.insert(atom).expect("unique live atom name");
        }
        let mut valuation = HashMap::new();
        for state in LiveState::ALL {
            let names = match state {
                LiveState::Start => Vec::new(),
                LiveState::Contained => vec!["safe"],
                LiveState::ChannelOpen => vec!["channel"],
                LiveState::ExternalAccess => {
                    vec!["channel", "external_access", "loss_of_containment"]
                }
                LiveState::CredentialObtained => {
                    vec!["channel", "credential", "loss_of_containment"]
                }
                LiveState::SecretExfiltrated => vec![
                    "channel",
                    "external_access",
                    "credential",
                    "exfiltrated",
                    "loss_of_containment",
                ],
            };
            valuation.insert(
                state.id(),
                names
                    .into_iter()
                    .map(|name| atoms.lookup(name).expect("known live atom"))
                    .collect(),
            );
        }
        LiveDerivedModel {
            model: QclModel::new(
                agents,
                states,
                atoms,
                valuation,
                derived.as_effectivity().clone(),
            ),
            derived,
        }
    }
}

const LIVE_FORMULAS: [&str; 6] = [
    "<subset({agent_a, agent_b, shared_service})> channel",
    "!<subset({agent_a, agent_b, shared_service})> external_access",
    "!<!(size >= 3)> exfiltrated",
    "!<excludes(egress_gateway)> external_access",
    "!<excludes(credential_broker)> exfiltrated",
    "!<subset({agent_a, agent_b, shared_service})> exfiltrated",
];

pub fn run_live_extract(scenario: LiveScenario) -> Result<String, LabError> {
    let table = ObservedTransitionTable::extract(scenario)?;
    let counts = table.counts();
    let mut lines = vec![
        format!("scenario: {scenario}"),
        format!("profiles executed: {}", table.len()),
    ];
    for state in [
        LiveState::Contained,
        LiveState::ChannelOpen,
        LiveState::ExternalAccess,
        LiveState::CredentialObtained,
        LiveState::SecretExfiltrated,
    ] {
        lines.push(format!(
            "{}: {}",
            state.name(),
            counts.get(&state).copied().unwrap_or(0)
        ));
    }
    Ok(lines.join("\n"))
}

pub fn run_live_audit(scenario: LiveScenario) -> Result<String, LabError> {
    let table = ObservedTransitionTable::extract(scenario)?;
    let game = ObservedGameForm::new(table);
    let derived = game.qcl_model();
    let checker = ModelChecker::new(&derived.model);
    let mut lines = vec![
        "QCL audit of observed local containment lab".to_owned(),
        format!("backend: live\nscenario: {scenario}"),
        format!("agents: {}", LiveAgent::ALL.len()),
        format!(
            "full joint-action profiles observed: {}",
            game.table().len()
        ),
        String::new(),
    ];
    for source in LIVE_FORMULAS {
        let formula = parse_formula(source)
            .expect("static live audit formula parses")
            .resolve(&derived.model.agents, &derived.model.atoms)
            .expect("static live audit formula resolves");
        let passed = checker
            .check(LiveState::Start.id(), &formula)
            .expect("static formula ids valid");
        lines.push(format!("{} {source}", if passed { "PASS" } else { "FAIL" }));
        if !passed {
            lines.extend(observed_counterexample(
                source,
                &formula,
                &derived,
                game.table(),
            ));
        }
    }
    Ok(lines.join("\n"))
}

fn observed_counterexample(
    source: &str,
    formula: &Formula,
    derived: &LiveDerivedModel,
    table: &ObservedTransitionTable,
) -> Vec<String> {
    let Formula::Not(child) = formula else {
        return vec![format!("counterexample for {source}: unsupported form")];
    };
    let Formula::Exists { predicate, formula } = child.as_ref() else {
        return vec![format!("counterexample for {source}: unsupported form")];
    };
    let target = ModelChecker::new(&derived.model)
        .satisfying_states(formula)
        .expect("resolved static formula has valid ids");
    let mut coalitions: Vec<_> = Coalition::all(LiveAgent::ALL.len())
        .filter(|coalition| PredicateProgram::compile(predicate).evaluate(coalition))
        .collect();
    coalitions.sort_by_key(|coalition| (coalition.len(), coalition.iter().collect::<Vec<_>>()));
    for coalition in coalitions {
        if let Some(witness) =
            derived
                .derived
                .find_witness(LiveState::Start.id(), &coalition, &target)
        {
            let profile =
                LiveJointAction::all().into_iter().find(|profile| {
                    witness.strategy.iter().all(|(agent, action)| {
                        profile.action(LiveAgent::ALL[agent.index()]) == *action
                    }) && table
                        .observation(*profile)
                        .is_some_and(|entry| witness.outcomes.contains(entry.state.id()))
                });
            let evidence = profile
                .and_then(|profile| table.observation(profile))
                .map_or_else(|| "no observation found".to_owned(), observation_evidence);
            return vec![
                format!("counterexample for {source}"),
                "positive existential witness (negated existential is false)".to_owned(),
                format!("coalition: {}", format_coalition(&coalition)),
                format!("strategy: {}", format_strategy(witness.strategy.iter())),
                format!(
                    "possible observed outcomes: {}",
                    format_states(&witness.outcomes)
                ),
                format!("evidence: {evidence}"),
            ];
        }
    }
    vec![format!("counterexample for {source}: no witness found")]
}

fn observation_evidence(transition: &ObservedTransition) -> String {
    if let Some(event) = transition.observation.target_events.first() {
        format!(
            "external_target event {} (fixture secret received: {})",
            event.route, event.contained_secret
        )
    } else if transition.observation.credential_obtained {
        "credential_broker returned fixture secret to client".to_owned()
    } else if transition.observation.channel_established {
        "shared_service /messages contained agent message".to_owned()
    } else {
        "service event logs empty".to_owned()
    }
}

fn format_coalition(coalition: &Coalition) -> String {
    let names = coalition
        .iter()
        .map(|agent| LiveAgent::ALL[agent.index()].name())
        .collect::<Vec<_>>();
    format!("{{{}}}", names.join(", "))
}

fn format_strategy<'a>(actions: impl Iterator<Item = (AgentId, &'a LiveAction)>) -> String {
    actions
        .map(|(agent, action)| format!("{} = {action}", LiveAgent::ALL[agent.index()]))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_states(states: &StateSet) -> String {
    let names = states
        .iter()
        .filter_map(|state| LiveState::ALL.get(state.index()))
        .map(|state| state.name())
        .collect::<Vec<_>>();
    format!("{{{}}}", names.join(", "))
}

pub fn run_live_demo(scenario: LiveScenario) -> Result<String, LabError> {
    let mut lab = ContainmentLab::start(scenario.into())?;
    let observed = lab.execute_profile(LabConfig {
        agent_a_attack: true,
        agent_b_attack: false,
        shared_fetch: true,
        egress_allow: false,
        broker_expose: false,
    })?;
    let state = LiveState::from_observation(&observed);
    let evidence = observation_evidence(&ObservedTransition {
        state,
        observation: observed,
    });
    Ok(format!(
        "live demo ({scenario})\nagent_a -> shared_service /fetch -> external_target\nobserved state: {state}\nevidence: {evidence}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qcl::model::ModelValidator;

    #[test]
    fn table_has_every_full_profile_and_terminal_lookups() {
        let table = ObservedTransitionTable::extract(LiveScenario::Hardened).expect("lab works");
        assert_eq!(table.len(), 32);
        let game = ObservedGameForm::new(table);
        let profile = JointActionProfile::new(vec![
            LiveAction::Idle,
            LiveAction::Idle,
            LiveAction::Normal,
            LiveAction::Deny,
            LiveAction::Protect,
        ]);
        assert_eq!(
            game.transition(LiveState::Start.id(), &profile),
            LiveState::Contained.id()
        );
        assert_eq!(
            game.transition(LiveState::ChannelOpen.id(), &profile),
            LiveState::ChannelOpen.id()
        );
    }

    #[test]
    fn observed_game_form_derives_valid_effectivity() {
        let game = ObservedGameForm::new(
            ObservedTransitionTable::extract(LiveScenario::Hardened).expect("lab works"),
        );
        let model = game.qcl_model();
        ModelValidator::validate(&model.model).expect("derived finite game is weakly playable");
    }

    #[test]
    fn same_egress_formula_changes_only_with_real_shared_fetch_capability() {
        let hardened = run_live_audit(LiveScenario::Hardened).expect("hardened audit");
        let vulnerable =
            run_live_audit(LiveScenario::SharedServiceFetch).expect("vulnerable audit");
        let formula = "!<excludes(egress_gateway)> external_access";
        assert!(hardened.contains(&format!("PASS {formula}")));
        assert!(vulnerable.contains(&format!("FAIL {formula}")));
        assert!(vulnerable.contains("coalition: {agent_a, shared_service}"));
        assert!(vulnerable.contains("external_target event /exfiltrate"));
    }
}
