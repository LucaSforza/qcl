//! Deterministic, offline zero-trust control-plane audit built on QCL.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use qcl::ast::Formula;
use qcl::checker::ModelChecker;
use qcl::domain::{AgentId, Coalition, StateId, StateSet};
use qcl::model::{Effectivity, QclModel};
use qcl::parser::parse_formula;
use qcl::predicate::PredicateProgram;
use qcl::symbols::SymbolTable;

const PRINCIPALS: [PrincipalMetadata; 11] = [
    PrincipalMetadata::new("llm_ops", "operations", true, &["propose_change"]),
    PrincipalMetadata::new("deployer_sa", "operations", true, &["create_workload"]),
    PrincipalMetadata::new("autoscaler", "operations", true, &["scale_workload"]),
    PrincipalMetadata::new(
        "backup_controller",
        "resilience",
        true,
        &["restore_or_delete"],
    ),
    PrincipalMetadata::new("network_controller", "operations", true, &["modify_egress"]),
    PrincipalMetadata::new(
        "secrets_operator",
        "security",
        true,
        &["materialize_secret"],
    ),
    PrincipalMetadata::new("cloud_iam", "cloud", true, &["modify_workload_identity"]),
    PrincipalMetadata::new(
        "admission_controller",
        "security",
        true,
        &["admit_safe_change"],
    ),
    PrincipalMetadata::new("human_sre", "human", false, &["operate_change"]),
    PrincipalMetadata::new(
        "security_approver",
        "human",
        false,
        &["approve_security_change"],
    ),
    PrincipalMetadata::new("break_glass", "emergency", false, &["emergency_override"]),
];

const OUTCOMES: [&str; 6] = [
    "normal",
    "safe_change",
    "data_deleted",
    "secret_exfiltrated",
    "cluster_root",
    "policy_bypassed",
];

/// Metadata kept in showcase only; QCL formulas still quantify explicit sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrincipalMetadata {
    /// Stable principal name used in QCL formulas.
    pub name: &'static str,
    /// Human-readable boundary, not a QCL primitive.
    pub trust_domain: &'static str,
    /// Whether principal is an automated component.
    pub automated: bool,
    /// Abstract capabilities used while deriving capability routes.
    pub capabilities: &'static [&'static str],
}

impl PrincipalMetadata {
    const fn new(
        name: &'static str,
        trust_domain: &'static str,
        automated: bool,
        capabilities: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            trust_domain,
            automated,
            capabilities,
        }
    }
}

/// Available deterministic synthetic configurations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditScenario {
    /// Capability routes retain human/security vetoes.
    Secure,
    /// Automatic deployment plus cloud identity guarantee root.
    PrivilegeEscalation,
    /// Emergency identity bypasses ordinary gates.
    BreakGlassBypass,
}

impl AuditScenario {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Secure => "secure",
            Self::PrivilegeEscalation => "privilege-escalation",
            Self::BreakGlassBypass => "break-glass-bypass",
        }
    }
}

impl FromStr for AuditScenario {
    type Err = AuditError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "secure" => Ok(Self::Secure),
            "privilege-escalation" => Ok(Self::PrivilegeEscalation),
            "break-glass-bypass" => Ok(Self::BreakGlassBypass),
            _ => Err(AuditError::UnknownScenario(value.to_owned())),
        }
    }
}

/// Audit construction or checking error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditError {
    /// CLI named no supported audit configuration.
    UnknownScenario(String),
    /// Static formula unexpectedly failed to parse or resolve.
    Formula(String),
}

impl fmt::Display for AuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownScenario(name) => write!(formatter, "unknown audit scenario `{name}`"),
            Self::Formula(message) => write!(formatter, "audit formula error: {message}"),
        }
    }
}

impl std::error::Error for AuditError {}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CapabilityRoute {
    required: &'static [&'static str],
    outcome: &'static str,
    explanation: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuditRuleKind {
    Prohibition,
    Universal,
    Requirement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AuditRule {
    source: &'static str,
    kind: AuditRuleKind,
}

/// One formula result and, on failure, one concrete coalition witness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditResult {
    /// Exact parsed-and-resolved QCL source.
    pub formula: &'static str,
    /// Whether formula holds at `normal`.
    pub passed: bool,
    /// Concrete failing coalition, if formula failed.
    pub counterexample: Option<Counterexample>,
}

/// Explainable local diagnostic; core checker remains boolean-oriented.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Counterexample {
    /// Coalition selected by formula predicate.
    pub coalition: Vec<String>,
    /// Guaranteed outcome, or `None` when universal guarantee is missing.
    pub enforced_outcome: Option<String>,
    /// Capability-route reason.
    pub explanation: String,
}

