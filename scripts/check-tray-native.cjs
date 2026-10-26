const { chromium } = require('playwright');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
// Opt-in native check for an isolated QA executable/profile. The metadata contains
// pid, debugPort, executable and dataDirectory, all created by the QA launcher.
(async () => {
  assert(process.argv[2], 'Pass the isolated QA native-process.json path');
  const metadataPath = path.resolve(process.argv[2]);
  const qaRoot = path.resolve(__dirname, '../.qa') + path.sep;
  assert(metadataPath.startsWith(qaRoot), 'QA metadata must be inside .qa');
  const runRoot = path.dirname(metadataPath);
  const metadata = JSON.parse(fs.readFileSync(metadataPath, 'utf8').replace(/^\uFEFF/, ''));
  assert.equal(path.dirname(metadata.executable), runRoot);
  assert.equal(path.dirname(metadata.dataDirectory), runRoot);
  assert(Number.isInteger(metadata.pid) && metadata.pid > 0);
  assert(
    Number.isInteger(metadata.debugPort) && metadata.debugPort > 0 && metadata.debugPort < 65536,
  );
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${metadata.debugPort}`);
  const page = browser.contexts()[0].pages()[0];
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  const invoke = (command, args) =>
    page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), {
      command,
      args,
    });
  const waitState = (expected) =>
    page.waitForFunction(
      async (expected) => {
        const state = await window.__TAURI_INTERNALS__.invoke('window_state');
        return Object.entries(expected).every(([key, value]) => state[key] === value);
      },
      expected,
      { polling: 100, timeout: 10000 },
    );
  const reopen = async () => {
    await new Promise((resolve, reject) => {
      const child = spawn(metadata.executable, [], { windowsHide: true, stdio: 'ignore' });
      const timeout = setTimeout(() => reject(Error('Second instance did not exit')), 10000);
      child.on('error', reject);
      child.on('exit', (code) => {
        clearTimeout(timeout);
        code === 0 ? resolve() : reject(Error(`Second instance exit ${code}`));
      });
    });
    await waitState({ visible: true, minimized: false, focused: true });
    process.kill(metadata.pid, 0);
  };
  try {
    const snapshot = await invoke('workspace_snapshot');
    assert.equal(snapshot.settings.trayEnabled, undefined);
    assert.equal((await invoke('close_status')).trayEnabled, true);
    assert.equal(
      await page.getByRole('button', { name: '关闭窗口', exact: true }).getAttribute('title'),
      '关闭到托盘',
    );
    // Both entry points share native minimize -> resize -> hide, rather than bypassing it.
    await page.getByRole('button', { name: '最小化', exact: true }).click();
    await waitState({ visible: false, minimized: true });
    await reopen();
    await invoke('window_action', { action: 'maximize' });
    await waitState({ maximized: true });
    await invoke('window_action', { action: 'minimize' });
    await waitState({ visible: false, minimized: true });
    await reopen();
    await waitState({ maximized: true });
    await page.getByRole('button', { name: '关闭窗口', exact: true }).click();
    await waitState({ visible: false });
    await reopen();
    await waitState({ maximized: true });
    // Relaunching an already visible maximized window must preserve maximization.
    await reopen();
    await waitState({ maximized: true });
    await invoke('window_action', { action: 'maximize' });
    await waitState({ maximized: false });
    await invoke('window_action', { action: 'close' });
    await waitState({ visible: false });
    await reopen();
    await invoke('settings_save', { settings: { ...snapshot.settings, trayEnabled: false } });
    assert.equal((await invoke('close_status')).trayEnabled, true);
    assert.equal((await invoke('workspace_snapshot')).settings.trayEnabled, undefined);
    await invoke('window_action', { action: 'close' });
    await waitState({ visible: false });
    await reopen();
    await invoke('window_action', { action: 'minimize' });
    await waitState({ visible: false, minimized: true });
    await reopen();
    const exported = await invoke('export_prepare', { kind: 'diagnostics', redacted: true });
    await invoke('window_action', { action: 'close' });
    await waitState({ visible: false });
    await reopen();
    assert((await invoke('close_status')).unsaved.some((a) => a.id === exported.artifact.id));
    const blocked = await page.evaluate(() =>
      window.__TAURI_INTERNALS__.invoke('close_resolve', { mode: 'exit' }).then(
        () => '',
        (e) => e.code,
      ),
    );
    assert.equal(blocked, 'UNSAVED_ARTIFACTS');
    // Unsaved artifacts stay intact while the app continues in the tray.
    await invoke('close_resolve', { mode: 'hide' });
    await waitState({ visible: false });
    await reopen();
    await invoke('close_resolve', { mode: 'return' });
    await invoke('artifacts_discard', { artifactIds: [exported.artifact.id] });
    await invoke('settings_save', { settings: snapshot.settings });
    assert.deepEqual(errors, []);
    const report = {
      nativeMinimizeToTray: true,
      singleInstanceRestoreAndFocus: true,
      maximizedStatePreserved: true,
      closeToTray: true,
      legacyCloseOptOutIgnored: true,
      closePreservesUnsavedArtifacts: true,
      unsavedExitGuard: true,
      explicitHideToTray: true,
      errors,
    };
    fs.writeFileSync(path.join(runRoot, 'tray-report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report));
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
