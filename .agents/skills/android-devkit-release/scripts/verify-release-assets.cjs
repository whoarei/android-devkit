#!/usr/bin/env node
// Read-only checks for local packages or assets downloaded from a GitHub Release.
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const assert = require("node:assert/strict");

function verify(directory, version, portableDirectory) {
  assert.match(
    version,
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/,
    "Expected a stable X.Y.Z version",
  );
  const root = path.resolve(directory);
  const installer = `AndroidDevKit-${version}-setup.exe`;
  const archive = `AndroidDevKit-${version}-windows-x64.zip`;
  const shared = [
    "THIRD_PARTY_NOTICES.txt",
    "third-party-manifest.json",
    "build-info.json",
  ];
  const hashedFiles = [installer, archive, ...shared].sort();
  const expected = [...hashedFiles, "SHA256SUMS.txt"].sort();
  assert.deepEqual(
    fs.readdirSync(root).sort(),
    expected,
    "Release directory must contain exactly the six assets for this version",
  );
  const read = (base, name) => {
    const file = path.join(base, name);
    const stat = fs.lstatSync(file);
    assert(
      stat.isFile() && !stat.isSymbolicLink() && stat.size > 0,
      `Expected a nonempty regular file: ${name}`,
    );
    return fs.readFileSync(file);
  };
  const digest = (buffer) =>
    crypto.createHash("sha256").update(buffer).digest("hex");
  const json = (buffer) =>
    JSON.parse(buffer.toString("utf8").replace(/^\uFEFF/, ""));
  const checksums = new Map();
  const lines = read(root, "SHA256SUMS.txt")
    .toString("utf8")
    .trim()
    .split(/\r?\n/);
  for (const line of lines) {
    const match = /^([a-f\d]{64}) {2}([^/\\\r\n]+)$/i.exec(line);
    assert(
      match,
      "Invalid SHA256SUMS entry (expected a hash and plain filename)",
    );
    const [, hash, name] = match;
    assert(hashedFiles.includes(name), `Unexpected checksum entry: ${name}`);
    assert(!checksums.has(name), `Duplicate checksum entry: ${name}`);
    checksums.set(name, hash.toLowerCase());
  }
  assert.deepEqual(
    [...checksums.keys()].sort(),
    hashedFiles,
    "Every release asset must have a checksum",
  );
  for (const [name, hash] of checksums) {
    assert.equal(digest(read(root, name)), hash, `SHA-256 mismatch: ${name}`);
  }
  const info = json(read(root, "build-info.json"));
  assert.equal(info.product, "Android DevKit", "Product");
  assert.equal(info.version, version, "Build version");
  assert.equal(info.profile, "release", "Build profile");
  assert.equal(info.platform, "windows-x86_64", "Build platform");
  assert.match(info.sha256, /^[a-f\d]{64}$/i, "Portable executable digest");
  assert(
    Number.isSafeInteger(info.sizeBytes) && info.sizeBytes > 0,
    "Portable executable size",
  );
  assert.equal(info.installer?.file, installer, "Installer filename");
  assert.equal(
    info.installer.sha256,
    checksums.get(installer),
    "Installer build metadata digest",
  );
  json(read(root, "third-party-manifest.json"));

  if (portableDirectory) {
    const portableRoot = path.resolve(portableDirectory);
    assert.deepEqual(
      fs.readdirSync(portableRoot).sort(),
      ["AndroidDevKit.exe", ...shared].sort(),
      "Extracted ZIP must contain the executable, notices, manifest and build info",
    );
    const executable = read(portableRoot, "AndroidDevKit.exe");
    assert.equal(executable.length, info.sizeBytes, "Portable executable size");
    assert.equal(
      digest(executable),
      info.sha256.toLowerCase(),
      "Portable executable SHA-256",
    );
    for (const name of shared) {
      assert.equal(
        digest(read(portableRoot, name)),
        checksums.get(name),
        `ZIP and release copy differ: ${name}`,
      );
    }
  }
  return {
    version,
    directory: root,
    assets: expected,
    checksumsVerified: true,
    portableVerified: Boolean(portableDirectory),
  };
}

if (require.main === module) {
  const args = process.argv.slice(2);
  if (args.length !== 2 && !(args.length === 4 && args[2] === "--portable")) {
    console.error(
      "Usage: node verify-release-assets.cjs <release-directory> <X.Y.Z> [--portable <extracted-zip-directory>]",
    );
    process.exitCode = 1;
  } else {
    try {
      console.log(JSON.stringify(verify(args[0], args[1], args[3]), null, 2));
    } catch (error) {
      console.error(`Release verification failed: ${error.message}`);
      process.exitCode = 1;
    }
  }
}

module.exports = { verify };
