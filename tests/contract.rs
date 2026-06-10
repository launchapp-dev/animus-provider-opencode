use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use animus_plugin_protocol::HealthStatus;
use animus_provider_opencode::backend::OpenCodeProviderBackend;
use animus_provider_opencode::config::OpenCodeConfig;
use animus_provider_protocol::{
    AgentNotification, AgentRunRequest, NotificationSink, ProviderBackend,
    NOTIFICATION_AGENT_ERROR, NOTIFICATION_AGENT_OUTPUT, NOTIFICATION_AGENT_THINKING,
    NOTIFICATION_AGENT_TOOL_CALL, NOTIFICATION_AGENT_TOOL_RESULT,
};
use animus_session_backend::{
    Error as SessionError, Result as SessionResult, SessionBackend, SessionBackendInfo,
    SessionBackendKind, SessionCapabilities, SessionEvent, SessionRequest, SessionRun,
    SessionStability,
};
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Fake `SessionBackend` that replays a canned event script and records the
/// last request it was called with.
#[derive(Clone)]
struct FakeSession {
    events: Arc<Mutex<Vec<SessionEvent>>>,
    last_request: Arc<Mutex<Option<SessionRequest>>>,
    last_resume_id: Arc<Mutex<Option<String>>>,
    last_cancel_id: Arc<Mutex<Option<String>>>,
    capabilities: SessionCapabilities,
}

impl FakeSession {
    fn with_events(events: Vec<SessionEvent>) -> Self {
        Self {
            events: Arc::new(Mutex::new(events)),
            last_request: Arc::new(Mutex::new(None)),
            last_resume_id: Arc::new(Mutex::new(None)),
            last_cancel_id: Arc::new(Mutex::new(None)),
            capabilities: SessionCapabilities {
                supports_resume: true,
                supports_terminate: true,
                supports_permissions: true,
                supports_mcp: true,
                supports_tool_events: true,
                supports_thinking_events: true,
                supports_artifact_events: true,
                supports_usage_metadata: true,
            },
        }
    }

    fn build_run(&self) -> SessionRun {
        let events: Vec<SessionEvent> = self.events.lock().unwrap().clone();
        let (tx, rx) = mpsc::channel(events.len().max(1));
        for ev in events {
            tx.try_send(ev).expect("test channel large enough");
        }
        drop(tx);
        SessionRun {
            session_id: Some("fake-session-1".to_string()),
            events: rx,
            selected_backend: "opencode-fake".to_string(),
            fallback_reason: None,
            pid: None,
        }
    }
}

#[async_trait]
impl SessionBackend for FakeSession {
    fn info(&self) -> SessionBackendInfo {
        SessionBackendInfo {
            kind: SessionBackendKind::OpenCodeSdk,
            provider_tool: "opencode".to_string(),
            stability: SessionStability::Experimental,
            display_name: "fake-opencode".to_string(),
        }
    }

    fn capabilities(&self) -> SessionCapabilities {
        self.capabilities.clone()
    }

    async fn start_session(&self, request: SessionRequest) -> SessionResult<SessionRun> {
        *self.last_request.lock().unwrap() = Some(request);
        Ok(self.build_run())
    }

    async fn resume_session(
        &self,
        request: SessionRequest,
        session_id: &str,
    ) -> SessionResult<SessionRun> {
        *self.last_request.lock().unwrap() = Some(request);
        *self.last_resume_id.lock().unwrap() = Some(session_id.to_string());
        Ok(self.build_run())
    }

    async fn terminate_session(&self, session_id: &str) -> SessionResult<()> {
        *self.last_cancel_id.lock().unwrap() = Some(session_id.to_string());
        Ok(())
    }
}

/// A second fake that always fails terminate — used to confirm errors are
/// propagated.
#[derive(Clone)]
struct AlwaysFailTerminate;

#[async_trait]
impl SessionBackend for AlwaysFailTerminate {
    fn info(&self) -> SessionBackendInfo {
        SessionBackendInfo {
            kind: SessionBackendKind::OpenCodeSdk,
            provider_tool: "opencode".to_string(),
            stability: SessionStability::Experimental,
            display_name: "always-fail".to_string(),
        }
    }
    fn capabilities(&self) -> SessionCapabilities {
        SessionCapabilities {
            supports_resume: true,
            supports_terminate: true,
            supports_permissions: false,
            supports_mcp: false,
            supports_tool_events: false,
            supports_thinking_events: false,
            supports_artifact_events: false,
            supports_usage_metadata: false,
        }
    }
    async fn start_session(&self, _request: SessionRequest) -> SessionResult<SessionRun> {
        Err(SessionError::ExecutionFailed("nope".into()))
    }
    async fn resume_session(
        &self,
        _request: SessionRequest,
        _session_id: &str,
    ) -> SessionResult<SessionRun> {
        Err(SessionError::ExecutionFailed("nope".into()))
    }
    async fn terminate_session(&self, _session_id: &str) -> SessionResult<()> {
        Err(SessionError::ExecutionFailed("terminate failed".into()))
    }
}

