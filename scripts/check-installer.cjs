const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const assert = require('assert/strict');
const { execFileSync } = require('child_process');
const metadata = JSON.parse(
  fs.readFileSync('artifacts/windows/build-info.json', 'utf8').replace(/^\uFEFF/, ''),
);
const directory = path.resolve('.qa/runtime/installer-inspection');
execFileSync(
  process.argv[2] ?? 'C:/Program Files/7-Zip/7z.exe',
  ['x', path.resolve('artifacts/windows', metadata.installer.file), `-o${directory}`, '-y'],
  { stdio: 'ignore' },
);
const hash = (b) => crypto.createHash('sha256').update(b).digest('hex');
const portable = fs.readFileSync('artifacts/windows/AndroidDevKit.exe');
const installed = fs.readFileSync(path.join(directory, 'android-devkit.exe'));
const marker = Buffer.from('__TAURI_BUNDLE_TYPE_VAR_NSS');
const index = installed.indexOf(marker);
assert(index >= 0, 'NSIS Tauri bundle marker');
const normalized = Buffer.from(installed);
Buffer.from('__TAURI_BUNDLE_TYPE_VAR_UNK').copy(normalized, index);
assert.equal(
  hash(normalized),
  hash(portable),
  'payload must match portable except Tauri bundle type marker',
);
for (const file of ['THIRD_PARTY_NOTICES.txt', 'third-party-manifest.json'])
  assert.equal(
    hash(fs.readFileSync(path.join(directory, file))),
    hash(fs.readFileSync(path.join('artifacts/windows', file))),
  );
const report = {
  installer: metadata.installer.file,
  installerSha256: metadata.installer.sha256,
  portableSha256: hash(portable),
  installerPayloadSha256: hash(installed),
  payloadMatchesAfterTauriBundleMarkerNormalization: true,
  differingBytes: 3,
  markerChange: 'UNK → NSS',
  noticeFilesMatch: true,
  signed: false,
  cleanMachineInstallUninstallVerified: false,
};
fs.mkdirSync('.qa/app', { recursive: true });
fs.writeFileSync('.qa/app/installer-report.json', JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
