const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');
(async () => {
  const b = await chromium.connectOverCDP('http://127.0.0.1:9224');
  try {
    const p = b.contexts()[0].pages()[0];
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    // Explicitly exercise the exit dialog; fresh profiles now close to the tray by default.
    await p.evaluate(async () => {
      const settings = (await window.__TAURI_INTERNALS__.invoke('workspace_snapshot')).settings;
      await window.__TAURI_INTERNALS__.invoke('settings_save', {
        settings: { ...settings, trayEnabled: false },
      });
    });
    await p.locator('nav').getByRole('button', { name: '环境诊断', exact: true }).click();
    await p.getByRole('button', { name: '预览诊断导出', exact: true }).click();
    const preview = p.getByRole('dialog', { name: '导出内容预览', exact: true });
    await preview.waitFor();
    await preview.getByText(/已默认脱敏/).waitFor();
    await p.screenshot({ path: '.qa/app/m4-export.png' });
    await preview.getByRole('button', { name: '关闭面板', exact: true }).click();
    const blocked = await p.evaluate(async () => {
      try {
        await window.__TAURI_INTERNALS__.invoke('close_resolve', { mode: 'exit' });
        return 'unexpected';
      } catch (e) {
        return e.code;
      }
    });
    assert.equal(blocked, 'UNSAVED_ARTIFACTS');
    await p.evaluate(() => window.__TAURI_INTERNALS__.invoke('window_action', { action: 'close' }));
    const closing = p.getByRole('dialog', { name: '退出工作台', exact: true });
    await closing.waitFor();
    await closing.getByText(/尚未另存/).waitFor();
    await p.screenshot({ path: '.qa/app/m4-close.png' });
    await closing.getByRole('button', { name: '返回', exact: true }).click();
    await p.locator('nav').getByRole('button', { name: '设置', exact: true }).click();
    const tray = p.getByRole('checkbox', { name: /关闭窗口时隐藏到托盘/ });
    await tray.check();
    await p.getByRole('button', { name: '保存设置', exact: true }).click();
    await p.getByRole('status').filter({ hasText: '设置已保存' }).waitFor();
    await p.waitForFunction(
      async () =>
        (await window.__TAURI_INTERNALS__.invoke('workspace_snapshot')).settings.trayEnabled,
    );
    await p.evaluate(() => window.__TAURI_INTERNALS__.invoke('window_action', { action: 'close' }));
    await p.waitForTimeout(300);
    assert.equal(
      (await p.evaluate(() => window.__TAURI_INTERNALS__.invoke('window_state'))).visible,
      false,
    );
    // Starting the same executable exercises the existing single-instance restore path.
    const child = spawn(path.resolve('.qa/app/AndroidDevKit-preview.exe'), [], {
      windowsHide: true,
      stdio: 'ignore',
    });
    await new Promise((resolve, reject) => {
      child.on('exit', resolve);
      child.on('error', reject);
    });
    await p.waitForTimeout(500);
    assert.equal(
      (await p.evaluate(() => window.__TAURI_INTERNALS__.invoke('window_state'))).visible,
      true,
    );
    await tray.uncheck();
    await p.getByRole('button', { name: '保存设置', exact: true }).click();
    await p.locator('nav').getByRole('button', { name: '任务中心', exact: true }).click();
    await p.getByRole('heading', { name: /未另存产物/ }).waitFor();
    const artifacts = await p.evaluate(() => window.__TAURI_INTERNALS__.invoke('artifacts_list'));
    assert(artifacts.some((a) => a.kind === 'export' && !a.acknowledged));
    assert.deepEqual(errors, []);
    const result = {
      diagnosticPreview: true,
      defaultRedaction: true,
      unsavedExitBlocked: true,
      closeDialogListsArtifacts: true,
      closeToTrayEnabledAndDisabled: true,
      singleInstanceRestored: true,
      artifactRegistryPersisted: true,
      errors,
    };
    fs.writeFileSync('.qa/app/m4-report.json', JSON.stringify(result, null, 2));
    console.log(JSON.stringify(result));
  } finally {
    await b.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
