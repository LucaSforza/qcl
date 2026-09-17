use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{
    Action, DeploymentSnapshot, ExecutionGrant, ExecutionPermit, RolloutStrategy, SafeOpsError,
    SafetyKernel, SnapshotError,
};

const CONTEXT: &str = "kind-safeops-qcl";
const NAMESPACE: &str = "safeops-demo";
const DEPLOYMENT: &str = "safeops-demo";
const IMPERSONATED_EXECUTOR: &str = "system:serviceaccount:safeops-demo:safeops-executor";
// Rollout status has its own 60s bound, so the process bound must leave room
// for that operation to report failure rather than killing it prematurely.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(90);
const ROLLOUT_TIMEOUT: &str = "60s";
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// A bounded, allowlisted adapter for the real `kubectl` binary.
pub struct KubectlAdapter {
    timeout: Duration,
    max_output_bytes: usize,
}

impl Default for KubectlAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl KubectlAdapter {
    /// Construct an adapter with the fixed `SafeOps` cluster boundary.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            timeout: COMMAND_TIMEOUT,
            max_output_bytes: MAX_OUTPUT_BYTES,
        }
    }

    /// Read the allowlisted Deployment from the real cluster.
    ///
    /// # Errors
    ///
    /// Returns a bounded command, timeout, JSON, or snapshot validation error.
    pub fn snapshot(&self) -> Result<DeploymentSnapshot, AdapterError> {
        let output = self.run_kubectl(&[
            "get".to_owned(),
            format!("deployment/{DEPLOYMENT}"),
            "--output=json".to_owned(),
        ])?;
        parse_snapshot(&output.stdout)
    }

    /// Consume a kernel grant and execute its permitted operation against the
    /// real allowlisted Deployment. `DeleteNamespace` has no adapter mapping.
    ///
    /// # Errors
    ///
    /// Returns before any command for deletion, stale/replayed grants, invalid
    /// image input, command failure, timeout, or failed postcondition.
    pub fn execute(
        &self,
        kernel: &mut SafetyKernel,
        grant: ExecutionGrant,
    ) -> Result<ExecutionResult, AdapterError> {
        if matches!(grant.intent().action(), Action::DeleteNamespace) {
            return Err(AdapterError::DeleteForbidden);
        }
        let before = self.snapshot()?;
        let permit = kernel
            .consume_grant(grant, &before)
            .map_err(AdapterError::Kernel)?;
        let result = self.execute_permit(&permit)?;
        if matches!(permit.action(), Action::Inspect) {
            return Ok(ExecutionResult::Observed(result));
        }
        let after = self.snapshot()?;
        Ok(ExecutionResult::Mutated { before, after })
    }

    fn execute_permit(&self, permit: &ExecutionPermit) -> Result<DeploymentSnapshot, AdapterError> {
        match permit.action() {
            Action::Inspect => self.snapshot(),
            Action::RestartRollout {
                strategy: RolloutStrategy::Rolling,
            } => {
                self.run_kubectl(&[
                    "rollout".to_owned(),
                    "restart".to_owned(),
                    format!("deployment/{DEPLOYMENT}"),
                ])?;
                self.wait_rollout()?;
                self.snapshot()
            }
            Action::RollbackRollout => {
                self.run_kubectl(&[
                    "rollout".to_owned(),
                    "undo".to_owned(),
                    format!("deployment/{DEPLOYMENT}"),
                ])?;
                self.wait_rollout()?;
                self.snapshot()
            }
            Action::Scale { replicas } => {
                self.run_kubectl(&[
                    "scale".to_owned(),
                    format!("deployment/{DEPLOYMENT}"),
                    format!("--replicas={replicas}"),
                ])?;
                self.wait_rollout()?;
                self.snapshot()
            }
            Action::UpdateImage { image } => {
                validate_image(image)?;
                self.run_kubectl(&[
                    "set".to_owned(),
                    "image".to_owned(),
                    format!("deployment/{DEPLOYMENT}"),
                    format!("app={image}"),
                ])?;
                self.wait_rollout()?;
                self.snapshot()
            }
            Action::RestartRollout {
                strategy: RolloutStrategy::Recreate,
            }
            | Action::DeleteNamespace => Err(AdapterError::UnsupportedAction),
        }
    }

    fn wait_rollout(&self) -> Result<(), AdapterError> {
        self.run_kubectl(&[
            "rollout".to_owned(),
            "status".to_owned(),
            format!("deployment/{DEPLOYMENT}"),
            format!("--timeout={ROLLOUT_TIMEOUT}"),
        ])?;
        Ok(())
    }

    fn run_kubectl(&self, args: &[String]) -> Result<CommandOutput, AdapterError> {
        let mut command = Command::new("kubectl");
        command
            .arg("--context")
            .arg(CONTEXT)
            .arg("--namespace")
            .arg(NAMESPACE)
            .arg(format!("--as={IMPERSONATED_EXECUTOR}"))
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| AdapterError::Spawn(error.to_string()))?;
        let stdout = child.stdout.take().ok_or(AdapterError::MissingOutput)?;
        let stderr = child.stderr.take().ok_or(AdapterError::MissingOutput)?;
        let max = self.max_output_bytes;
        let stdout_thread = thread::spawn(move || read_bounded(stdout, max));
        let stderr_thread = thread::spawn(move || read_bounded(stderr, max));
        let status = wait_child(&mut child, self.timeout)?;
        let stdout = stdout_thread
            .join()
            .map_err(|_| AdapterError::OutputReader)?;
        let stderr = stderr_thread
            .join()
            .map_err(|_| AdapterError::OutputReader)?;
        if stdout.truncated || stderr.truncated {
            return Err(AdapterError::OutputLimit);
        }
        if !status.success() {
            return Err(AdapterError::CommandFailed {
                status: status.code(),
                stderr: bounded_text(&stderr.bytes),
            });
        }
        Ok(CommandOutput {
            stdout: stdout.bytes,
        })
    }
}

