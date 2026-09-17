//! Real LLM adapters for the `SafeOps` proposal boundary.
//!
//! Providers return only a validated [`Action`]. Every caller must still pass
//! that action through [`SafetyKernel::authorize`](crate::SafetyKernel);
//! provider output is never an execution permission.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde_json::{Map, Value, json};

use crate::{Action, DeploymentSnapshot, RolloutStrategy};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const CODEX_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "action": {
      "type": "string",
      "enum": ["inspect", "restart_rollout", "rollback_rollout", "scale", "update_image", "delete_namespace"]
    },
    "strategy": {"type": ["string", "null"], "enum": ["rolling", "recreate", null]},
    "replicas": {"type": ["integer", "null"], "minimum": 0, "maximum": 4294967295},
    "image": {"type": ["string", "null"]}
  },
  "required": ["action", "strategy", "replicas", "image"]
}"#;

const SYSTEM_PROMPT: &str = "You propose one Kubernetes SafeOps action. Never call tools, shell, kubectl, or external services. Return only the requested structured action; do not include explanations, reasoning, markdown, or extra fields.";

/// Errors returned by an LLM adapter. Error values never contain API keys or
/// provider response bodies.
#[derive(Debug)]
pub enum LlmError {
    MissingConfiguration(&'static str),
    InvalidConfiguration(&'static str),
    InvalidResponse(&'static str),
    InvalidAction(String),
    Io(io::Error),
    ProviderUnavailable(&'static str),
    HttpStatus(u16),
    Timeout,
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingConfiguration(name) => write!(f, "missing configuration {name}"),
            Self::InvalidConfiguration(name) => write!(f, "invalid configuration {name}"),
            Self::InvalidResponse(reason) => write!(f, "invalid provider response: {reason}"),
            Self::InvalidAction(reason) => write!(f, "invalid action: {reason}"),
            Self::Io(error) => write!(f, "provider I/O failed: {error}"),
            Self::ProviderUnavailable(name) => write!(f, "provider {name} failed"),
            Self::HttpStatus(status) => write!(f, "provider returned HTTP status {status}"),
            Self::Timeout => f.write_str("provider timed out"),
        }
    }
}

impl std::error::Error for LlmError {}

impl From<io::Error> for LlmError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// A provider that can propose one typed kernel action for a deployment goal.
pub trait ActionProvider {
    /// Ask the provider for an action. The result is untrusted until the
    /// caller constructs a [`ToolIntent`](crate::ToolIntent) and authorizes it.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider is unavailable or returns an invalid
    /// structured action.
    fn propose_action(&self, snapshot: &DeploymentSnapshot, goal: &str)
    -> Result<Action, LlmError>;
}

/// Configuration for the local, subscription-authenticated Codex CLI.
pub struct CodexProvider {
    model: Option<String>,
    timeout: Duration,
}

impl CodexProvider {
    /// Build a provider from `SAFEOPS_CODEX_MODEL` and
    /// `SAFEOPS_LLM_TIMEOUT_SECS`. Codex authentication is handled by its
    /// existing local login state.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            model: env::var("SAFEOPS_CODEX_MODEL")
                .ok()
                .filter(|value| !value.is_empty()),
            timeout: timeout_from_env(),
        }
    }

    /// Build a provider with explicit model and timeout settings.
    #[must_use]
    pub const fn new(model: Option<String>, timeout: Duration) -> Self {
        Self { model, timeout }
    }
}

impl ActionProvider for CodexProvider {
    fn propose_action(
        &self,
        snapshot: &DeploymentSnapshot,
        goal: &str,
    ) -> Result<Action, LlmError> {
        let output_path = unique_temp_path("safeops-codex-response", "json");
        let schema_path = unique_temp_path("safeops-codex-schema", "json");
        write_new_file(&schema_path, CODEX_OUTPUT_SCHEMA.as_bytes())?;
        let prompt = prompt_for(snapshot, goal);

        let mut command = Command::new("codex");
        command
            .arg("exec")
            .arg("--ephemeral")
            .arg("--sandbox")
            .arg("read-only")
            .arg("--output-schema")
            .arg(&schema_path)
            .arg("--output-last-message")
            .arg(&output_path)
            .arg("--skip-git-repo-check");
        if let Some(model) = &self.model {
            command.arg("--model").arg(model);
        }
        command
            .arg("--")
            .arg(prompt)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let result = run_with_timeout(command, self.timeout, "Codex", &output_path);
        remove_temp_files(&[&output_path, &schema_path]);
        let output = result?;
        parse_action_json(&output)
    }
}

/// Configuration for the `DeepSeek` chat-completions API.
pub struct DeepSeekProvider {
    api_key: String,
    model: String,
    endpoint: String,
    client: Client,
}

