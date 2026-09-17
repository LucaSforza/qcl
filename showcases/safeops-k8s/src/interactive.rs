use std::io::{self, BufRead, Write};

use crate::llm::provider_from_env;
use crate::{
    Action, Approval, Decision, Denial, DeploymentSnapshot, ExecutionGrant, ExecutionResult,
    KubectlAdapter, SafeOpsError, SafetyKernel, ToolIntent,
};

const MAX_TIMELINE_EVENTS: usize = 256;

/// Run the human-driven `SafeOps` terminal.
///
/// The terminal never executes after `plan`: `execute` is an explicit second
/// command, and only an in-memory kernel grant can reach the adapter.
///
/// # Errors
///
/// Returns an error when kernel initialization or terminal input fails.
pub fn run() -> Result<(), InteractiveError> {
    let mut kernel = SafetyKernel::new().map_err(InteractiveError::Kernel)?;
    let adapter = KubectlAdapter::new();
    let mut state = InteractiveState::default();
    let stdin = io::stdin();
    let mut input = String::new();

    println!("SafeOps interactive. Type `help` for commands; `quit` to exit.");
    loop {
        print!("safeops> ");
        io::stdout().flush().map_err(InteractiveError::Io)?;
        input.clear();
        if stdin
            .lock()
            .read_line(&mut input)
            .map_err(InteractiveError::Io)?
            == 0
        {
            break;
        }
        match parse_command(&input) {
            Ok(Command::Quit) => break,
            Ok(Command::Help) => println!("{}", help_text()),
            Ok(Command::Status) => match adapter.snapshot() {
                Ok(snapshot) => {
                    state.record_snapshot(snapshot.clone());
                    println!("{}", render_snapshot(&snapshot));
                }
                Err(error) => {
                    state.record(TimelineEvent::Failed {
                        stage: "snapshot.read",
                    });
                    print_error("status", &error);
                }
            },
            Ok(Command::Plan(goal)) => plan(&mut state, &mut kernel, &adapter, &goal),
            Ok(Command::Approve) => approve(&mut state, &mut kernel),
            Ok(Command::Execute) => execute(&mut state, &mut kernel, &adapter),
            Ok(Command::Discard) => {
                if state.pending.take().is_some() {
                    state.record(TimelineEvent::Discarded);
                } else {
                    println!("no pending proposal");
                }
            }
            Ok(Command::Timeline) => print_timeline(&state),
            Err(error) => println!("input error: {error}"),
        }
    }
    Ok(())
}

fn plan(
    state: &mut InteractiveState,
    kernel: &mut SafetyKernel,
    adapter: &KubectlAdapter,
    goal: &str,
) {
    state.begin_plan();
    let snapshot = match adapter.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            state.record(TimelineEvent::Failed {
                stage: "snapshot.read",
            });
            print_error("plan snapshot", &error);
            return;
        }
    };
    state.record_snapshot(snapshot.clone());
    state.record(TimelineEvent::LlmRequest);
    let provider = match provider_from_env() {
        Ok(provider) => provider,
        Err(error) => {
            state.record(TimelineEvent::Failed {
                stage: "llm.request",
            });
            print_error("plan provider", &error);
            return;
        }
    };
    let action = match provider.propose_action(&snapshot, goal) {
        Ok(action) => action,
        Err(error) => {
            state.record(TimelineEvent::Failed {
                stage: "llm.proposal",
            });
            print_error("plan provider", &error);
            return;
        }
    };
    state.record(TimelineEvent::Proposal(action.clone()));
    let intent = ToolIntent::new(crate::Principal::OperatorLlm, action, snapshot);
    match kernel.authorize(&intent, &[]) {
        Decision::Allow(grant) => {
            state.record(TimelineEvent::Decision {
                allowed: true,
                denial: None,
            });
            state.record(TimelineEvent::Grant(grant.intent().action().clone()));
            state.pending = Some(PendingProposal {
                intent,
                grant: Some(grant),
            });
            println!("proposal allowed; type `execute` to run it");
        }
        Decision::Deny(denial) => {
            let approval_needed = matches!(denial, Denial::MissingApproval { .. });
            state.record(TimelineEvent::Decision {
                allowed: false,
                denial: Some(denial.clone()),
            });
            if approval_needed {
                state.pending = Some(PendingProposal {
                    intent,
                    grant: None,
                });
                println!(
                    "proposal denied: {}. Type `approve` for human approval",
                    render_denial(&denial)
                );
            } else {
                println!("proposal denied: {}", render_denial(&denial));
            }
        }
    }
}