fn run_request(session_id: Option<&str>, model: Option<&str>, prompt: &str) -> AgentRunRequest {
    AgentRunRequest {
        session_id: session_id.map(|s| s.to_string()),
        prompt: prompt.to_string(),
        model: model.map(|s| s.to_string()),
        system_prompt: None,
        cwd: PathBuf::from("/tmp"),
        project_root: None,
        permission_mode: None,
        timeout_secs: None,
        env: HashMap::new(),
        mcp_servers: None,
        tools: None,
        response_schema: None,
        runtime_contract: None,
        extras: HashMap::new(),
    }
}

#[tokio::test]
async fn run_agent_via_fake_session() {
    let fake = FakeSession::with_events(vec![
        SessionEvent::Started {
            backend: "opencode-fake".into(),
            session_id: Some("fake-session-1".into()),
            pid: Some(42),
        },
        SessionEvent::TextDelta {
            text: "hello".into(),
        },
        SessionEvent::TextDelta {
            text: " world".into(),
        },
        SessionEvent::ToolCall {
            tool_name: "read_file".into(),
            arguments: serde_json::json!({ "path": "/etc/hosts" }),
            server: None,
        },
        SessionEvent::ToolResult {
            tool_name: "read_file".into(),
            output: serde_json::json!("contents"),
            success: true,
        },
        SessionEvent::Thinking {
            text: "thinking out loud".into(),
        },
        SessionEvent::FinalText {
            text: "FINAL".into(),
        },
        SessionEvent::Finished { exit_code: Some(0) },
    ]);

    let backend = OpenCodeProviderBackend::with_session(
        fake.clone(),
        OpenCodeConfig::for_testing("opencode"),
    );

    let response = backend
        .run_agent(run_request(None, Some("gpt-5.2"), "ping"))
        .await
        .expect("run_agent should succeed");

    assert_eq!(response.session_id, "fake-session-1");
    assert_eq!(response.exit_code, 0);
    assert_eq!(response.output, "FINAL");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_results.len(), 1);
    assert_eq!(response.thinking, vec!["thinking out loud".to_string()]);
    assert!(response.backend.contains("opencode-fake"));

    let sent = fake
        .last_request
        .lock()
        .unwrap()
        .clone()
        .expect("request recorded");
    assert_eq!(sent.tool, "opencode");
    assert_eq!(sent.model, "gpt-5.2");
    assert_eq!(sent.prompt, "ping");
    assert_eq!(sent.cwd, PathBuf::from("/tmp"));
}

#[tokio::test]
async fn resume_agent_via_fake_session() {
    let fake = FakeSession::with_events(vec![
        SessionEvent::Started {
            backend: "opencode-fake".into(),
            session_id: Some("fake-session-1".into()),
            pid: None,
        },
        SessionEvent::FinalText {
            text: "resumed".into(),
        },
        SessionEvent::Finished { exit_code: Some(0) },
    ]);

    let backend = OpenCodeProviderBackend::with_session(
        fake.clone(),
        OpenCodeConfig::for_testing("opencode"),
    );

    let response = backend
        .resume_agent(run_request(Some("prev-id-xyz"), Some("gpt-5.2"), "more"))
        .await
        .expect("resume should succeed");

    assert_eq!(response.output, "resumed");
    assert_eq!(
        fake.last_resume_id.lock().unwrap().clone().as_deref(),
        Some("prev-id-xyz")
    );
}

#[tokio::test]
async fn resume_agent_without_session_id_errors() {
    let fake = FakeSession::with_events(vec![]);
    let backend =
        OpenCodeProviderBackend::with_session(fake, OpenCodeConfig::for_testing("opencode"));

    let err = backend
        .resume_agent(run_request(None, Some("gpt-5.2"), "x"))
        .await
        .expect_err("resume without session_id should fail");
    let msg = format!("{err}");
    assert!(msg.contains("session_id"), "unexpected resume error: {msg}");
}