/// Fully built offline model and results for one scenario.
#[derive(Clone, Debug)]
pub struct AuditReport {
    scenario: AuditScenario,
    principals: &'static [PrincipalMetadata],
    model: QclModel,
    routes: Vec<CapabilityRoute>,
    normal: StateId,
    results: Vec<AuditResult>,
}

impl AuditReport {
    /// Build model, parse and resolve audit formulas, then model-check them.
    ///
    /// # Errors
    ///
    /// Returns an error only if a static audit formula is malformed.
    pub fn build(scenario: AuditScenario) -> Result<Self, AuditError> {
        let routes = routes_for(scenario);
        let (model, normal) = build_model(&routes);
        let mut report = Self {
            scenario,
            principals: &PRINCIPALS,
            model,
            routes,
            normal,
            results: Vec::new(),
        };
        report.results = rules_for(scenario)
            .iter()
            .map(|rule| report.evaluate_rule(rule))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(report)
    }

    /// Return scenario configuration.
    #[must_use]
    pub const fn scenario(&self) -> AuditScenario {
        self.scenario
    }

    /// Return results in deterministic display order.
    #[must_use]
    pub fn results(&self) -> &[AuditResult] {
        &self.results
    }

    /// Return number of principals considered by QCL.
    #[must_use]
    pub fn agent_count(&self) -> usize {
        self.model.agent_count()
    }

    /// Return number of enumerated coalitions.
    #[must_use]
    pub fn coalition_count(&self) -> usize {
        Coalition::all(self.model.agent_count()).count()
    }

    /// Render human-readable, deterministic offline audit output.
    #[must_use]
    pub fn render(&self) -> String {
        let mut lines = vec![
            format!(
                "Kubernetes/cloud zero-trust control-plane audit: {}",
                self.scenario.name()
            ),
            "offline deterministic model; no LLM, Kubernetes, or cloud API contacted".to_owned(),
            "principals:".to_owned(),
        ];
        lines.extend(self.principals.iter().map(|principal| {
            format!(
                "- {} [{}; {}]",
                principal.name,
                principal.trust_domain,
                if principal.automated {
                    "automated"
                } else {
                    "human/emergency"
                }
            )
        }));
        for result in &self.results {
            lines.push(String::new());
            lines.push(format!(
                "{}  {}",
                if result.passed { "PASS" } else { "FAIL" },
                result.formula
            ));
            if let Some(counterexample) = &result.counterexample {
                lines.push("counterexample coalition:".to_owned());
                lines.push(format!("{{{}}}", counterexample.coalition.join(", ")));
                if let Some(outcome) = &counterexample.enforced_outcome {
                    lines.push(format!("enforced outcome: {outcome}"));
                } else {
                    lines.push("enforced outcome: none".to_owned());
                }
                lines.push(format!("explanation: {}", counterexample.explanation));
            }
        }
        lines.push(String::new());
        lines.push(format!(
            "analyzed: {} agents, {} coalitions",
            self.agent_count(),
            self.coalition_count()
        ));
        lines.join("\n")
    }

    fn evaluate_rule(&self, rule: &AuditRule) -> Result<AuditResult, AuditError> {
        let formula = parse_formula(rule.source)
            .map_err(|error| AuditError::Formula(error.to_string()))?
            .resolve(&self.model.agents, &self.model.atoms)
            .map_err(|error| AuditError::Formula(error.to_string()))?;
        let passed = ModelChecker::new(&self.model)
            .check(self.normal, &formula)
            .map_err(|error| AuditError::Formula(error.to_string()))?;
        let counterexample = (!passed)
            .then(|| self.find_counterexample(&formula, rule.kind))
            .flatten();
        Ok(AuditResult {
            formula: rule.source,
            passed,
            counterexample,
        })
    }

    fn find_counterexample(
        &self,
        formula: &Formula,
        kind: AuditRuleKind,
    ) -> Option<Counterexample> {
        let (predicate, target, universal) = match formula {
            Formula::Not(inner) if kind == AuditRuleKind::Prohibition => match inner.as_ref() {
                Formula::Exists { predicate, formula } => (predicate, formula, false),
                _ => return None,
            },
            Formula::Forall { predicate, formula } if universal_kind(kind) => {
                (predicate, formula, true)
            }
            Formula::Exists { predicate, formula } if kind == AuditRuleKind::Requirement => {
                (predicate, formula, false)
            }
            _ => return None,
        };
        let program = PredicateProgram::compile(predicate);
        let target_states = ModelChecker::new(&self.model)
            .satisfying_states(target)
            .ok()?;
        for coalition in Coalition::all(self.model.agent_count()) {
            if !program.evaluate(&coalition) {
                continue;
            }
            let can_enforce =
                self.model
                    .effectivity
                    .can_enforce(self.normal, &coalition, &target_states);
            if (!universal && can_enforce) || (universal && !can_enforce) {
                let route = self.route_for(&coalition, &target_states);
                return Some(Counterexample {
                    coalition: coalition_names(&self.model, &coalition),
                    enforced_outcome: (!universal).then(|| {
                        route.map_or_else(|| "unknown".to_owned(), |entry| entry.outcome.to_owned())
                    }),
                    explanation: if universal {
                        "matching coalition has no route that guarantees required outcome"
                            .to_owned()
                    } else {
                        route.map_or_else(
                            || "coalition has a dangerous guaranteed outcome".to_owned(),
                            |entry| entry.explanation.to_owned(),
                        )
                    },
                });
            }
        }
        None
    }

