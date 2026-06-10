use anyhow::Result;

/// Runtime configuration for the OpenCode provider plugin.
///
/// Reads:
/// - `OPENCODE_BIN` — binary name or absolute path for the `opencode` CLI
///   (default: `"opencode"`).
/// - `OPENCODE_DEFAULT_MODEL` — model identifier passed through when an
///   `AgentRunRequest` does not specify one, in opencode's `provider/model`
///   format (e.g. `"openai/gpt-5.2"`). Empty (the default) lets the opencode
///   CLI use its own configured default model.
#[derive(Debug, Clone)]
pub struct OpenCodeConfig {
    pub opencode_bin: String,
    pub default_model: String,
}

impl OpenCodeConfig {
    pub fn from_env() -> Result<Self> {
        let opencode_bin = std::env::var("OPENCODE_BIN").unwrap_or_else(|_| "opencode".to_string());
        let default_model = std::env::var("OPENCODE_DEFAULT_MODEL").unwrap_or_default();

        Ok(Self {
            opencode_bin,
            default_model,
        })
    }

    /// Helper for integration tests / embedders that want to construct a
    /// config without going through env vars.
    pub fn for_testing(opencode_bin: impl Into<String>) -> Self {
        Self {
            opencode_bin: opencode_bin.into(),
            default_model: String::new(),
        }
    }
}

impl Default for OpenCodeConfig {
    fn default() -> Self {
        Self {
            opencode_bin: "opencode".to_string(),
            default_model: String::new(),
        }
    }
}
