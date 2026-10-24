const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const run = path.join(root, '.qa', 'faults', `run-${Date.now()}`);
const cases = path.join(run, 'cases');
fs.mkdirSync(cases, { recursive: true });
const log = fs.createWriteStream(path.join(run, 'cargo.log'));
const startedAt = new Date().toISOString();
const child = spawn('cargo', [
  'test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', 'fault_', '--', '--nocapture',
], {
  cwd: root,
  env: { ...process.env, DEVKIT_FAULT_REPORT_DIR: cases },
  windowsHide: true,
  stdio: ['ignore', 'pipe', 'pipe'],
});
for (const [stream, output] of [[child.stdout, process.stdout], [child.stderr, process.stderr]]) {
  stream.on('data', chunk => { output.write(chunk); log.write(chunk); });
}
child.on('error', error => {
  console.error(error);
  log.end();
  process.exitCode = 1;
});
child.on('close', code => {
  const results = fs.readdirSync(cases).filter(name => name.endsWith('.json'))
    .sort().map(name => JSON.parse(fs.readFileSync(path.join(cases, name), 'utf8')));
  const expected = process.platform === 'win32' ? 35 : 34;
  const killed = results.filter(item => item.details.termination === 'OS child kill').length;
  const passed = code === 0 && results.length === expected && killed === 14
    && results.every(item => item.passed);
  const report = {
    startedAt, completedAt: new Date().toISOString(), platform: process.platform,
    passed, cargoExitCode: code, expectedCases: expected, passedCases: results.length,
    osKilledChildren: killed, cases: results,
    scope: {
      sdk: 'Real bootstrap archive commit; multi-component reconciliation uses isolated installed metadata.',
      apk: 'Real install-response validation and task recovery, using a sandbox transport response; no physical device.',
      io: 'Native disk-full/permission errors injected at test-only boundaries, real SQLite pager-full/read-only failures and Windows handle lock.',
      interruption: 'OS process termination, not sudden power loss or physical filesystem corruption.',
    },
    cargoLog: path.join(run, 'cargo.log'),
  };
  fs.writeFileSync(path.join(run, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  fs.writeFileSync(path.join(root, '.qa', 'faults', 'latest.json'), JSON.stringify({ report: path.join(run, 'report.json') }, null, 2) + '\n');
  log.end();
  console.log(`Fault acceptance: ${passed ? 'PASSED' : 'FAILED'}; ${results.length}/${expected} cases; ${killed} OS-killed children`);
  console.log(`Evidence: ${path.join(run, 'report.json')}`);
  process.exitCode = passed ? 0 : 1;
});
