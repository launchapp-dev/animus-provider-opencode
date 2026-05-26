use animus_plugin_protocol::{PluginInfo, PLUGIN_KIND_PROVIDER};
use animus_plugin_runtime::provider_main_with_capabilities;
use animus_provider_opencode::backend::OpenCodeProviderBackend;
use animus_provider_opencode::config::OpenCodeConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
