#!/usr/bin/env node
'use strict';

const path = require('node:path');
const { spawnSync } = require('node:child_process');

const pkg = require('../package.json');
const { ensureBinary } = require('../lib/install');
const { detectTarget } = require('../lib/platform');

const USAGE = `onchain-strategy-mcp v${pkg.version}

Usage:
  npx onchain-strategy-mcp init [...args]   Set up a fresh burner wallet and config
                                            in the current directory.
  npx onchain-strategy-mcp serve            Run the MCP server (used by Claude Code).
  npx onchain-strategy-mcp --version        Print the package version.
  npx onchain-strategy-mcp --help           Show this help.

Aliases:
  osmcp <command>                           Short alias for onchain-strategy-mcp.

'init' auto-registers the MCP with Claude Code when 'claude' is on PATH.
If you need to register manually later:
  claude mcp add osmcp -- npx onchain-strategy-mcp serve

Docs: https://github.com/forrestkim-iotrust/onchain-strategy-mcp
`;

function printUsage() {
  process.stdout.write(USAGE);
}

function fail(msg, code = 1) {
  process.stderr.write(`${msg}\n`);
  process.exit(code);
}

async function runBinary(args) {
  let bin;
  try {
    bin = await ensureBinary(pkg.version);
  } catch (e) {
    fail(`onchain-strategy-mcp: ${e.message}`);
    return;
  }
  const result = spawnSync(bin, args, { stdio: 'inherit' });
  if (result.error) {
    fail(`Failed to execute ${bin}: ${result.error.message}`);
  }
  return result.status ?? 0;
}

// One-shot Claude Code MCP registration: idempotent, silent on success.
// Bails quietly if `claude` isn't on PATH (e.g. user installs in a
// non-Claude-Code environment); returns true if registration ran.
function tryRegisterWithClaudeCode() {
  const probe = spawnSync('claude', ['--version'], { stdio: 'ignore' });
  if (probe.error || probe.status !== 0) {
    process.stdout.write(
      "\n(Skipping `claude mcp add`: the `claude` CLI is not on PATH.\n" +
      " Install Claude Code, then run:\n" +
      "   claude mcp add osmcp -- npx onchain-strategy-mcp serve\n" +
      " to register the MCP server.)\n"
    );
    return false;
  }
  process.stdout.write("\n→ Registering osmcp with Claude Code...\n");
  const add = spawnSync(
    'claude',
    ['mcp', 'add', 'osmcp', '--', 'npx', '-y', 'onchain-strategy-mcp', 'serve'],
    { stdio: 'inherit' }
  );
  if (add.status === 0) {
    process.stdout.write("✓ osmcp registered with Claude Code. Run `claude` to start.\n");
    return true;
  }
  process.stderr.write(
    "(`claude mcp add` exited non-zero — it may already be registered. " +
    "Verify with `claude mcp list`.)\n"
  );
  return false;
}

async function main(argv) {
  const args = argv.slice(2);

  if (args.length === 0) {
    printUsage();
    process.exit(0);
  }

  const first = args[0];

  if (first === '--version' || first === '-v') {
    process.stdout.write(`${pkg.version}\n`);
    return;
  }
  if (first === '--help' || first === '-h' || first === 'help') {
    printUsage();
    return;
  }

  if (first === 'init') {
    // The Rust binary's `init` already prompts the user and runs
    // `claude mcp add osmcp -- npx onchain-strategy-mcp serve` when
    // they accept. Don't double-register from this wrapper.
    const code = await runBinary(['init', ...args.slice(1)]);
    process.exit(code);
  }

  if (first === 'serve') {
    // The Rust binary defaults to serve mode with no args; it reads
    // ./.local/config.toml relative to cwd.
    const code = await runBinary([]);
    process.exit(code);
  }

  // Unknown subcommand — show usage and exit non-zero.
  process.stderr.write(`Unknown command: ${first}\n\n`);
  process.stderr.write(USAGE);
  process.exit(2);
}

main(process.argv).catch((e) => {
  fail(`onchain-strategy-mcp: ${e.stack || e.message || e}`);
});