fn approve(state: &mut InteractiveState, kernel: &mut SafetyKernel) {
    let Some(mut pending) = state.pending.take() else {
        println!("no approval-gated proposal");
        return;
    };
    if !matches!(pending.intent.action(), Action::UpdateImage { .. }) || pending.grant.is_some() {
        state.pending = Some(pending);
        println!(
            "approval applies only to a pending update_image denied for missing human approval"
        );
        return;
    }
    state.record(TimelineEvent::HumanApproval);
    match kernel.authorize(&pending.intent, &[Approval::human_operator()]) {
        Decision::Allow(grant) => {
            state.record(TimelineEvent::Decision {
                allowed: true,
                denial: None,
            });
            state.record(TimelineEvent::Grant(grant.intent().action().clone()));
            pending.grant = Some(grant);
            println!("human approval accepted; type `execute` to run it");
        }
        Decision::Deny(denial) => {
            state.record(TimelineEvent::Decision {
                allowed: false,
                denial: Some(denial.clone()),
            });
            pending.grant = None;
            println!("approval rejected: {}", render_denial(&denial));
        }
    }
    state.pending = Some(pending);
}

fn execute(state: &mut InteractiveState, kernel: &mut SafetyKernel, adapter: &KubectlAdapter) {
    let Some(pending) = state.pending.take() else {
        println!("no approved proposal; run `plan` first");
        return;
    };
    let Some(grant) = pending.grant else {
        state.pending = Some(pending);
        println!("proposal has no grant; type `approve` if it is an update_image request");
        return;
    };
    let action = grant.intent().action().clone();
    state.record(TimelineEvent::ExecutionStarted {
        action: action.clone(),
    });
    match adapter.execute(kernel, grant) {
        Ok(result) => {
            state.record(TimelineEvent::ExecutionCompleted { action });
            let snapshot = match result {
                ExecutionResult::Observed(snapshot) => snapshot,
                ExecutionResult::Mutated { after, .. } => after,
            };
            state.record(TimelineEvent::PostSnapshot(snapshot.clone()));
            println!("execution completed; {}", render_snapshot(&snapshot));
        }
        Err(error) => {
            state.record(TimelineEvent::ExecutionFailed { action });
            println!("execution failed: {error}");
            // A grant is consumed before adapter side effects. Never restore it.
            if let Ok(snapshot) = adapter.snapshot() {
                state.record(TimelineEvent::PostSnapshot(snapshot));
            } else {
                state.record(TimelineEvent::Failed {
                    stage: "snapshot.post",
                });
            }
        }
    }
}

fn print_timeline(state: &InteractiveState) {
    if state.events.is_empty() {
        println!("timeline empty");
        return;
    }
    let first_sequence = state.next_sequence - state.events.len() as u64 + 1;
    for (offset, event) in state.events.iter().enumerate() {
        println!("{}. {}", first_sequence + offset as u64, event.render());
    }
}

fn print_error(context: &str, error: &impl std::fmt::Display) {
    println!("{context} failed: {error}");
}

fn help_text() -> &'static str {
    "status | plan <goal> | approve | execute | discard | timeline | help | quit"
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Status,
    Plan(String),
    Approve,
    Execute,
    Discard,
    Timeline,
    Help,
    Quit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ParseError {
    Empty,
    MissingGoal,
    UnknownCommand,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "enter a command",
            Self::MissingGoal => "plan requires a goal",
            Self::UnknownCommand => "unknown command; type `help`",
        })
    }
}

fn parse_command(line: &str) -> Result<Command, ParseError> {
    let line = line.trim();
    if line.is_empty() {
        return Err(ParseError::Empty);
    }
    let (name, rest) = line
        .split_once(' ')
        .map_or((line, ""), |(name, rest)| (name, rest.trim()));
    match name {
        "status" if rest.is_empty() => Ok(Command::Status),
        "plan" if !rest.is_empty() => Ok(Command::Plan(rest.to_owned())),
        "plan" => Err(ParseError::MissingGoal),
        "approve" if rest.is_empty() => Ok(Command::Approve),
        "execute" if rest.is_empty() => Ok(Command::Execute),
        "discard" if rest.is_empty() => Ok(Command::Discard),
        "timeline" if rest.is_empty() => Ok(Command::Timeline),
        "help" if rest.is_empty() => Ok(Command::Help),
        "quit" if rest.is_empty() => Ok(Command::Quit),
        _ => Err(ParseError::UnknownCommand),
    }
}

