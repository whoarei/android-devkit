const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const assert = require('assert/strict');
const { execFileSync } = require('child_process');
const config = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const installerScript = fs.readFileSync('src-tauri/target/release/nsis/x64/installer.nsi', 'utf8');
const hookPath = path.resolve('src-tauri', config.bundle.windows.nsis.installerHooks);
assert(installerScript.includes(`!include "${hookPath}"`), 'Production installer includes hooks');
const installSection = installerScript.slice(installerScript.indexOf('Section Install'));
const waitHook = installSection.indexOf('!insertmacro NSIS_HOOK_PREINSTALL');
const runningCheck = installSection.indexOf('!insertmacro CheckIfAppIsRunning');
const copyExecutable = installSection.indexOf('File "${MAINBINARYSRCPATH}"');
assert(
  waitHook >= 0 && waitHook < runningCheck && runningCheck < copyExecutable,
  'Wait for updater exit, retain the running-app check, then copy the executable',
);
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
  updateExitWaitHookIncluded: true,
  signed: false,
  cleanMachineInstallUninstallVerified: false,
};
fs.mkdirSync('.qa/app', { recursive: true });
fs.writeFileSync('.qa/app/installer-report.json', JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
