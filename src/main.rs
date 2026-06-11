use animus_plugin_protocol::{PluginInfo, PLUGIN_KIND_PROVIDER};
use animus_plugin_runtime::provider_main_with_capabilities;
use animus_provider_opencode::backend::OpenCodeProviderBackend;
use animus_provider_opencode::config::OpenCodeConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    emit_manifest_if_requested();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let config = OpenCodeConfig::from_env()?;
    let backend = OpenCodeProviderBackend::new(config);

    let info = PluginInfo {
        name: env!("CARGO_PKG_NAME").into(),
        version: env!("CARGO_PKG_VERSION").into(),
        plugin_kind: PLUGIN_KIND_PROVIDER.into(),
        description: Some(env!("CARGO_PKG_DESCRIPTION").into()),
    };

    // opencode supports mid-flight cancel: the session manager terminates the
    // opencode CLI subprocess and the wrapper emits SessionEvent::Error with
    // recoverable=false, which becomes the AgentNotification::Error{
    // recoverable:false} the testkit accepts as a valid cancel signal.
    let extra_capabilities = vec!["$harness/cancellation-loop-v2".to_string()];

    provider_main_with_capabilities(info, backend, extra_capabilities).await
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
                "description": "Override the OpenCode CLI binary path.",
                "required": false
            },
            {
                "name": "OPENCODE_DEFAULT_MODEL",
                "description": "Fallback model used when the request omits a model.",
                "required": false
            },
            {
                "name": "OPENAI_API_KEY",
                "description": "OpenAI API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "ANTHROPIC_API_KEY",
                "description": "Anthropic API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "GEMINI_API_KEY",
                "description": "Gemini API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "GOOGLE_API_KEY",
                "description": "Google API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "OPENROUTER_API_KEY",
                "description": "OpenRouter API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "GROQ_API_KEY",
                "description": "Groq API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "TOGETHER_API_KEY",
                "description": "Together API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "MISTRAL_API_KEY",
                "description": "Mistral API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "FIREWORKS_API_KEY",
                "description": "Fireworks API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "DEEPSEEK_API_KEY",
                "description": "DeepSeek API key forwarded to OpenCode when configured.",
                "sensitive": true,
                "required": false
            },
            {
                "name": "OPENCODE_CONFIG_CONTENT",
                "description": "Inline OpenCode config JSON the session backend injects per run.",
                "sensitive": true,
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
