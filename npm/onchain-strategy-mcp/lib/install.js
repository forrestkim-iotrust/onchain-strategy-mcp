'use strict';

const os = require('node:os');
const path = require('node:path');
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const crypto = require('node:crypto');
const { pipeline } = require('node:stream/promises');
const { Readable } = require('node:stream');
const tar = require('tar');

const { detectTarget } = require('./platform');

const REPO_RELEASE_BASE =
  'https://github.com/forrestkim-iotrust/onchain-strategy-mcp/releases/download';

function installDir() {
  return path.join(os.homedir(), '.osmcp', 'bin');
}

function binaryPath() {
  return path.join(installDir(), 'executor-mcp');
}

function versionFilePath() {
  return path.join(installDir(), 'version.json');
}

async function readCachedVersion() {
  try {
    const raw = await fsp.readFile(versionFilePath(), 'utf8');
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

async function fileExists(p) {
  try {
    await fsp.access(p, fs.constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

async function fetchBuffer(url) {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) {
    throw new Error(
      `Download failed: ${res.status} ${res.statusText}\n  URL: ${url}`
    );
  }
  const ab = await res.arrayBuffer();
  return Buffer.from(ab);
}

function sha256(buf) {
  return crypto.createHash('sha256').update(buf).digest('hex');
}

function parseSha256File(text) {
  // Either bare hex, or `<hex>  filename` (shasum / sha256sum format).
  const m = String(text).trim().match(/^([0-9a-fA-F]{64})/);
  if (!m) {
    throw new Error(`Could not parse sha256 checksum file:\n${text}`);
  }
  return m[1].toLowerCase();
}

/**
 * Ensure the executor-mcp binary exists locally for the given version.
 * Returns the absolute binary path.
 *
 * Options for testing:
 *   - opts.fetchBuffer:   override the network fetcher (url) => Promise<Buffer>
 *   - opts.releaseBase:   override the release URL base
 *   - opts.target:        force a target triple (skip detection)
 */
async function ensureBinary(version, opts = {}) {
  if (!version || typeof version !== 'string') {
    throw new Error('ensureBinary: version is required');
  }
  const target = opts.target || detectTarget();
  const fetcher = opts.fetchBuffer || fetchBuffer;
  const releaseBase = opts.releaseBase || REPO_RELEASE_BASE;

  const dir = installDir();
  const bin = binaryPath();
  const vfile = versionFilePath();

  // Fast path: cached & matching.
  const cached = await readCachedVersion();
  if (
    cached &&
    cached.version === version &&
    cached.target === target &&
    (await fileExists(bin))
  ) {
    return bin;
  }

  await fsp.mkdir(dir, { recursive: true });

  const assetName = `executor-mcp-${target}.tar.gz`;
  const tarUrl = `${releaseBase}/v${version}/${assetName}`;
  const shaUrl = `${tarUrl}.sha256`;

  let tarBuf;
  let shaText;
  try {
    [tarBuf, shaText] = await Promise.all([
      fetcher(tarUrl),
      fetcher(shaUrl).then((b) => b.toString('utf8')),
    ]);
  } catch (e) {
    throw new Error(
      `Failed to download executor-mcp ${version} for ${target}.\n` +
        `  ${e.message}\n` +
        `\n` +
        `Check your network, or see release page:\n` +
        `  https://github.com/forrestkim-iotrust/onchain-strategy-mcp/releases/tag/v${version}`
    );
  }

  const expected = parseSha256File(shaText);
  const actual = sha256(tarBuf);
  if (expected !== actual) {
    throw new Error(
      `Checksum mismatch for ${assetName}.\n` +
        `  expected: ${expected}\n` +
        `  actual:   ${actual}\n` +
        `Refusing to install a tampered or corrupt binary.`
    );
  }

  // Extract into install dir. The tarball contains a single `executor-mcp` file
  // at its root (per Stream C contract).
  await pipeline(
    Readable.from(tarBuf),
    tar.x({ cwd: dir, strip: 0 })
  );

  if (!(await fileExists(bin))) {
    throw new Error(
      `Extraction succeeded but ${bin} is missing.\n` +
        `The release tarball does not contain an 'executor-mcp' entry at its root.`
    );
  }

  await fsp.chmod(bin, 0o755);

  const manifest = { version, sha256: actual, target };
  await fsp.writeFile(vfile, JSON.stringify(manifest, null, 2) + '\n', 'utf8');

  return bin;
}

module.exports = {
  ensureBinary,
  installDir,
  binaryPath,
  versionFilePath,
  // exported for tests
  _internal: { parseSha256File, sha256, fetchBuffer },
};
