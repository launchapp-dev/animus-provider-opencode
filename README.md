# animus-provider-opencode

An [OpenCode](https://opencode.ai) provider plugin for [Animus](https://github.com/launchapp-dev/animus-cli).

## What this is

A stdio provider plugin that exposes the OpenCode CLI as an Animus provider. Any workflow phase that targets `tool: opencode` dispatches through this plugin.

As of v0.3.0 it drives the OpenCode CLI over the **Agent Client Protocol (ACP)** — `opencode acp` — rather than scraping stdout. It is a thin wrapper over the shared ACP client ([`animus-provider-acp`](https://github.com/launchapp-dev/animus-provider-acp)), pinned to the OpenCode harness and advertising `provider_tool = "opencode"`. This gives structured streaming + tool events and a **native permission callback**, with every tool call gated through `animus agent approve-hook`. ACP is an internal transport detail; the kernel still routes OpenCode models to `tool: opencode` exactly as before.

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
