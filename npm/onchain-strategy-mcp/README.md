# onchain-strategy-mcp

npm wrapper for the `executor-mcp` runtime — a Claude Code MCP server that
executes agent-authored onchain strategies on Base from a local burner wallet.

## Quick start

```bash
npx onchain-strategy-mcp init
```

This downloads the prebuilt `executor-mcp` binary for your platform into
`~/.osmcp/bin/`, generates a fresh burner wallet, stores its private key in
the OS keychain, and scaffolds `./.local/config.toml` + `./.local/policy.toml`.

Then register the MCP with Claude Code:

```bash
claude mcp add osmcp -- npx onchain-strategy-mcp serve
```

## Commands

| Command | What it does |
|---------|--------------|
| `npx onchain-strategy-mcp init`    | First-time setup (wallet + config + binary fetch) |
| `npx onchain-strategy-mcp serve`   | Run the MCP server (invoked by Claude Code) |
| `npx onchain-strategy-mcp --version` | Print version |

`osmcp` is a short alias for `onchain-strategy-mcp`.

## Supported platforms

- macOS arm64 (Apple Silicon)
- macOS x64 (Intel)
- Linux x64

Windows users: please build from source until v1.4.

## Full documentation

See the repository README:
https://github.com/forrestkim-iotrust/onchain-strategy-mcp

## License

Apache-2.0