impl DeepSeekProvider {
    /// Read `DEEPSEEK_API_KEY`, optional `DEEPSEEK_MODEL`, optional
    /// `DEEPSEEK_BASE_URL`, and optional `SAFEOPS_LLM_TIMEOUT_SECS`.
    ///
    /// The API key is retained only in the request header. It is never put in
    /// a URL, process argument, debug representation, or error value.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing or the
    /// HTTP client cannot be configured.
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = env::var("DEEPSEEK_API_KEY")
            .map_err(|_| LlmError::MissingConfiguration("DEEPSEEK_API_KEY"))?;
        let model = env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-chat".to_owned());
        let base_url =
            env::var("DEEPSEEK_BASE_URL").unwrap_or_else(|_| "https://api.deepseek.com".to_owned());
        Self::new(&api_key, &model, &base_url, timeout_from_env())
    }

    /// Build a provider with an API key, model, endpoint, and request timeout.
    /// The key is used only for the Authorization header.
    ///
    /// # Errors
    ///
    /// Returns an error when any required value is empty or the HTTP client
    /// cannot be configured.
    pub fn new(
        api_key: &str,
        model: &str,
        base_url: &str,
        timeout: Duration,
    ) -> Result<Self, LlmError> {
        if api_key.is_empty() {
            return Err(LlmError::InvalidConfiguration("DEEPSEEK_API_KEY"));
        }
        if model.is_empty() {
            return Err(LlmError::InvalidConfiguration("DEEPSEEK_MODEL"));
        }
        if base_url.is_empty() {
            return Err(LlmError::InvalidConfiguration("DEEPSEEK_BASE_URL"));
        }
        let endpoint = if base_url.ends_with("/chat/completions") {
            base_url.to_owned()
        } else {
            format!("{}/chat/completions", base_url.trim_end_matches('/'))
        };
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| LlmError::ProviderUnavailable("DeepSeek"))?;
        Ok(Self {
            api_key: api_key.to_owned(),
            model: model.to_owned(),
            endpoint,
            client,
        })
    }
}

impl ActionProvider for DeepSeekProvider {
    fn propose_action(
        &self,
        snapshot: &DeploymentSnapshot,
        goal: &str,
    ) -> Result<Action, LlmError> {
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&deepseek_request(&self.model, snapshot, goal))
            .send()
            .map_err(|_| LlmError::ProviderUnavailable("DeepSeek"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(LlmError::HttpStatus(status.as_u16()));
        }
        let body = response
            .json::<Value>()
            .map_err(|_| LlmError::InvalidResponse("response was not JSON"))?;
        parse_deepseek_response(&body)
    }
}

/// Select a real provider using `SAFEOPS_LLM_PROVIDER=codex|deepseek`.
///
/// # Errors
///
/// Returns an error when the provider selector or its required credentials are
/// missing or invalid.
pub fn provider_from_env() -> Result<Box<dyn ActionProvider>, LlmError> {
    match env::var("SAFEOPS_LLM_PROVIDER").as_deref() {
        Ok("codex") => Ok(Box::new(CodexProvider::from_env())),
        Ok("deepseek") => Ok(Box::new(DeepSeekProvider::from_env()?)),
        Ok(_) => Err(LlmError::InvalidConfiguration("SAFEOPS_LLM_PROVIDER")),
        Err(_) => Err(LlmError::MissingConfiguration("SAFEOPS_LLM_PROVIDER")),
    }
}

fn timeout_from_env() -> Duration {
    env::var("SAFEOPS_LLM_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map_or(DEFAULT_TIMEOUT, Duration::from_secs)
}

fn prompt_for(snapshot: &DeploymentSnapshot, goal: &str) -> String {
    format!(
        "{SYSTEM_PROMPT}\n\nDeployment snapshot:\nnamespace={}; name={}; resource_version={}; desired_replicas={}; ready_replicas={}; image={}\nGoal: {goal}\n\nChoose exactly one action object with action and only the parameters required by that action.",
        snapshot.namespace(),
        snapshot.name(),
        snapshot.resource_version(),
        snapshot.desired_replicas(),
        snapshot.ready_replicas(),
        snapshot.image(),
    )
}

fn deepseek_request(model: &str, snapshot: &DeploymentSnapshot, goal: &str) -> Value {
    json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt_for(snapshot, goal)},
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "propose_action",
                "description": "Return exactly one SafeOps action.",
                "parameters": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "action": {"type": "string", "enum": ["inspect", "restart_rollout", "rollback_rollout", "scale", "update_image", "delete_namespace"]},
                        "strategy": {"type": "string", "enum": ["rolling", "recreate"]},
                        "replicas": {"type": "integer", "minimum": 0, "maximum": 4_294_967_295_u64},
                        "image": {"type": "string"}
                    },
                    "required": ["action"]
                }
            }
        }],
        "tool_choice": {"type": "function", "function": {"name": "propose_action"}}
    })
}

fn parse_deepseek_response(body: &Value) -> Result<Action, LlmError> {
    let choices = body
        .get("choices")
        .and_then(Value::as_array)
        .filter(|choices| choices.len() == 1)
        .ok_or(LlmError::InvalidResponse("expected one choice"))?;
    let calls = choices
        .first()
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
        .filter(|calls| calls.len() == 1)
        .ok_or(LlmError::InvalidResponse("expected one function call"))?;
    let call = &calls[0];
    if call.get("type").and_then(Value::as_str) != Some("function")
        || call
            .get("function")
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            != Some("propose_action")
    {
        return Err(LlmError::InvalidResponse("unexpected function call"));
    }
    let arguments = call
        .get("function")
        .and_then(|function| function.get("arguments"))
        .and_then(Value::as_str)
        .ok_or(LlmError::InvalidResponse("missing function arguments"))?;
    parse_action_json(arguments)
}