#[tokio::test]
async fn cancel_agent_forwards_session_id() {
    let fake = FakeSession::with_events(vec![]);
    let backend = OpenCodeProviderBackend::with_session(
        fake.clone(),
        OpenCodeConfig::for_testing("opencode"),
    );

    backend
        .cancel_agent("session-to-kill")
        .await
        .expect("cancel should succeed");

    assert_eq!(
        fake.last_cancel_id.lock().unwrap().clone().as_deref(),
        Some("session-to-kill")
    );
}

#[tokio::test]
async fn cancel_agent_propagates_errors() {
    let backend = OpenCodeProviderBackend::with_session(
        AlwaysFailTerminate,
        OpenCodeConfig::for_testing("opencode"),
    );
    let err = backend
        .cancel_agent("anything")
        .await
        .expect_err("cancel should fail");
    let msg = format!("{err}");
    assert!(msg.contains("terminate failed") || msg.contains("opencode"));
}

#[tokio::test]
async fn health_unhealthy_when_opencode_missing() {
    let backend = OpenCodeProviderBackend::new(OpenCodeConfig::for_testing(
        "/nonexistent/opencode-binary-xyz",
    ));
    let health = backend
        .health()
        .await
        .expect("health call should not error");
    assert_eq!(health.status, HealthStatus::Unhealthy);
    assert!(health.last_error.is_some());
}

#[tokio::test]
async fn health_healthy_when_opencode_on_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub_path = dir.path().join("opencode");
    std::fs::write(&stub_path, "#!/bin/sh\nexit 0\n").expect("write stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&stub_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub_path, perms).unwrap();
    }

    let old_path = std::env::var_os("PATH");
    let new_path = match &old_path {
        Some(p) => {
            let mut combined = std::ffi::OsString::from(dir.path());
            combined.push(":");
            combined.push(p);
            combined
        }
        None => std::ffi::OsString::from(dir.path()),
    };
    // SAFETY: tests in this crate are sequential by default for env mutation;
    // we restore PATH at the end.
    std::env::set_var("PATH", &new_path);

    let backend = OpenCodeProviderBackend::new(OpenCodeConfig::for_testing("opencode"));
    let health = backend
        .health()
        .await
        .expect("health call should not error");

    // Restore PATH regardless of assert outcome.
    match old_path {
        Some(p) => std::env::set_var("PATH", p),
        None => std::env::remove_var("PATH"),
    }

    assert_eq!(
        health.status,
        HealthStatus::Healthy,
        "last_error: {:?}",
        health.last_error
    );
    assert!(health.last_error.is_none());
}