#[derive(Default)]
struct InteractiveState {
    pending: Option<PendingProposal>,
    events: Vec<TimelineEvent>,
    next_sequence: u64,
}

struct PendingProposal {
    intent: ToolIntent,
    grant: Option<ExecutionGrant>,
}

impl InteractiveState {
    fn begin_plan(&mut self) {
        if self.pending.take().is_some() {
            self.record(TimelineEvent::Discarded);
        }
    }

    fn record(&mut self, event: TimelineEvent) {
        let rendered = event.render();
        self.next_sequence = self.next_sequence.saturating_add(1);
        if self.events.len() == MAX_TIMELINE_EVENTS {
            self.events.remove(0);
        }
        self.events.push(event);
        println!("[{}] {rendered}", self.next_sequence);
    }

    fn record_snapshot(&mut self, snapshot: DeploymentSnapshot) {
        self.record(TimelineEvent::Snapshot(snapshot));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TimelineEvent {
    Snapshot(DeploymentSnapshot),
    LlmRequest,
    Proposal(Action),
    Decision {
        allowed: bool,
        denial: Option<Denial>,
    },
    Grant(Action),
    HumanApproval,
    ExecutionStarted {
        action: Action,
    },
    ExecutionCompleted {
        action: Action,
    },
    ExecutionFailed {
        action: Action,
    },
    PostSnapshot(DeploymentSnapshot),
    Failed {
        stage: &'static str,
    },
    Discarded,
}

impl TimelineEvent {
    fn render(&self) -> String {
        match self {
            Self::Snapshot(snapshot) => format!("snapshot: {}", render_snapshot(snapshot)),
            Self::LlmRequest => "llm request: started (goal=<redacted>)".to_owned(),
            Self::Proposal(action) => format!("typed proposal: {}", render_action(action)),
            Self::Decision { allowed: true, .. } => "decision: ALLOW".to_owned(),
            Self::Decision {
                allowed: false,
                denial,
            } => format!(
                "decision: DENY ({})",
                denial
                    .as_ref()
                    .map_or("unknown denial".to_owned(), render_denial)
            ),
            Self::Grant(action) => format!("grant: {} (in memory)", render_action(action)),
            Self::HumanApproval => "human approval: recorded".to_owned(),
            Self::ExecutionStarted { action } => {
                format!("execution: STARTED ({})", render_action(action))
            }
            Self::ExecutionCompleted { action } => {
                format!("execution: COMPLETED ({})", render_action(action))
            }
            Self::ExecutionFailed { action } => {
                format!("execution: FAILED ({})", render_action(action))
            }
            Self::PostSnapshot(snapshot) => format!("post-snapshot: {}", render_snapshot(snapshot)),
            Self::Failed { stage } => format!("{stage}: FAILED (closed)"),
            Self::Discarded => "proposal discarded".to_owned(),
        }
    }
}

fn render_snapshot(snapshot: &DeploymentSnapshot) -> String {
    format!(
        "{}/{}@{} desired={} ready={} image=<redacted>",
        snapshot.namespace(),
        snapshot.name(),
        snapshot.resource_version(),
        snapshot.desired_replicas(),
        snapshot.ready_replicas()
    )
}

fn render_action(action: &Action) -> String {
    match action {
        Action::Inspect => "inspect".to_owned(),
        Action::RestartRollout { strategy } => format!("restart_rollout({strategy:?})"),
        Action::RollbackRollout => "rollback_rollout".to_owned(),
        Action::Scale { replicas } => format!("scale(replicas={replicas})"),
        Action::UpdateImage { .. } => "update_image(image=<redacted>)".to_owned(),
        Action::DeleteNamespace => "delete_namespace".to_owned(),
    }
}

fn render_denial(denial: &Denial) -> String {
    match denial {
        Denial::UnauthorizedActor { actor } => format!("unauthorized actor {actor}"),
        Denial::MissingApproval { required } => format!("missing approval from {required}"),
        Denial::Precondition { action, reason } => format!("{}: {reason}", render_action(action)),
        Denial::PolicyRejected { action } => format!("policy rejects {}", render_action(action)),
        Denial::PolicyFailure { action, .. } => {
            format!("policy failure for {}", render_action(action))
        }
        Denial::UnsafeOutcome { action } => format!("unsafe outcome for {}", render_action(action)),
    }
}

#[derive(Debug)]
pub enum InteractiveError {
    Kernel(SafeOpsError),
    Io(io::Error),
}

impl std::fmt::Display for InteractiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kernel(error) => write!(f, "kernel initialization failed: {error}"),
            Self::Io(error) => write!(f, "interactive input failed: {error}"),
        }
    }
}

