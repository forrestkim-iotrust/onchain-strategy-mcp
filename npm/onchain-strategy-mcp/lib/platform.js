'use strict';

// OS/arch → Rust target triple mapping for v1.3 supported platforms.
// Contract: DESIGN.md v1.3 naming contract.

const TARGETS = {
  'darwin/arm64': 'aarch64-apple-darwin',
  'darwin/x64': 'x86_64-apple-darwin',
  'linux/x64': 'x86_64-unknown-linux-gnu',
};

function detectTarget(platform = process.platform, arch = process.arch) {
  const key = `${platform}/${arch}`;
  const target = TARGETS[key];
  if (!target) {
    const err = new Error(
      `Unsupported platform: ${platform}/${arch}.\n` +
        `\n` +
        `onchain-strategy-mcp v1.3 ships prebuilt binaries for:\n` +
        `  - macOS arm64  (aarch64-apple-darwin)\n` +
        `  - macOS x64    (x86_64-apple-darwin)\n` +
        `  - Linux x64    (x86_64-unknown-linux-gnu)\n` +
        `\n` +
        `Windows is not yet supported. To build from source, see:\n` +
        `  https://github.com/forrestkim-iotrust/onchain-strategy-mcp#build-from-source`
    );
    err.code = 'EUNSUPPORTEDPLATFORM';
    throw err;
  }
  return target;
}

module.exports = { detectTarget, TARGETS };