    fn route_for(&self, coalition: &Coalition, target: &StateSet) -> Option<&CapabilityRoute> {
        self.routes.iter().find(|route| {
            let outcome = self.model.states.lookup(route.outcome).ok();
            outcome.is_some_and(|state| target.contains(state))
                && route_required(route, &self.model).is_subset(coalition)
        })
    }
}

const fn universal_kind(kind: AuditRuleKind) -> bool {
    matches!(kind, AuditRuleKind::Universal)
}

/// Run and render one offline audit scenario.
///
/// # Errors
///
/// Returns an error for an invalid static audit formula.
pub fn run(scenario: AuditScenario) -> Result<String, AuditError> {
    Ok(AuditReport::build(scenario)?.render())
}

fn rules_for(scenario: AuditScenario) -> Vec<AuditRule> {
    let mut rules = vec![
        AuditRule {
            source: "!<subset({llm_ops, deployer_sa, autoscaler, backup_controller, network_controller, secrets_operator, cloud_iam})> root",
            kind: AuditRuleKind::Prohibition,
        },
        AuditRule {
            source: "!<excludes(human_sre)> deleted",
            kind: AuditRuleKind::Prohibition,
        },
        AuditRule {
            source: "!<excludes(security_approver)> exfiltrated",
            kind: AuditRuleKind::Prohibition,
        },
        AuditRule {
            source: "!<subset({llm_ops, deployer_sa, network_controller, secrets_operator})> exfiltrated",
            kind: AuditRuleKind::Prohibition,
        },
        AuditRule {
            source: "<superset({human_sre, security_approver, admission_controller})> safe",
            kind: AuditRuleKind::Requirement,
        },
        AuditRule {
            source: "[superset({human_sre, security_approver, admission_controller})] safe",
            kind: AuditRuleKind::Universal,
        },
        AuditRule {
            source: "!<includes(break_glass)> bypassed",
            kind: AuditRuleKind::Prohibition,
        },
    ];
    if scenario == AuditScenario::BreakGlassBypass {
        rules.push(AuditRule {
            source: "!<excludes(security_approver)> root",
            kind: AuditRuleKind::Prohibition,
        });
    }
    rules
}

fn routes_for(scenario: AuditScenario) -> Vec<CapabilityRoute> {
    let mut routes = vec![
        CapabilityRoute {
            required: &["human_sre", "security_approver", "admission_controller"],
            outcome: "safe_change",
            explanation: "human SRE, security approver, and admission controller jointly guarantee safe change",
        },
        CapabilityRoute {
            required: &["human_sre", "backup_controller"],
            outcome: "data_deleted",
            explanation: "human SRE authorizes backup-controller deletion route",
        },
        CapabilityRoute {
            required: &[
                "security_approver",
                "secrets_operator",
                "network_controller",
            ],
            outcome: "secret_exfiltrated",
            explanation: "security approval, Secret materialization, and egress jointly enable exfiltration",
        },
        // Possible is not enforceable: normal remains possible alongside root.
    ];
    match scenario {
        AuditScenario::Secure => {}
        AuditScenario::PrivilegeEscalation => routes.push(CapabilityRoute {
            required: &["llm_ops", "deployer_sa", "cloud_iam"],
            outcome: "cluster_root",
            explanation: "coalition can create privileged workload and obtain cloud identity with control-plane privileges",
        }),
        AuditScenario::BreakGlassBypass => routes.extend([
            CapabilityRoute { required: &["break_glass"], outcome: "cluster_root", explanation: "break-glass identity bypasses admission and obtains control-plane root" },
            CapabilityRoute { required: &["break_glass"], outcome: "data_deleted", explanation: "break-glass identity executes deletion without human SRE" },
            CapabilityRoute { required: &["break_glass"], outcome: "secret_exfiltrated", explanation: "break-glass identity bypasses security approval for Secret access" },
            CapabilityRoute { required: &["break_glass"], outcome: "policy_bypassed", explanation: "break-glass identity modifies or bypasses admission policy" },
        ]),
    }
    routes
}

