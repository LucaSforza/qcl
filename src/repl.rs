//! Testable command dispatcher and linenoise-independent REPL state.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::checker::ModelChecker;
use crate::domain::{Coalition, StateSet};
use crate::inference::InferenceEngine;
use crate::model::{ModelValidationErrors, ModelValidator, QclModel};
use crate::parser::{ParseError, parse_formula, parse_model, parse_predicate};
use crate::predicate::PredicateProgram;

/// A parsed REPL command. Parsing does not access the filesystem or a model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Load(PathBuf),
    Validate,
    Check {
        state: String,
        formula: String,
    },
    States {
        formula: String,
    },
    Infer {
        premises: Vec<String>,
        conclusion: String,
    },
    Coalitions {
        predicate: String,
    },
    Tutorial,
    Help,
    Quit,
    Empty,
}

const COMMAND_COMPLETIONS: [&str; 10] = [
    ":load",
    ":validate",
    ":check",
    ":states",
    ":infer",
    ":coalitions",
    ":tutorial",
    ":help",
    ":quit",
    ":q",
];

/// Return REPL command completions matching the token currently being edited.
///
/// `linenoise-rust` passes the current input buffer to callbacks, so command
/// candidates include their leading colon and replace the complete buffer.
#[must_use]
pub fn command_completions(input: &str) -> Vec<String> {
    if !input.starts_with(':') {
        return Vec::new();
    }

    COMMAND_COMPLETIONS
        .iter()
        .filter(|command| command.starts_with(input))
        .map(|command| (*command).to_owned())
        .collect()
}

/// Return the on-disk history location used by the command-line REPL.
///
/// The XDG state directory is preferred on Unix-like systems. If it is not
/// configured, history is stored below the user's home directory.
#[must_use]
pub fn history_path() -> Option<PathBuf> {
    let state_home = env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    history_path_from_dirs(state_home.as_deref(), home.as_deref())
}

fn history_path_from_dirs(state_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    state_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.join("qcl").join("history"))
        .or_else(|| {
            home.filter(|path| !path.as_os_str().is_empty())
                .map(|path| {
                    path.join(".local")
                        .join("state")
                        .join("qcl")
                        .join("history")
                })
        })
}

/// Syntax errors produced before command execution.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CommandParseError {
    #[error("unknown command `{0}` (try :help)")]
    Unknown(String),
    #[error("usage: {usage}")]
    Usage { usage: &'static str },
    #[error("inference requires `PREMISES |- CONCLUSION`")]
    MissingInferenceDelimiter,
}

/// Errors returned by command execution. No command turns malformed input
/// into a panic, and loading only replaces a previous model after success.
#[derive(Debug, Error)]
pub enum ReplError {
    #[error(transparent)]
    Command(#[from] CommandParseError),
    #[error("cannot read model file `{path}`: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Validation(#[from] ModelValidationErrors),
    #[error("unknown state `{0}`")]
    UnknownState(String),
    #[error(transparent)]
    Check(#[from] crate::checker::ModelCheckerError),
    #[error(transparent)]
    Inference(#[from] crate::inference::InferenceError),
    #[error("no model loaded (use :load FILE)")]
    NoModel,
}

/// Result of dispatching one command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    pub text: String,
    pub quit: bool,
}

impl CommandOutput {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            quit: false,
        }
    }

    fn quit() -> Self {
        Self {
            text: String::new(),
            quit: true,
        }
    }
}

/// Parse one line according to the documented command contract.
///
/// # Errors
///
/// Returns a [`CommandParseError`] when the command name or arguments are invalid.
pub fn parse_command(line: &str) -> Result<Command, CommandParseError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(Command::Empty);
    }
    let Some(rest) = line.strip_prefix(':') else {
        return Err(CommandParseError::Unknown(line.to_owned()));
    };
    let (name, argument) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let argument = argument.trim();
    match name {
        "load" if !argument.is_empty() => Ok(Command::Load(PathBuf::from(argument))),
        "load" => Err(CommandParseError::Usage {
            usage: ":load FILE",
        }),
        "validate" if argument.is_empty() => Ok(Command::Validate),
        "validate" => Err(CommandParseError::Usage { usage: ":validate" }),
        "check" => {
            let Some((state, formula)) = argument.split_once(char::is_whitespace) else {
                return Err(CommandParseError::Usage {
                    usage: ":check STATE FORMULA",
                });
            };
            if formula.trim().is_empty() {
                return Err(CommandParseError::Usage {
                    usage: ":check STATE FORMULA",
                });
            }
            Ok(Command::Check {
                state: state.to_owned(),
                formula: formula.trim().to_owned(),
            })
        }
        "states" if !argument.is_empty() => Ok(Command::States {
            formula: argument.to_owned(),
        }),
        "states" => Err(CommandParseError::Usage {
            usage: ":states FORMULA",
        }),
        "infer" => parse_infer(argument),
        "coalitions" if !argument.is_empty() => Ok(Command::Coalitions {
            predicate: argument.to_owned(),
        }),
        "coalitions" => Err(CommandParseError::Usage {
            usage: ":coalitions PREDICATE",
        }),
        "tutorial" if argument.is_empty() => Ok(Command::Tutorial),
        "tutorial" => Err(CommandParseError::Usage { usage: ":tutorial" }),
        "help" if argument.is_empty() => Ok(Command::Help),
        "help" => Err(CommandParseError::Usage { usage: ":help" }),
        "quit" | "q" if argument.is_empty() => Ok(Command::Quit),
        "quit" | "q" => Err(CommandParseError::Usage { usage: ":quit" }),
        _ => Err(CommandParseError::Unknown(name.to_owned())),
    }
}