fn parse_action_json(source: &str) -> Result<Action, LlmError> {
    let value: Value = serde_json::from_str(source)
        .map_err(|_| LlmError::InvalidResponse("action was not valid JSON"))?;
    let object = value
        .as_object()
        .ok_or(LlmError::InvalidResponse("action was not a JSON object"))?;
    let object: Map<String, Value> = object
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .ok_or(LlmError::InvalidAction("missing action".to_owned()))?;

    let allowed = ["action", "strategy", "replicas", "image"];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(LlmError::InvalidAction("unknown action field".to_owned()));
    }

    match action {
        "inspect" => require_only_action(&object, Action::Inspect),
        "rollback_rollout" => require_only_action(&object, Action::RollbackRollout),
        "delete_namespace" => require_only_action(&object, Action::DeleteNamespace),
        "restart_rollout" => {
            let strategy = match object.get("strategy").and_then(Value::as_str) {
                Some("rolling") => RolloutStrategy::Rolling,
                Some("recreate") => RolloutStrategy::Recreate,
                _ => return Err(LlmError::InvalidAction("missing strategy".to_owned())),
            };
            require_no_fields(&object, &["action", "strategy"])?;
            Ok(Action::RestartRollout { strategy })
        }
        "scale" => {
            let replicas = object
                .get("replicas")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| LlmError::InvalidAction("invalid replicas".to_owned()))?;
            require_no_fields(&object, &["action", "replicas"])?;
            Ok(Action::Scale { replicas })
        }
        "update_image" => {
            let image = object
                .get("image")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| LlmError::InvalidAction("invalid image".to_owned()))?;
            require_no_fields(&object, &["action", "image"])?;
            Ok(Action::UpdateImage {
                image: image.to_owned(),
            })
        }
        _ => Err(LlmError::InvalidAction("unknown action".to_owned())),
    }
}

fn require_only_action(object: &Map<String, Value>, action: Action) -> Result<Action, LlmError> {
    require_no_fields(object, &["action"])?;
    Ok(action)
}

fn require_no_fields(object: &Map<String, Value>, expected: &[&str]) -> Result<(), LlmError> {
    if object.keys().all(|key| expected.contains(&key.as_str())) {
        Ok(())
    } else {
        Err(LlmError::InvalidAction(
            "unexpected action parameter".to_owned(),
        ))
    }
}

fn unique_temp_path(prefix: &str, extension: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    env::temp_dir().join(format!(
        "{prefix}-{}-{nanos}.{extension}",
        std::process::id()
    ))
}

fn write_new_file(path: &Path, contents: &[u8]) -> Result<(), LlmError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(contents)?;
    Ok(())
}

fn remove_temp_files(paths: &[&Path]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

fn run_with_timeout(
    mut command: Command,
    timeout: Duration,
    provider: &'static str,
    output_path: &Path,
) -> Result<String, LlmError> {
    let mut child = command
        .spawn()
        .map_err(|_| LlmError::ProviderUnavailable(provider))?;
    let started = SystemTime::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| LlmError::ProviderUnavailable(provider))?
        {
            if !status.success() {
                return Err(LlmError::ProviderUnavailable(provider));
            }
            return fs::read_to_string(output_path)
                .map_err(|_| LlmError::InvalidResponse("provider output was unavailable"));
        }
        if started.elapsed().is_ok_and(|elapsed| elapsed >= timeout) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LlmError::Timeout);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_typed_actions_and_kernel_can_reject_them_later() {
        assert_eq!(
            parse_action_json(r#"{"action":"inspect"}"#).unwrap(),
            Action::Inspect
        );
        assert_eq!(
            parse_action_json(r#"{"action":"scale","replicas":1}"#).unwrap(),
            Action::Scale { replicas: 1 }
        );
        assert_eq!(
            parse_action_json(r#"{"action":"restart_rollout","strategy":"rolling"}"#).unwrap(),
            Action::RestartRollout {
                strategy: RolloutStrategy::Rolling
            }
        );
    }

    #[test]
    fn parser_rejects_explanations_unknown_fields_and_missing_parameters() {
        assert!(parse_action_json(r#"{"action":"inspect","explanation":"why"}"#).is_err());
        assert!(parse_action_json(r#"{"action":"scale"}"#).is_err());
        assert!(parse_action_json("```json {\"action\":\"inspect\"} ```").is_err());
        assert!(
            parse_action_json(r#"{"action":"restart_rollout","strategy":"recreate","image":"x"}"#)
                .is_err()
        );
    }

    #[test]
    fn deepseek_response_requires_a_function_call() {
        let response = json!({
            "choices": [{"message": {"content": "{\"action\":\"inspect\"}"}}]
        });
        assert!(matches!(
            parse_deepseek_response(&response),
            Err(LlmError::InvalidResponse("expected one function call"))
        ));
    }
}
