use std::sync::Arc;
use std::time::Instant;

use animus_plugin_protocol::{HealthCheckResult, HealthStatus};
use animus_provider_protocol::{
    AgentNotification, AgentResumeRequest, AgentRunRequest, AgentRunResponse, BackendError,
    NotificationSink, ProviderBackend, ProviderCapabilities, ProviderManifest,
};
use animus_session_backend::{
    lookup_binary_in_path, OpenCodeSessionBackend, SessionBackend, SessionEvent, SessionRequest,
    SessionRun,
};
use async_trait::async_trait;
use serde_json::Value;

use crate::config::OpenCodeConfig;

/// Provider backend that wraps a `SessionBackend` (defaulting to the native
/// `OpenCodeSessionBackend`) so the same plugin can be exercised in tests
/// against a fake session backend without spawning a real `opencode` CLI.
pub struct OpenCodeProviderBackend {
    session: Arc<dyn SessionBackend>,
    config: OpenCodeConfig,
}

impl OpenCodeProviderBackend {
    /// Construct with the native `OpenCodeSessionBackend`.
    pub fn new(config: OpenCodeConfig) -> Self {
        Self {
            session: Arc::new(OpenCodeSessionBackend::new()),
            config,
        }
    }

    /// Construct with a caller-supplied `SessionBackend`. Test helper.
    pub fn with_session<S>(session: S, config: OpenCodeConfig) -> Self
    where
        S: SessionBackend + 'static,
    {
        Self {
            session: Arc::new(session),
            config,
        }
    }

    fn build_session_request(&self, request: &AgentRunRequest) -> SessionRequest {
        let model = request
            .model
            .clone()
            .unwrap_or_else(|| self.config.default_model.clone());
        let prompt = if let Some(system) = &request.system_prompt {
            format!("{system}\n\n{}", request.prompt)
        } else {
            request.prompt.clone()
        };
        let mcp_endpoint = request.mcp_servers.as_ref().and_then(extract_mcp_endpoint);
        let env_vars: Vec<(String, String)> = request
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let extras = if request.extras.is_empty() {
            request
                .runtime_contract
                .clone()
                .unwrap_or(Value::Object(Default::default()))
        } else {
            let map: serde_json::Map<String, Value> = request
                .extras
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut merged = Value::Object(map);
            if let (Some(obj), Some(contract)) = (merged.as_object_mut(), &request.runtime_contract)
            {
                obj.insert("runtime_contract".to_string(), contract.clone());
            }
            merged
        };

        SessionRequest {
            tool: "opencode".to_string(),
            model,
            prompt,
            cwd: request.cwd.clone(),
            project_root: request.project_root.clone(),
            mcp_endpoint,
            permission_mode: request.permission_mode.clone(),
            timeout_secs: request.timeout_secs,
            env_vars,
            extras,
        }
    }

    async fn drain_run(
        &self,
        mut run: SessionRun,
        started: Instant,
        sink: NotificationSink,
    ) -> Result<AgentRunResponse, BackendError> {
        let mut output_buf = String::new();
        let mut final_text: Option<String> = None;
        let mut metadata: Vec<Value> = Vec::new();
        let mut tool_calls: Vec<Value> = Vec::new();
        let mut tool_results: Vec<Value> = Vec::new();
        let mut thinking: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut exit_code: i32 = 0;
        let mut runtime_session_id = run.session_id.clone();
        // Resolved up-front so streaming notifications always carry a stable id,
        // even for events that arrive before `Started`.
        let stream_session_id = runtime_session_id.clone().unwrap_or_else(uuid::new_v4_like);

        while let Some(event) = run.events.recv().await {
            match event {
                SessionEvent::Started { session_id, .. } => {
                    if runtime_session_id.is_none() {
                        runtime_session_id = session_id;
                    }
                }
                SessionEvent::TextDelta { text } => {
                    sink.emit(AgentNotification::Output {
                        session_id: stream_session_id.clone(),
                        text: text.clone(),
                        is_final: false,
                    });
                    output_buf.push_str(&text);
                }
                SessionEvent::FinalText { text } => {
                    sink.emit(AgentNotification::Output {
                        session_id: stream_session_id.clone(),
                        text: text.clone(),
                        is_final: true,
                    });
                    final_text = Some(text);
                }
                SessionEvent::ToolCall {
                    tool_name,
                    arguments,
                    server,
                } => {
                    sink.emit(AgentNotification::ToolCall {
                        session_id: stream_session_id.clone(),
                        name: tool_name.clone(),
                        arguments: arguments.clone(),
                        server: server.clone(),
                    });
                    tool_calls.push(serde_json::json!({
                        "tool_name": tool_name,
                        "arguments": arguments,
                        "server": server,
                    }));
                }
                SessionEvent::ToolResult {
                    tool_name,
                    output,
                    success,
                } => {
                    sink.emit(AgentNotification::ToolResult {
                        session_id: stream_session_id.clone(),
                        name: tool_name.clone(),
                        output: output.clone(),
                        success,
                    });
                    tool_results.push(serde_json::json!({
                        "tool_name": tool_name,
                        "output": output,
                        "success": success,
                    }));
                }
                SessionEvent::Thinking { text } => {
                    sink.emit(AgentNotification::Thinking {
                        session_id: stream_session_id.clone(),
                        text: text.clone(),
                    });
                    thinking.push(text);
                }
                SessionEvent::Artifact {
                    artifact_id,
                    metadata: meta,
                } => {
                    metadata.push(serde_json::json!({
                        "kind": "artifact",
                        "artifact_id": artifact_id,
                        "metadata": meta,
                    }));
                }
                SessionEvent::Metadata { metadata: meta } => {
                    metadata.push(meta);
                }
                SessionEvent::Error {
                    message,
                    recoverable,
                } => {
                    sink.emit(AgentNotification::Error {
                        session_id: stream_session_id.clone(),
                        message: message.clone(),
                        recoverable,
                    });
                    errors.push(message);
                    if !recoverable {
                        exit_code = 1;
                    }
                }
                SessionEvent::Finished { exit_code: code } => {
                    if let Some(c) = code {
                        exit_code = c;
                    }
                }
            }
        }

        let output = final_text.unwrap_or(output_buf);
        let session_id = runtime_session_id.unwrap_or(stream_session_id);
        let backend_label = format!("opencode:{}", run.selected_backend);

        Ok(AgentRunResponse {
            session_id,
            exit_code,
            output,
            metadata,
            tool_calls,
            tool_results,
            thinking,
            errors,
            duration_ms: started.elapsed().as_millis() as u64,
            backend: backend_label,
            tokens_used: None,
            decision_verdict: None,
        })
    }
}