impl std::error::Error for InteractiveError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> DeploymentSnapshot {
        DeploymentSnapshot::try_new("safeops-demo", "safeops-demo", "7", 2, 2, "secret/image:9")
            .expect("valid snapshot")
    }

    #[test]
    fn parser_accepts_commands_and_preserves_goal_for_provider_boundary() {
        assert_eq!(parse_command("status"), Ok(Command::Status));
        assert_eq!(
            parse_command("plan restart service"),
            Ok(Command::Plan("restart service".to_owned()))
        );
        assert_eq!(parse_command("execute"), Ok(Command::Execute));
        assert_eq!(parse_command("timeline"), Ok(Command::Timeline));
    }

    #[test]
    fn parser_rejects_missing_goal_and_extra_arguments() {
        assert_eq!(parse_command("plan"), Err(ParseError::MissingGoal));
        assert_eq!(parse_command("status now"), Err(ParseError::UnknownCommand));
    }

    #[test]
    fn timeline_redacts_goal_independent_action_parameters_and_image() {
        let action = Action::UpdateImage {
            image: "registry/private:secret".to_owned(),
        };
        let event = TimelineEvent::Snapshot(snapshot());
        let rendered = event.render();
        assert!(rendered.contains("image=<redacted>"));
        assert!(!rendered.contains("secret/image"));
        assert!(
            TimelineEvent::Proposal(action)
                .render()
                .contains("image=<redacted>")
        );
        assert!(
            !TimelineEvent::Proposal(Action::UpdateImage {
                image: "secret".to_owned()
            })
            .render()
            .contains("secret")
        );
    }

    #[test]
    fn state_keeps_grant_until_explicit_discard() {
        let mut state = InteractiveState::default();
        let intent = ToolIntent::new(crate::Principal::OperatorLlm, Action::Inspect, snapshot());
        state.pending = Some(PendingProposal {
            intent,
            grant: None,
        });
        assert!(state.pending.is_some());
        state.pending.take();
        assert!(state.pending.is_none());
    }

    #[test]
    fn new_plan_discards_previous_pending_grant_before_external_calls() {
        let mut state = InteractiveState::default();
        let intent = ToolIntent::new(crate::Principal::OperatorLlm, Action::Inspect, snapshot());
        state.pending = Some(PendingProposal {
            intent,
            grant: None,
        });

        state.begin_plan();

        assert!(state.pending.is_none());
        assert_eq!(state.events, [TimelineEvent::Discarded]);
    }

    #[test]
    fn timeline_is_bounded_without_reusing_sequence_numbers() {
        let mut state = InteractiveState::default();
        for _ in 0..=MAX_TIMELINE_EVENTS {
            state.record(TimelineEvent::Discarded);
        }

        assert_eq!(state.events.len(), MAX_TIMELINE_EVENTS);
        assert_eq!(state.next_sequence, MAX_TIMELINE_EVENTS as u64 + 1);
    }

    #[test]
    fn timeline_has_required_phase_labels() {
        let events = [
            TimelineEvent::Snapshot(snapshot()),
            TimelineEvent::LlmRequest,
            TimelineEvent::Proposal(Action::Inspect),
            TimelineEvent::Decision {
                allowed: true,
                denial: None,
            },
            TimelineEvent::Grant(Action::Inspect),
            TimelineEvent::ExecutionStarted {
                action: Action::Inspect,
            },
            TimelineEvent::ExecutionCompleted {
                action: Action::Inspect,
            },
            TimelineEvent::PostSnapshot(snapshot()),
        ];
        let output = events
            .iter()
            .map(TimelineEvent::render)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(output.contains("snapshot:"));
        assert!(output.contains("llm request:"));
        assert!(output.contains("typed proposal:"));
        assert!(output.contains("decision:"));
        assert!(output.contains("grant:"));
        assert!(output.contains("execution: STARTED"));
        assert!(output.contains("execution: COMPLETED"));
        assert!(output.contains("post-snapshot:"));
    }
}
