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

After 'init' you can register the MCP with Claude Code:
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
  process.exit(result.status ?? 0);
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
    await runBinary(['init', ...args.slice(1)]);
    return;
  }

  if (first === 'serve') {
    // The Rust binary defaults to serve mode with no args; it reads
    // ./.local/config.toml relative to cwd.
    await runBinary([]);
    return;
  }

  // Unknown subcommand — show usage and exit non-zero.
  process.stderr.write(`Unknown command: ${first}\n\n`);
  process.stderr.write(USAGE);
  process.exit(2);
}

main(process.argv).catch((e) => {
  fail(`onchain-strategy-mcp: ${e.stack || e.message || e}`);
});
