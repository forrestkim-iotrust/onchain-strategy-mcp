'use strict';

// Smoke test for lib/install.js using a locally-fabricated tarball + sha256,
// served via an injected fetcher (no network). Verifies:
//   - happy path: download, checksum, extract, chmod, version.json
//   - idempotent fast path: second call doesn't refetch
//   - checksum mismatch: throws
//
// Run: node test/install.smoke.js

const os = require('node:os');
const path = require('node:path');
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const crypto = require('node:crypto');
const tar = require('tar');
const assert = require('node:assert/strict');

// Redirect HOME so installDir() points into a tmpdir.
const tmpHome = fs.mkdtempSync(path.join(os.tmpdir(), 'osmcp-smoke-'));
process.env.HOME = tmpHome;
process.env.USERPROFILE = tmpHome;

const { ensureBinary, installDir, binaryPath, versionFilePath } = require('../lib/install');

async function buildTarball() {
  // Create a fake executor-mcp file and tar it.
  const stageDir = fs.mkdtempSync(path.join(os.tmpdir(), 'osmcp-stage-'));
  const binFile = path.join(stageDir, 'executor-mcp');
  fs.writeFileSync(binFile, '#!/bin/sh\necho fake-executor-mcp\n', { mode: 0o644 });
  const tarPath = path.join(stageDir, 'out.tar.gz');
  await tar.c({ gzip: true, cwd: stageDir, file: tarPath }, ['executor-mcp']);
  const buf = fs.readFileSync(tarPath);
  const sha = crypto.createHash('sha256').update(buf).digest('hex');
  return { buf, sha };
}

(async () => {
  assert.ok(installDir().startsWith(tmpHome), 'install dir should be in tmp HOME');

  const { buf, sha } = await buildTarball();

  let calls = 0;
  const fetchBuffer = async (url) => {
    calls += 1;
    if (url.endsWith('.sha256')) {
      return Buffer.from(`${sha}  executor-mcp.tar.gz\n`, 'utf8');
    }
    return buf;
  };

  // 1. Happy path.
  const bin = await ensureBinary('1.3.0', {
    fetchBuffer,
    target: 'aarch64-apple-darwin',
  });
  assert.equal(bin, binaryPath());
  assert.ok(fs.existsSync(bin), 'binary should exist after install');
  const mode = fs.statSync(bin).mode & 0o777;
  assert.equal(mode, 0o755, `binary should be 0755, got ${mode.toString(8)}`);
  const manifest = JSON.parse(fs.readFileSync(versionFilePath(), 'utf8'));
  assert.equal(manifest.version, '1.3.0');
  assert.equal(manifest.target, 'aarch64-apple-darwin');
  assert.equal(manifest.sha256, sha);
  assert.equal(calls, 2, 'should have fetched tarball + sha256');

  // 2. Idempotent fast path.
  calls = 0;
  await ensureBinary('1.3.0', { fetchBuffer, target: 'aarch64-apple-darwin' });
  assert.equal(calls, 0, 'cached install should not refetch');

  // 3. Checksum mismatch.
  await fsp.rm(versionFilePath(), { force: true });
  await fsp.rm(binaryPath(), { force: true });
  const badFetch = async (url) => {
    if (url.endsWith('.sha256')) {
      return Buffer.from(`${'0'.repeat(64)}  executor-mcp.tar.gz\n`, 'utf8');
    }
    return buf;
  };
  let threw = false;
  try {
    await ensureBinary('1.3.0', { fetchBuffer: badFetch, target: 'aarch64-apple-darwin' });
  } catch (e) {
    threw = true;
    assert.match(e.message, /Checksum mismatch/, 'should throw checksum mismatch');
  }
  assert.ok(threw, 'mismatched checksum should throw');

  // 4. Unsupported platform path through detectTarget.
  const { detectTarget } = require('../lib/platform');
  assert.throws(() => detectTarget('win32', 'x64'), /Unsupported platform/);

  console.log('OK — install.smoke passed');
})().catch((e) => {
  console.error('FAIL', e);
  process.exit(1);
});