fn extract_mcp_endpoint(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => map
            .get("endpoint")
            .or_else(|| map.get("url"))
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        _ => None,
    }
}

// Tiny dependency-free pseudo-uuid generator. We only need a unique-ish
// fallback session id when the underlying session backend never assigned
// one; we deliberately avoid pulling in the `uuid` crate just for this.
mod uuid {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub fn new_v4_like() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("opencode-session-{now:016x}-{seq:08x}")
    }
}

#[async_trait]
impl ProviderBackend for OpenCodeProviderBackend {
    fn manifest(&self) -> ProviderManifest {
        let info = self.session.info();
        let caps = self.session.capabilities();
        ProviderManifest {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: env!("CARGO_PKG_DESCRIPTION").to_string(),
            supported_models: vec![
                "gpt-5.2".to_string(),
                "gpt-5".to_string(),
                "claude-sonnet-4-6".to_string(),
                "claude-opus-4-7".to_string(),
            ],
            tool: info.provider_tool.clone(),
            capabilities: ProviderCapabilities {
                streaming: true,
                resume: caps.supports_resume,
                cancellation: caps.supports_terminate,
                write_capable: true,
                mcp: caps.supports_mcp,
            },
        }
    }

    async fn run_agent(&self, request: AgentRunRequest) -> Result<AgentRunResponse, BackendError> {
        self.run_agent_streaming(request, NotificationSink::noop())
            .await
    }

    async fn run_agent_streaming(
        &self,
        request: AgentRunRequest,
        sink: NotificationSink,
    ) -> Result<AgentRunResponse, BackendError> {
        let started = Instant::now();
        let session_request = self.build_session_request(&request);
        let run = self
            .session
            .start_session(session_request)
            .await
            .map_err(map_session_error)?;
        self.drain_run(run, started, sink).await
    }

    async fn resume_agent(
        &self,
        request: AgentResumeRequest,
    ) -> Result<AgentRunResponse, BackendError> {
        self.resume_agent_streaming(request, NotificationSink::noop())
            .await
    }

    async fn resume_agent_streaming(
        &self,
        request: AgentResumeRequest,
        sink: NotificationSink,
    ) -> Result<AgentRunResponse, BackendError> {
        let started = Instant::now();
        let session_id = request.session_id.clone().ok_or_else(|| {
            BackendError::Other(anyhow::anyhow!(
                "opencode: resume requires a session_id on the request"
            ))
        })?;
        let session_request = self.build_session_request(&request);
        let run = self
            .session
            .resume_session(session_request, &session_id)
            .await
            .map_err(map_session_error)?;
        self.drain_run(run, started, sink).await
    }

    async fn cancel_agent(&self, session_id: &str) -> Result<(), BackendError> {
        self.session
            .terminate_session(session_id)
            .await
            .map_err(map_session_error)
    }

    async fn health(&self) -> Result<HealthCheckResult, BackendError> {
        match lookup_binary_in_path(&self.config.opencode_bin) {
            Some(_) => Ok(HealthCheckResult {
                status: HealthStatus::Healthy,
                uptime_ms: None,
                memory_usage_bytes: None,
                last_error: None,
            }),
            None => Ok(HealthCheckResult {
                status: HealthStatus::Unhealthy,
                uptime_ms: None,
                memory_usage_bytes: None,
                last_error: Some(format!(
                    "opencode binary not found on PATH: {}",
                    self.config.opencode_bin
                )),
            }),
        }
    }
}

fn map_session_error(err: animus_session_backend::Error) -> BackendError {
    use animus_session_backend::Error as E;
    match err {
        E::CliNotFound(msg) => BackendError::Unavailable(format!("opencode CLI not found: {msg}")),
        E::ExecutionFailed(msg) => BackendError::RunFailed(format!("opencode execution: {msg}")),
        E::ValidationFailed(msg) => {
            BackendError::Other(anyhow::anyhow!("opencode validation: {msg}"))
        }
        E::IoError(io) => BackendError::RunFailed(format!("opencode io: {io}")),
        E::SerializationError(msg) => {
            BackendError::Other(anyhow::anyhow!("opencode serialization: {msg}"))
        }
        E::Other(other) => BackendError::Other(other),
    }
}
