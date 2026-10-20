const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { create } = require('./create-updater-manifest.cjs');
const {
  verify,
} = require('../.agents/skills/android-devkit-release/scripts/verify-release-assets.cjs');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'android-devkit-update-test-'));
const digest = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
let index = 0;
function fixture(signed = true) {
  const directory = path.join(root, String(++index));
  const portable = path.join(root, `portable-${index}`);
  fs.mkdirSync(directory);
  fs.mkdirSync(portable);
  const exe = Buffer.from('fixture executable');
  const installer = Buffer.from('fixture installer');
  const info = {
    product: 'Android DevKit',
    version: '1.2.3',
    profile: 'release',
    platform: 'windows-x86_64',
    sha256: digest(exe),
    sizeBytes: exe.length,
    installer: { file: 'AndroidDevKit-1.2.3-setup.exe', sha256: digest(installer) },
  };
  const write = (name, value) => fs.writeFileSync(path.join(directory, name), value);
  write(info.installer.file, installer);
  write('AndroidDevKit-1.2.3-windows-x64.zip', 'fixture archive');
  write('THIRD_PARTY_NOTICES.txt', 'fixture notices');
  write('third-party-manifest.json', '{}');
  write('build-info.json', JSON.stringify(info));
  fs.writeFileSync(path.join(portable, 'AndroidDevKit.exe'), exe);
  for (const name of ['THIRD_PARTY_NOTICES.txt', 'third-party-manifest.json', 'build-info.json'])
    fs.copyFileSync(path.join(directory, name), path.join(portable, name));
  if (signed) {
    write(
      `${info.installer.file}.sig`,
      Buffer.from('fixture detached signature').toString('base64') + '\n',
    );
    create(directory, '1.2.3', 'Fixture update notes', '2026-10-20T00:00:00Z');
  }
  const checksum = () =>
    write(
      'SHA256SUMS.txt',
      fs
        .readdirSync(directory)
        .filter((name) => name !== 'SHA256SUMS.txt')
        .map((name) => `${digest(fs.readFileSync(path.join(directory, name)))}  ${name}`)
        .join('\n') + '\n',
    );
  const editManifest = (edit) => {
    const manifest = JSON.parse(fs.readFileSync(path.join(directory, 'latest.json')));
    edit(manifest);
    write('latest.json', JSON.stringify(manifest));
    checksum();
  };
  checksum();
  return { directory, portable, write, checksum, editManifest };
}

test('signed release includes matching manifest, signature, checksums and portable contents', () => {
  const f = fixture();
  const report = verify(f.directory, '1.2.3', f.portable, true);
  assert.equal(report.assets.length, 8);
  assert.equal(report.updaterVerified, true);
  assert.equal(report.portableVerified, true);
});
test('legacy local packages remain verifiable, but signed releases require updater assets', () => {
  const f = fixture(false);
  assert.equal(verify(f.directory, '1.2.3').updaterVerified, false);
  assert.throws(() => verify(f.directory, '1.2.3', undefined, true), /exactly 8/);
  assert.throws(() => create(f.directory, '1.2.3'), /ENOENT/);
});
test('tampered installers and mismatched signatures fail', () => {
  const f = fixture();
  f.write('AndroidDevKit-1.2.3-setup.exe', 'tampered');
  assert.throws(() => verify(f.directory, '1.2.3', undefined, true), /SHA-256 mismatch/);
  const g = fixture();
  g.editManifest((manifest) => {
    manifest.platforms['windows-x86_64'].signature = 'wrong';
  });
  assert.throws(() => verify(g.directory, '1.2.3', undefined, true), /signature must match/);
});
test('wrong repository, version, platform or release date fails even with valid checksums', () => {
  for (const edit of [
    (m) => {
      m.platforms['windows-x86_64'].url = m.platforms['windows-x86_64'].url.replace(
        'android-devkit',
        'remote-runner',
      );
    },
    (m) => {
      m.version = '1.2.4';
    },
    (m) => {
      m.platforms = { 'linux-x86_64': m.platforms['windows-x86_64'] };
    },
    (m) => {
      m.pub_date = 'invalid';
    },
  ]) {
    const f = fixture();
    f.editManifest(edit);
    assert.throws(() => verify(f.directory, '1.2.3', undefined, true));
  }
});
test('missing assets, stale files and invalid signature encodings fail', () => {
  const f = fixture();
  fs.unlinkSync(path.join(f.directory, 'latest.json'));
  assert.throws(() => verify(f.directory, '1.2.3', undefined, true), /exactly 8/);
  const g = fixture();
  g.write('AndroidDevKit-1.2.2-setup.exe', 'old');
  assert.throws(() => verify(g.directory, '1.2.3', undefined, true), /exactly 8/);
  const h = fixture();
  h.write('AndroidDevKit-1.2.3-setup.exe.sig', 'invalid signature!');
  assert.throws(() => create(h.directory, '1.2.3'), /encoding/);
  assert.throws(() => create(h.directory, '1.2.3-rc.1'), /stable version/);
});
test('ZIP executable and shared metadata must match release copies', () => {
  const f = fixture();
  fs.writeFileSync(path.join(f.portable, 'AndroidDevKit.exe'), 'tampered program');
  assert.throws(() => verify(f.directory, '1.2.3', f.portable, true), /Portable executable/);
  const g = fixture();
  fs.appendFileSync(path.join(g.portable, 'build-info.json'), ' ');
  assert.throws(
    () => verify(g.directory, '1.2.3', g.portable, true),
    /ZIP and release copy differ/,
  );
});
after(() => {
  const parent = fs.realpathSync(os.tmpdir());
  const target = fs.realpathSync(root);
  const relative = path.relative(parent, target);
  assert(
    relative.startsWith('android-devkit-update-test-') &&
      !relative.includes(path.sep) &&
      !path.isAbsolute(relative),
    'Cleanup must stay inside this test directory',
  );
  fs.rmSync(target, { recursive: true, force: true });
});