fn parse_infer(argument: &str) -> Result<Command, CommandParseError> {
    let Some((premises, conclusion)) = argument.split_once("|-") else {
        return Err(CommandParseError::MissingInferenceDelimiter);
    };
    let conclusion = conclusion.trim();
    if conclusion.is_empty() {
        return Err(CommandParseError::MissingInferenceDelimiter);
    }
    let premises = split_premises(premises)
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    Ok(Command::Infer {
        premises,
        conclusion: conclusion.to_owned(),
    })
}

/// Split comma/semicolon-separated premises, ignoring separators in braces
/// and parentheses used by coalition predicates.
fn split_premises(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    for (index, character) in source.char_indices() {
        match character {
            '{' | '(' => depth += 1,
            '}' | ')' => depth = (depth - 1).max(0),
            ',' | ';' if depth == 0 => {
                result.push(source[start..index].trim().to_owned());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    result.push(source[start..].trim().to_owned());
    result
}

/// Mutable model context used by the command dispatcher.
#[derive(Default)]
pub struct Repl {
    model: Option<QclModel>,
}

impl Repl {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn model(&self) -> Option<&QclModel> {
        self.model.as_ref()
    }

    /// Parse and execute one command.
    ///
    /// # Errors
    ///
    /// Returns a [`ReplError`] for command syntax, model, parsing, or checking failures.
    pub fn execute_line(&mut self, line: &str) -> Result<CommandOutput, ReplError> {
        let command = parse_command(line)?;
        self.execute(command)
    }

    /// Execute an already parsed command, useful for embedding and tests.
    ///
    /// # Errors
    ///
    /// Returns a [`ReplError`] if the command cannot be executed against the current model.
    pub fn execute(&mut self, command: Command) -> Result<CommandOutput, ReplError> {
        match command {
            Command::Empty => Ok(CommandOutput::text("")),
            Command::Tutorial => Ok(CommandOutput::text(TUTORIAL)),
            Command::Help => Ok(CommandOutput::text(HELP)),
            Command::Quit => Ok(CommandOutput::quit()),
            Command::Load(path) => self.load(&path),
            Command::Validate => {
                let model = self.model.as_ref().ok_or(ReplError::NoModel)?;
                ModelValidator::validate(model)?;
                Ok(CommandOutput::text("model valid"))
            }
            Command::Check { state, formula } => {
                let model = self.model.as_ref().ok_or(ReplError::NoModel)?;
                let state = model
                    .states
                    .get(&state)
                    .ok_or(ReplError::UnknownState(state))?;
                let formula = parse_formula(&formula)?.resolve(&model.agents, &model.atoms)?;
                let result = ModelChecker::new(model).check(state, &formula)?;
                Ok(CommandOutput::text(result.to_string()))
            }
            Command::States { formula } => {
                let model = self.model.as_ref().ok_or(ReplError::NoModel)?;
                let formula = parse_formula(&formula)?.resolve(&model.agents, &model.atoms)?;
                let states = ModelChecker::new(model).satisfying_states(&formula)?;
                Ok(CommandOutput::text(format_states(model, &states)))
            }
            Command::Infer {
                premises,
                conclusion,
            } => {
                let model = self.model.as_ref().ok_or(ReplError::NoModel)?;
                let premises = premises
                    .iter()
                    .map(|source| {
                        parse_formula(source).and_then(|f| f.resolve(&model.agents, &model.atoms))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let conclusion =
                    parse_formula(&conclusion)?.resolve(&model.agents, &model.atoms)?;
                let counterexamples = InferenceEngine::new(model).infer(&premises, &conclusion)?;
                Ok(CommandOutput::text(format_states(model, &counterexamples)))
            }
            Command::Coalitions { predicate } => {
                let model = self.model.as_ref().ok_or(ReplError::NoModel)?;
                let predicate = parse_predicate(&predicate)?.resolve(&model.agents)?;
                let program = PredicateProgram::compile(&predicate);
                let coalitions = Coalition::all(model.agent_count())
                    .filter(|coalition| program.evaluate(coalition))
                    .map(|coalition| format_coalition(model, &coalition))
                    .collect::<Vec<_>>();
                Ok(CommandOutput::text(if coalitions.is_empty() {
                    "(none)".to_owned()
                } else {
                    coalitions.join("\n")
                }))
            }
        }
    }

    fn load(&mut self, path: &Path) -> Result<CommandOutput, ReplError> {
        let source = fs::read_to_string(path).map_err(|source| ReplError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let model = parse_model(&source)?;
        let summary = format!(
            "loaded {} states, {} agents, {} properties",
            model.state_count(),
            model.agent_count(),
            model.atom_count()
        );
        self.model = Some(model);
        Ok(CommandOutput::text(summary))
    }
}

const HELP: &str = ":load FILE\n:validate\n:check STATE FORMULA\n:states FORMULA\n:infer PREMISES |- CONCLUSION\n:coalitions PREDICATE\n:tutorial\n:help\n:quit";
const TUTORIAL: &str = include_str!("../kb/tutorial.md");

fn format_states(model: &QclModel, states: &StateSet) -> String {
    let names = states
        .iter()
        .filter_map(|state| model.states.name(state))
        .collect::<Vec<_>>();
    if names.is_empty() {
        "(none)".to_owned()
    } else {
        names.join(" ")
    }
}

fn format_coalition(model: &QclModel, coalition: &Coalition) -> String {
    let mut text = String::from("{");
    for (index, agent) in coalition.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        let _ = write!(text, "{}", model.agents.name(agent).unwrap_or("?"));
    }
    text.push('}');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const MODEL: &str = "model { agents { alice }; states { s0 }; props { ready }; valuation { s0: { ready }; }; effectivity { s0, {} -> { s0 }; s0, { alice } -> { s0 }; }; }";

    #[test]
    fn parses_commands_without_terminal() {
        assert_eq!(parse_command(":tutorial"), Ok(Command::Tutorial));
        assert_eq!(
            parse_command(":tutorial extra"),
            Err(CommandParseError::Usage { usage: ":tutorial" })
        );
        assert_eq!(
            parse_command(":states ready"),
            Ok(Command::States {
                formula: "ready".into()
            })
        );
        assert_eq!(
            parse_command(":check s0 !ready"),
            Ok(Command::Check {
                state: "s0".into(),
                formula: "!ready".into()
            })
        );
        assert_eq!(
            parse_command(":infer p, [any] q |- r"),
            Ok(Command::Infer {
                premises: vec!["p".into(), "[any] q".into()],
                conclusion: "r".into()
            })
        );
    }

    #[test]
    fn completes_repl_commands_by_prefix() {
        assert_eq!(
            command_completions(":"),
            vec![
                ":load",
                ":validate",
                ":check",
                ":states",
                ":infer",
                ":coalitions",
                ":tutorial",
                ":help",
                ":quit",
                ":q",
            ]
        );
        assert_eq!(command_completions(":sta"), vec![":states"]);
        assert_eq!(command_completions("states"), Vec::<String>::new());
    }

    #[test]
    fn chooses_xdg_history_path_before_home() {
        assert_eq!(
            history_path_from_dirs(Some(Path::new("/state")), Some(Path::new("/home/user"))),
            Some(PathBuf::from("/state/qcl/history"))
        );
        assert_eq!(
            history_path_from_dirs(None, Some(Path::new("/home/user"))),
            Some(PathBuf::from("/home/user/.local/state/qcl/history"))
        );
        assert_eq!(history_path_from_dirs(None, None), None);
    }

    #[test]
    fn reports_no_model_instead_of_panicking() {
        assert_eq!(
            Repl::new()
                .execute_line(":validate")
                .expect_err("no model")
                .to_string(),
            "no model loaded (use :load FILE)"
        );
    }

    #[test]
    fn tutorial_does_not_require_a_model() {
        let output = Repl::new()
            .execute_line(":tutorial")
            .expect("tutorial without model");
        assert!(!output.text.is_empty());
        assert!(!output.quit);
    }

    #[test]
    fn help_lists_tutorial() {
        assert!(
            Repl::new()
                .execute_line(":help")
                .unwrap()
                .text
                .contains(":tutorial")
        );
    }

    #[test]
    fn dispatches_loaded_model_commands() {
        let path = std::env::temp_dir().join(format!("qcl-repl-{}.qcl", std::process::id()));
        fs::write(&path, MODEL).expect("fixture");
        let mut repl = Repl::new();
        repl.execute_line(&format!(":load {}", path.display()))
            .expect("load");
        assert_eq!(
            repl.execute_line(":validate").expect("validate").text,
            "model valid"
        );
        assert_eq!(
            repl.execute_line(":check s0 ready").expect("check").text,
            "true"
        );
        assert_eq!(
            repl.execute_line(":states ready").expect("states").text,
            "s0"
        );
        let _ = fs::remove_file(path);
    }
}
