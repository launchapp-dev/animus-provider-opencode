# animus-provider-opencode

An [OpenCode](https://opencode.ai) provider plugin for [Animus](https://github.com/launchapp-dev/animus-cli).

> **Status:** Under construction — landing in Animus v0.4.x. This crate currently lives in the Animus core workspace at `crates/animus-provider-opencode/`; v0.4.x extracts it to this standalone repository.

## What this is

Animus v0.4.0 makes providers (LLM CLI wrappers) pluggable. This repository will ship `animus-provider-opencode`, a stdio plugin that wraps the OpenCode CLI as an Animus provider. Any workflow phase that targets `tool: opencode` dispatches through this plugin.

## Install (planned)

```bash
animus plugin install animus-provider-opencode
```

The Animus daemon image bundles this plugin pre-installed.

## Workflow YAML usage

```yaml
agents:
  default:
    tool: opencode
    mcp_servers: ["animus"]
```

## Roadmap

- [ ] Extract from Animus core workspace at v0.4.x cut
- [ ] Publish `animus-provider-opencode` crate to crates.io
- [ ] Release binaries (macOS aarch64/x86_64, Linux x86_64) on tag
- [ ] Independent semver track
- [ ] CI exercises the contract test from `animus-protocol`

## Design

- **Protocol:** [`animus-plugin-protocol`](https://github.com/launchapp-dev/animus-protocol) (provider variant)
- **Naming:** repo, crate, and binary all named `animus-provider-opencode`
- **Core repo:** [Animus](https://github.com/launchapp-dev/animus-cli)

## License

MIT — see [LICENSE](LICENSE).