fn build_model(routes: &[CapabilityRoute]) -> (QclModel, StateId) {
    let mut agents = SymbolTable::new();
    for principal in PRINCIPALS {
        agents
            .insert(principal.name)
            .expect("static unique principal");
    }
    let mut states = SymbolTable::new();
    for outcome in OUTCOMES {
        states.insert(outcome).expect("static unique outcome");
    }
    let mut atoms = SymbolTable::new();
    for atom in ["safe", "deleted", "exfiltrated", "root", "bypassed"] {
        atoms.insert(atom).expect("static unique atom");
    }
    let mut valuation = HashMap::new();
    for (state_name, atom_name) in [
        ("safe_change", "safe"),
        ("data_deleted", "deleted"),
        ("secret_exfiltrated", "exfiltrated"),
        ("cluster_root", "root"),
        ("policy_bypassed", "bypassed"),
    ] {
        valuation.insert(
            states.lookup(state_name).expect("static state"),
            HashSet::from([atoms.lookup(atom_name).expect("static atom")]),
        );
    }
    let universe = StateSet::from_states(states.iter().map(|(id, _)| id));
    let normal = states.lookup("normal").expect("static normal state");
    let mut effectivity = Effectivity::new();
    for coalition in Coalition::all(agents.len()) {
        let applicable: Vec<_> = routes
            .iter()
            .filter(|route| route_required_from_agents(route, &agents).is_subset(&coalition))
            .collect();
        for state in states.iter().map(|(id, _)| id) {
            effectivity.insert(state, coalition.clone(), universe.clone());
            for route in &applicable {
                effectivity.insert(
                    state,
                    coalition.clone(),
                    StateSet::singleton(states.lookup(route.outcome).expect("static route state")),
                );
            }
            // Possibility differs from QCL enforcement: this outcome includes
            // both normal and root, so it is not contained in `{cluster_root}`.
            if coalition.contains(agents.lookup("autoscaler").expect("static autoscaler")) {
                effectivity.insert(
                    state,
                    coalition.clone(),
                    StateSet::from_states([
                        states.lookup("normal").expect("static normal state"),
                        states.lookup("cluster_root").expect("static root state"),
                    ]),
                );
            }
        }
    }
    (
        QclModel::new(agents, states, atoms, valuation, effectivity),
        normal,
    )
}

fn route_required(route: &CapabilityRoute, model: &QclModel) -> Coalition {
    Coalition::from_agents(
        route
            .required
            .iter()
            .map(|name| model.agents.lookup(name).expect("static route principal")),
    )
}

fn route_required_from_agents(route: &CapabilityRoute, agents: &SymbolTable<AgentId>) -> Coalition {
    Coalition::from_agents(
        route
            .required
            .iter()
            .map(|name| agents.lookup(name).expect("static route principal")),
    )
}

fn coalition_names(model: &QclModel, coalition: &Coalition) -> Vec<String> {
    coalition
        .iter()
        .map(|agent| model.agents.name(agent).unwrap_or("unknown").to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_formulas_parse_and_resolve() {
        let report = AuditReport::build(AuditScenario::Secure).expect("audit model");
        assert_eq!(report.results().len(), 7);
        assert!(report.results().iter().all(|result| result.passed));
    }

    #[test]
    fn secure_scenario_preserves_vetoes_and_safe_change() {
        let report = AuditReport::build(AuditScenario::Secure).expect("audit model");
        assert!(report.results().iter().all(|result| result.passed));
        assert_eq!(report.agent_count(), 11);
        assert_eq!(report.coalition_count(), 2048);
    }

    #[test]
    fn privilege_escalation_reports_concrete_automatic_root_coalition() {
        let report = AuditReport::build(AuditScenario::PrivilegeEscalation).expect("audit model");
        let result = &report.results()[0];
        assert!(!result.passed);
        assert_eq!(
            result.counterexample.as_ref().map(|item| &item.coalition),
            Some(&vec![
                "llm_ops".to_owned(),
                "deployer_sa".to_owned(),
                "cloud_iam".to_owned()
            ])
        );
        assert_eq!(
            result
                .counterexample
                .as_ref()
                .and_then(|item| item.enforced_outcome.as_deref()),
            Some("cluster_root")
        );
    }

    #[test]
    fn break_glass_bypass_breaks_vetoes() {
        let report = AuditReport::build(AuditScenario::BreakGlassBypass).expect("audit model");
        let failures = report
            .results()
            .iter()
            .filter(|result| !result.passed)
            .count();
        assert!(failures >= 4);
        assert!(report.render().contains("break_glass"));
    }

    #[test]
    fn ambiguous_outcome_does_not_guarantee_root() {
        let routes = Vec::new();
        let (model, normal) = build_model(&routes);
        let autoscaler = model.agents.lookup("autoscaler").expect("agent");
        let root = model.states.lookup("cluster_root").expect("state");
        assert!(!model.effectivity.can_enforce(
            normal,
            &Coalition::singleton(autoscaler),
            &StateSet::singleton(root)
        ));
    }
}
