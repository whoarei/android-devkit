// Exercise the production NSIS hook against real Windows executable mappings.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawn, execFileSync } = require('node:child_process');
const { setTimeout: delay } = require('node:timers/promises');

const root = path.resolve(__dirname, '..');
const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.conf.json')));
const hooks = path.resolve(root, 'src-tauri', config.bundle.windows.nsis.installerHooks);
const makensis = process.argv[2] || path.join(process.env.LOCALAPPDATA, 'tauri/NSIS/makensis.exe');
const outputRoot = path.join(root, '.qa/runtime');
fs.mkdirSync(outputRoot, { recursive: true });
const directory = fs.mkdtempSync(path.join(outputRoot, 'installer-update-'));
const installed = path.join(directory, '升级 退出');
const other = path.join(directory, 'portable');
for (const location of [installed, other]) fs.mkdirSync(location);
const executable = path.join(installed, 'fixture.exe');
const source = path.join(directory, 'fixture.rs');
fs.writeFileSync(
  source,
  `use std::{env, fs, path::Path, thread, time::Duration};
fn main() {
    let args: Vec<String> = env::args().collect();
    fs::write(&args[1], b"ready").unwrap();
    while !Path::new(&args[2]).exists() { thread::sleep(Duration::from_millis(10)); }
}
`,
);
execFileSync('rustc', ['--edition=2021', source, '-o', executable], { cwd: root });
fs.copyFileSync(executable, path.join(other, 'fixture.exe'));
const harness = path.join(directory, 'harness.exe');
const script = path.join(directory, 'harness.nsi');
fs.writeFileSync(
  script,
  '\uFEFF' +
    `Unicode true
RequestExecutionLevel user
SilentInstall silent
OutFile "${harness}"
InstallDir "${installed}"
!include LogicLib.nsh
!include FileFunc.nsh
!include "Win\\RestartManager.nsh"
!define ERROR_MORE_DATA 234
!define MAINBINARYNAME "fixture"
!include "${hooks}"
Var UpdateMode
Function .onInit
  ClearErrors
  \${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  \${IfNot} \${Errors}
    StrCpy $UpdateMode 1
  \${EndIf}
FunctionEnd
Section
  FileOpen $0 "$INSTDIR\\entered" w
  FileClose $0
  StrCpy $0 101
  StrCpy $1 102
  StrCpy $2 103
  StrCpy $3 104
  StrCpy $4 105
  StrCpy $R0 106
  StrCpy $5 107
  Push "stack-sentinel"
  !insertmacro NSIS_HOOK_PREINSTALL
  Pop $6
  \${If} $0 != 101
  \${OrIf} $1 != 102
  \${OrIf} $2 != 103
  \${OrIf} $3 != 104
  \${OrIf} $4 != 105
  \${OrIf} $R0 != 106
  \${OrIf} $5 != 107
  \${OrIf} $6 != "stack-sentinel"
    SetErrorLevel 1
    Abort
  \${EndIf}
  FileOpen $0 "$INSTDIR\\completed" w
  FileWrite $0 "success"
  FileClose $0
SectionEnd
`,
);
execFileSync(makensis, ['/V2', script], { cwd: root, stdio: 'pipe' });

function launch(file, args = []) {
  const child = spawn(file, args, { windowsHide: true, stdio: 'ignore', timeout: 45000 });
  const done = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      child.finished = true;
      if (code === 0) resolve();
      else reject(new Error(`${path.basename(file)} exited ${code} (${signal})`));
    });
  });
  return { child, done };
}

async function waitFor(file, process) {
  const deadline = Date.now() + 10000;
  while (!fs.existsSync(file)) {
    assert(!process.child.finished, `${path.basename(file)} must be written before exit`);
    assert(Date.now() < deadline, `Timed out waiting for ${file}`);
    await delay(25);
  }
}

async function scenario(name, { holderRoot, update = true, releaseAfter, bounded = false } = {}) {
  const entered = path.join(installed, 'entered');
  const completed = path.join(installed, 'completed');
  for (const file of [entered, completed]) fs.rmSync(file, { force: true });
  const ready = path.join(directory, `${name}.ready`);
  const release = path.join(directory, `${name}.release`);
  let holder;
  try {
    if (holderRoot) {
      holder = launch(path.join(holderRoot, 'fixture.exe'), [ready, release]);
      await waitFor(ready, holder);
    }
    const start = Date.now();
    const installer = launch(harness, update ? ['/S', '/UPDATE'] : ['/S']);
    await waitFor(entered, installer);
    if (releaseAfter) {
      await delay(releaseAfter);
      assert(!fs.existsSync(completed), 'Hook must wait while the installed exe is in use');
      assert(!holder.child.finished, 'The old process must exit on its own');
      fs.writeFileSync(release, 'exit');
      await holder.done;
    }
    await installer.done;
    const elapsedMs = Date.now() - start;
    assert.equal(fs.readFileSync(completed, 'utf8'), 'success');
    if (bounded) {
      assert(elapsedMs >= 20000, 'A persistent holder must receive the full exit grace period');
      assert(elapsedMs < 35000, 'Waiting must be bounded');
      assert(!holder.child.finished, 'Hook must leave shutdown to the normal installer check');
    } else {
      assert(elapsedMs < 15000, `${name}: no unnecessary full timeout (${elapsedMs} ms)`);
    }
    const result = { name, elapsedMs, passed: true };
    console.log(JSON.stringify(result));
    return result;
  } finally {
    if (holder) {
      fs.writeFileSync(release, 'exit');
      await holder.done;
    }
  }
}

(async () => {
  const results = [];
  results.push(await scenario('no-running-app'));
  results.push(await scenario('old-app-exits', { holderRoot: installed, releaseAfter: 1000 }));
  results.push(await scenario('other-copy-running', { holderRoot: other }));
  results.push(await scenario('manual-install', { holderRoot: installed, update: false }));
  results.push(await scenario('bounded-wait', { holderRoot: installed, bounded: true }));
  const report = { productionHook: path.relative(root, hooks), results };
  fs.mkdirSync(path.join(root, '.qa/app'), { recursive: true });
  fs.writeFileSync(
    path.join(root, '.qa/app/installer-update-report.json'),
    JSON.stringify(report, null, 2) + '\n',
  );
  console.log(JSON.stringify(report));
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
