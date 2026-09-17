use std::io::{self, BufRead, Write};

use crate::llm::provider_from_env;
use crate::{
    Action, Approval, Decision, Denial, DeploymentSnapshot, ExecutionGrant, ExecutionResult,
    KubectlAdapter, SafeOpsError, SafetyKernel, ToolIntent,
};

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
                Err(error) => print_error("status", &error),
            },
            Ok(Command::Plan(goal)) => plan(&mut state, &mut kernel, &adapter, &goal),
            Ok(Command::Approve) => approve(&mut state, &mut kernel),
            Ok(Command::Execute) => execute(&mut state, &mut kernel, &adapter),
            Ok(Command::Discard) => {
                if state.pending.take().is_some() {
                    state.events.push(TimelineEvent::Discarded);
                    println!("pending proposal discarded");
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
    let snapshot = match adapter.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            print_error("plan snapshot", &error);
            return;
        }
    };
    state.pending = None;
    state.record_snapshot(snapshot.clone());
    let provider = match provider_from_env() {
        Ok(provider) => provider,
        Err(error) => {
            print_error("plan provider", &error);
            return;
        }
    };
    let action = match provider.propose_action(&snapshot, goal) {
        Ok(action) => action,
        Err(error) => {
            print_error("plan provider", &error);
            return;
        }
    };
    state.events.push(TimelineEvent::Proposal(action.clone()));
    let intent = ToolIntent::new(crate::Principal::OperatorLlm, action, snapshot);
    match kernel.authorize(&intent, &[]) {
        Decision::Allow(grant) => {
            state.events.push(TimelineEvent::Decision {
                allowed: true,
                denial: None,
            });
            state
                .events
                .push(TimelineEvent::Grant(grant.intent().action().clone()));
            state.pending = Some(PendingProposal {
                intent,
                grant: Some(grant),
            });
            println!("proposal allowed; type `execute` to run it");
        }
        Decision::Deny(denial) => {
            let approval_needed = matches!(denial, Denial::MissingApproval { .. });
            state.events.push(TimelineEvent::Decision {
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
    let Some(pending) = state.pending.as_mut() else {
        println!("no approval-gated proposal");
        return;
    };
    if !matches!(pending.intent.action(), Action::UpdateImage { .. }) || pending.grant.is_some() {
        println!(
            "approval applies only to a pending update_image denied for missing human approval"
        );
        return;
    }
    match kernel.authorize(&pending.intent, &[Approval::human_operator()]) {
        Decision::Allow(grant) => {
            state.events.push(TimelineEvent::Decision {
                allowed: true,
                denial: None,
            });
            state
                .events
                .push(TimelineEvent::Grant(grant.intent().action().clone()));
            pending.grant = Some(grant);
            println!("human approval accepted; type `execute` to run it");
        }
        Decision::Deny(denial) => {
            state.events.push(TimelineEvent::Decision {
                allowed: false,
                denial: Some(denial.clone()),
            });
            pending.grant = None;
            println!("approval rejected: {}", render_denial(&denial));
        }
    }
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
    match adapter.execute(kernel, grant) {
        Ok(result) => {
            state.events.push(TimelineEvent::Execution { action });
            let snapshot = match result {
                ExecutionResult::Observed(snapshot) => snapshot,
                ExecutionResult::Mutated { after, .. } => after,
            };
            state
                .events
                .push(TimelineEvent::PostSnapshot(snapshot.clone()));
            println!("execution completed; {}", render_snapshot(&snapshot));
        }
        Err(error) => {
            state.events.push(TimelineEvent::ExecutionFailed { action });
            println!("execution failed: {error}");
            // A grant is consumed before adapter side effects. Never restore it.
            if let Ok(snapshot) = adapter.snapshot() {
                state.events.push(TimelineEvent::PostSnapshot(snapshot));
            }
        }
    }
}

fn print_timeline(state: &InteractiveState) {
    if state.events.is_empty() {
        println!("timeline empty");
        return;
    }
    for (index, event) in state.events.iter().enumerate() {
        println!("{}. {}", index + 1, event.render());
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
}

struct PendingProposal {
    intent: ToolIntent,
    grant: Option<ExecutionGrant>,
}

impl InteractiveState {
    fn record_snapshot(&mut self, snapshot: DeploymentSnapshot) {
        self.events.push(TimelineEvent::Snapshot(snapshot));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TimelineEvent {
    Snapshot(DeploymentSnapshot),
    Proposal(Action),
    Decision {
        allowed: bool,
        denial: Option<Denial>,
    },
    Grant(Action),
    Execution {
        action: Action,
    },
    ExecutionFailed {
        action: Action,
    },
    PostSnapshot(DeploymentSnapshot),
    Discarded,
}

impl TimelineEvent {
    fn render(&self) -> String {
        match self {
            Self::Snapshot(snapshot) => format!("snapshot: {}", render_snapshot(snapshot)),
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
            Self::Execution { action } => format!("execution: {}", render_action(action)),
            Self::ExecutionFailed { action } => {
                format!("execution: FAILED ({})", render_action(action))
            }
            Self::PostSnapshot(snapshot) => format!("post-snapshot: {}", render_snapshot(snapshot)),
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
    fn timeline_has_required_phase_labels() {
        let events = [
            TimelineEvent::Snapshot(snapshot()),
            TimelineEvent::Proposal(Action::Inspect),
            TimelineEvent::Decision {
                allowed: true,
                denial: None,
            },
            TimelineEvent::Grant(Action::Inspect),
            TimelineEvent::Execution {
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
        assert!(output.contains("typed proposal:"));
        assert!(output.contains("decision:"));
        assert!(output.contains("grant:"));
        assert!(output.contains("execution:"));
        assert!(output.contains("post-snapshot:"));
    }
}
