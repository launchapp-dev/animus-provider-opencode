//! `animus-provider-opencode` — the Animus provider for OpenCode.
//!
//! A thin wrapper over the shared ACP client (`animus-provider-acp`). It
//! advertises `provider_tool = "opencode"` and pins the harness to
//! `opencode acp`, so the kernel routes OpenCode models here exactly as before
//! while the plugin drives the OpenCode CLI over the Agent Client Protocol
//! (structured streaming + a native permission callback) instead of scraping
//! stdout. Every tool call is gated through `animus agent approve-hook` by the
//! ACP client.

use std::sync::Arc;

use animus_plugin_runtime::{run_provider, ProviderInfo, SessionBackendProvider};
use animus_provider_acp::backend::AcpSessionBackend;
use animus_provider_acp::config::AcpConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    emit_manifest_if_requested();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // `OPENCODE_DEFAULT_MODEL` overrides the fallback model (empty lets OpenCode
    // pick its configured default); `OPENCODE_BIN` overrides the harness binary
    // (default `opencode`). The harness is always driven in ACP mode (`acp`).
    let default_model = std::env::var("OPENCODE_DEFAULT_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_default();
    let bin = std::env::var("OPENCODE_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "opencode".to_string());

    let config = AcpConfig::for_harness("opencode", bin, ["acp"], default_model.clone());
    let backend = Arc::new(AcpSessionBackend::new(config));

    // `ProviderInfo` fields are `&'static str`; leak the (process-lifetime)
    // default model so an `OPENCODE_DEFAULT_MODEL` override is honored.
    let default_model: &'static str = Box::leak(default_model.into_boxed_str());

    let info = ProviderInfo {
        plugin_name: env!("CARGO_PKG_NAME"),
        plugin_version: env!("CARGO_PKG_VERSION"),
        description: env!("CARGO_PKG_DESCRIPTION"),
        default_tool: "opencode",
        default_model,
    };

    run_provider(info, SessionBackendProvider::new(backend)).await
}

fn emit_manifest_if_requested() {
    if !std::env::args()
        .skip(1)
        .any(|arg| arg == "--manifest" || arg == "-m")
    {
        return;
    }

    let manifest = serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "plugin_kind": "provider",
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "protocol_version": animus_plugin_protocol::PROTOCOL_VERSION,
        "capabilities": [
            "agent/run",
            "agent/resume",
            "agent/cancel",
            "health/check"
        ],
        "env_required": [
            {
                "name": "OPENCODE_BIN",
                "description": "Override the OpenCode CLI binary (default `opencode`). Driven in ACP mode via the `acp` subcommand.",
                "required": false
            },
            {
                "name": "OPENCODE_DEFAULT_MODEL",
                "description": "Fallback model used when an agent/run request omits a model.",
                "required": false
            },
            {
                "name": "OPENAI_API_KEY",
                "description": "API key for OpenAI-compatible models routed through OpenCode.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "OPENROUTER_API_KEY",
                "description": "OpenRouter API key used by some OpenCode providers.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "ANIMUS_BIN",
                "description": "Path to the `animus` binary used for the approve-hook approval gate (default: resolved on PATH).",
                "required": false
            }
        ]
    });
    println!(
        "{}",
        serde_json::to_string(&manifest).expect("serialize manifest")
    );
    std::process::exit(0);
}