/// Result returned after a real adapter operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionResult {
    Observed(DeploymentSnapshot),
    Mutated {
        before: DeploymentSnapshot,
        after: DeploymentSnapshot,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterError {
    DeleteForbidden,
    UnsupportedAction,
    InvalidImage,
    Spawn(String),
    MissingOutput,
    OutputReader,
    Timeout,
    OutputLimit,
    CommandFailed { status: Option<i32>, stderr: String },
    Json(String),
    Snapshot(SnapshotError),
    Kernel(SafeOpsError),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeleteForbidden => {
                f.write_str("namespace deletion is not supported by this adapter")
            }
            Self::UnsupportedAction => f.write_str("action is not supported by this adapter"),
            Self::InvalidImage => f.write_str("image contains invalid command input"),
            Self::Spawn(error) => write!(f, "kubectl spawn failed: {error}"),
            Self::MissingOutput => f.write_str("kubectl did not provide piped output"),
            Self::OutputReader => f.write_str("kubectl output reader failed"),
            Self::Timeout => f.write_str("kubectl command timed out"),
            Self::OutputLimit => f.write_str("kubectl output exceeded the configured limit"),
            Self::CommandFailed { status, stderr } => {
                write!(f, "kubectl failed ({status:?}): {stderr}")
            }
            Self::Json(error) => write!(f, "deployment JSON is invalid: {error}"),
            Self::Snapshot(error) => write!(f, "deployment snapshot is invalid: {error}"),
            Self::Kernel(error) => write!(f, "kernel rejected grant: {error}"),
        }
    }
}

impl std::error::Error for AdapterError {}

fn parse_snapshot(bytes: &[u8]) -> Result<DeploymentSnapshot, AdapterError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| AdapterError::Json(error.to_string()))?;
    let metadata = value
        .get("metadata")
        .ok_or_else(|| AdapterError::Json("missing metadata".to_owned()))?;
    let spec = value
        .get("spec")
        .ok_or_else(|| AdapterError::Json("missing spec".to_owned()))?;
    let status = value
        .get("status")
        .ok_or_else(|| AdapterError::Json("missing status".to_owned()))?;
    let namespace = string_field(metadata, "namespace")?;
    let name = string_field(metadata, "name")?;
    let resource_version = string_field(metadata, "resourceVersion")?;
    let desired = integer_field(spec, "replicas")?;
    let ready = status
        .get("readyReplicas")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let image = spec
        .get("template")
        .and_then(|template| template.get("spec"))
        .and_then(|pod_spec| pod_spec.get("containers"))
        .and_then(Value::as_array)
        .and_then(|containers| containers.first())
        .map(|container| string_field(container, "image"))
        .ok_or_else(|| AdapterError::Json("missing first container image".to_owned()))??;
    let desired =
        u32::try_from(desired).map_err(|_| AdapterError::Json("replicas exceed u32".to_owned()))?;
    let ready = u32::try_from(ready)
        .map_err(|_| AdapterError::Json("readyReplicas exceed u32".to_owned()))?;
    if namespace != NAMESPACE || name != DEPLOYMENT {
        return Err(AdapterError::Json(
            "kubectl returned an object outside the adapter allowlist".to_owned(),
        ));
    }
    DeploymentSnapshot::try_new(namespace, name, resource_version, desired, ready, image)
        .map_err(AdapterError::Snapshot)
}

fn string_field(object: &Value, name: &str) -> Result<String, AdapterError> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| AdapterError::Json(format!("missing string field {name}")))
}

fn integer_field(object: &Value, name: &str) -> Result<u64, AdapterError> {
    object
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| AdapterError::Json(format!("missing integer field {name}")))
}

fn validate_image(image: &str) -> Result<(), AdapterError> {
    if image.is_empty()
        || image.len() > 512
        || image.starts_with('-')
        || image.chars().any(char::is_whitespace)
        || image.chars().any(char::is_control)
    {
        return Err(AdapterError::InvalidImage);
    }
    Ok(())
}

struct CommandOutput {
    stdout: Vec<u8>,
}

struct BoundedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn read_bounded(mut reader: impl Read, max: usize) -> BoundedOutput {
    let mut bytes = Vec::with_capacity(max.min(8192));
    let mut buffer = [0_u8; 4096];
    let mut truncated = false;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if bytes.len() < max {
                    let take = count.min(max - bytes.len());
                    bytes.extend_from_slice(&buffer[..take]);
                    truncated |= take < count;
                } else {
                    truncated = true;
                }
            }
            Err(_) => {
                truncated = true;
                break;
            }
        }
    }
    BoundedOutput { bytes, truncated }
}

fn bounded_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn wait_child(
    child: &mut Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, AdapterError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|_| AdapterError::OutputReader)? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AdapterError::Timeout);
        }
        thread::sleep(Duration::from_millis(20));
    }
}