#[tokio::test]
async fn run_agent_streaming_emits_notifications_in_order() {
    let fake = FakeSession::with_events(vec![
        SessionEvent::Started {
            backend: "opencode-fake".into(),
            session_id: Some("fake-session-1".into()),
            pid: Some(7),
        },
        SessionEvent::TextDelta { text: "hel".into() },
        SessionEvent::TextDelta { text: "lo".into() },
        SessionEvent::Thinking {
            text: "ponder".into(),
        },
        SessionEvent::ToolCall {
            tool_name: "shell".into(),
            arguments: serde_json::json!({ "cmd": "ls" }),
            server: Some("local".into()),
        },
        SessionEvent::ToolResult {
            tool_name: "shell".into(),
            output: serde_json::json!({ "stdout": "Cargo.toml" }),
            success: true,
        },
        SessionEvent::Error {
            message: "soft fail".into(),
            recoverable: true,
        },
        SessionEvent::FinalText {
            text: "hello FINAL".into(),
        },
        SessionEvent::Finished { exit_code: Some(0) },
    ]);

    let backend = OpenCodeProviderBackend::with_session(
        fake.clone(),
        OpenCodeConfig::for_testing("opencode"),
    );

    let recorder: Arc<Mutex<Vec<AgentNotification>>> = Arc::new(Mutex::new(Vec::new()));
    let r2 = recorder.clone();
    let sink = NotificationSink::new(move |n| r2.lock().unwrap().push(n));

    let response = backend
        .run_agent_streaming(run_request(None, Some("gpt-5.2"), "ping"), sink)
        .await
        .expect("streaming run should succeed");

    assert_eq!(response.session_id, "fake-session-1");
    assert_eq!(response.exit_code, 0);
    assert_eq!(response.output, "hello FINAL");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_results.len(), 1);
    assert_eq!(response.thinking, vec!["ponder".to_string()]);
    assert_eq!(response.errors, vec!["soft fail".to_string()]);

    let recorded = recorder.lock().unwrap().clone();
    let methods: Vec<&str> = recorded.iter().map(|n| n.method()).collect();
    assert_eq!(
        methods,
        vec![
            NOTIFICATION_AGENT_OUTPUT,
            NOTIFICATION_AGENT_OUTPUT,
            NOTIFICATION_AGENT_THINKING,
            NOTIFICATION_AGENT_TOOL_CALL,
            NOTIFICATION_AGENT_TOOL_RESULT,
            NOTIFICATION_AGENT_ERROR,
            NOTIFICATION_AGENT_OUTPUT,
        ],
        "unexpected notification order"
    );

    match &recorded[0] {
        AgentNotification::Output {
            session_id,
            text,
            is_final,
        } => {
            assert_eq!(session_id, "fake-session-1");
            assert_eq!(text, "hel");
            assert!(!is_final);
        }
        other => panic!("expected first emission to be Output, got {other:?}"),
    }
    match &recorded[6] {
        AgentNotification::Output { text, is_final, .. } => {
            assert_eq!(text, "hello FINAL");
            assert!(is_final, "FinalText should map to is_final=true");
        }
        other => panic!("expected last emission to be final Output, got {other:?}"),
    }
    match &recorded[3] {
        AgentNotification::ToolCall {
            name,
            arguments,
            server,
            ..
        } => {
            assert_eq!(name, "shell");
            assert_eq!(arguments["cmd"], "ls");
            assert_eq!(server.as_deref(), Some("local"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
    match &recorded[4] {
        AgentNotification::ToolResult {
            name,
            output,
            success,
            ..
        } => {
            assert_eq!(name, "shell");
            assert_eq!(output["stdout"], "Cargo.toml");
            assert!(success);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
    match &recorded[5] {
        AgentNotification::Error {
            message,
            recoverable,
            ..
        } => {
            assert_eq!(message, "soft fail");
            assert!(*recoverable);
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[tokio::test]
async fn run_agent_and_streaming_produce_same_response() {
    fn events() -> Vec<SessionEvent> {
        vec![
            SessionEvent::Started {
                backend: "opencode-fake".into(),
                session_id: Some("fake-session-1".into()),
                pid: None,
            },
            SessionEvent::TextDelta { text: "a".into() },
            SessionEvent::TextDelta { text: "b".into() },
            SessionEvent::ToolCall {
                tool_name: "read_file".into(),
                arguments: serde_json::json!({"path": "/x"}),
                server: None,
            },
            SessionEvent::ToolResult {
                tool_name: "read_file".into(),
                output: serde_json::json!("ok"),
                success: true,
            },
            SessionEvent::FinalText {
                text: "DONE".into(),
            },
            SessionEvent::Finished { exit_code: Some(0) },
        ]
    }

    let backend_a = OpenCodeProviderBackend::with_session(
        FakeSession::with_events(events()),
        OpenCodeConfig::for_testing("opencode"),
    );
    let resp_a = backend_a
        .run_agent(run_request(None, Some("gpt-5.2"), "p"))
        .await
        .expect("run_agent");

    let backend_b = OpenCodeProviderBackend::with_session(
        FakeSession::with_events(events()),
        OpenCodeConfig::for_testing("opencode"),
    );
    let resp_b = backend_b
        .run_agent_streaming(
            run_request(None, Some("gpt-5.2"), "p"),
            NotificationSink::noop(),
        )
        .await
        .expect("run_agent_streaming");

    assert_eq!(resp_a.session_id, resp_b.session_id);
    assert_eq!(resp_a.exit_code, resp_b.exit_code);
    assert_eq!(resp_a.output, resp_b.output);
    assert_eq!(resp_a.tool_calls, resp_b.tool_calls);
    assert_eq!(resp_a.tool_results, resp_b.tool_results);
    assert_eq!(resp_a.thinking, resp_b.thinking);
    assert_eq!(resp_a.errors, resp_b.errors);
    assert_eq!(resp_a.backend, resp_b.backend);
}

#[tokio::test]
async fn manifest_capabilities_sanity() {
    let backend = OpenCodeProviderBackend::new(OpenCodeConfig::for_testing("opencode"));
    let manifest = backend.manifest();
    assert_eq!(manifest.name, "animus-provider-opencode");
    assert_eq!(manifest.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest.tool, "opencode");
    assert!(manifest.capabilities.streaming);
    assert!(manifest.capabilities.resume);
    assert!(manifest.capabilities.cancellation);
    assert!(manifest.capabilities.write_capable);
    assert!(manifest.capabilities.mcp);
    assert!(
        manifest
            .supported_models
            .iter()
            .any(|m| m == "openai/gpt-5.2"),
        "expected openai/gpt-5.2 in supported_models: {:?}",
        manifest.supported_models
    );
}
