// Opt-in check against a separately launched QA application, never a user's instance.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
(async () => {
  const metadata = JSON.parse(
    fs.readFileSync('.qa/p0/native-process.json', 'utf8').replace(/^\uFEFF/, ''),
  );
  assert(path.resolve(metadata.dataDirectory).startsWith(path.resolve('.qa/p0') + path.sep));
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${metadata.debugPort}`);
  try {
    const page = browser
      .contexts()
      .flatMap((context) => context.pages())
      .find((page) => /tauri|localhost/.test(page.url()));
    assert(page, 'QA WebView');
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    const invoke = (command, args) =>
      page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), {
        command,
        args,
      });
    const initial = await invoke('workspace_snapshot');
    assert.equal(initial.setupRequired, true, 'Only a fresh isolated profile is supported');
    assert.deepEqual(initial.tasks, []);
    assert(initial.settings.sdkRoot.toLowerCase().includes('\\appdata\\local\\android\\sdk'));
    const setup = page.getByRole('dialog', { name: '选择或创建 SDK 环境', exact: true });
    await setup.waitFor();
    const candidates = await invoke('environment_scan');
    const existing = candidates.find(
      (candidate) => candidate.adb && candidate.path.toLowerCase() === 'e:\\android\\sdk',
    );
    assert(existing, 'Existing SDK is detected without changing the selected path');
    assert(existing.freeBytes > 0);
    await setup.getByText(existing.path, { exact: true }).click();
    await setup.getByRole('button', { name: '应用所选环境', exact: true }).click();
    await setup.waitFor({ state: 'detached' });
    const snapshot = await invoke('workspace_snapshot');
    assert.equal(snapshot.setupRequired, false);
    assert.equal(snapshot.settings.sdkRoot, existing.path);
    assert.deepEqual(snapshot.tasks, [], 'SDK reuse starts no installation tasks');
    const summary = await invoke('environment_summary', { redacted: true });
    for (const device of snapshot.devices)
      if (device.serial.length >= 4) assert(!summary.includes(device.serial));
    assert(summary.includes('Android DevKit'));
    assert.equal(
      (await invoke('artifacts_list')).length,
      0,
      'Summary preparation creates no files',
    );
    let usage;
    if (snapshot.avds.length) {
      usage = await invoke('avd_usage', { avdId: snapshot.avds[0].id });
      assert(usage.bytes >= 0 && usage.files >= 0);
      assert.equal(typeof usage.complete, 'boolean');
    }
    const rejection = (command, args) =>
      page.evaluate(
        ({ command, args }) =>
          window.__TAURI_INTERNALS__.invoke(command, args).then(
            () => '',
            (error) => error.code,
          ),
        { command, args },
      );
    const rejectedUsage = await rejection('avd_usage', { avdId: 'C:\\not-an-avd' });
    assert.equal(rejectedUsage, 'AVD_NOT_FOUND');
    const clipboardLimit = await rejection('text_copy', { text: 'x'.repeat(256 * 1024 + 1) });
    assert.equal(
      clipboardLimit,
      'CLIPBOARD_LIMIT',
      'Invalid requests are rejected before touching the clipboard',
    );
    const before = await invoke('window_state');
    await invoke('window_action', { action: 'maximize' });
    assert.notEqual((await invoke('window_state')).maximized, before.maximized);
    await invoke('window_action', { action: 'maximize' });
    assert.equal((await invoke('window_state')).maximized, before.maximized);
    await page.getByRole('button', { name: '环境诊断', exact: true }).click();
    await page.getByRole('button', { name: '预览并复制环境摘要', exact: true }).click();
    const preview = page.getByRole('dialog', { name: '环境摘要预览', exact: true });
    await preview.waitFor();
    assert((await preview.locator('pre').textContent()).includes('Android DevKit'));
    await page.screenshot({ path: '.qa/p0/native-summary.png' });
    await preview.getByRole('button', { name: '关闭面板', exact: true }).click();
    assert.deepEqual(errors, []);
    fs.writeFileSync(
      '.qa/p0/native-report.json',
      JSON.stringify(
        {
          isolatedProfile: true,
          defaultUserDirectory: true,
          actualSdkDiscoveryAndReuse: true,
          noInstallOnReuse: true,
          summaryRedacted: true,
          noUnsavedCopies: true,
          usage,
          unknownAvdRejected: true,
          clipboardLimitRejected: true,
          windowStateRestored: true,
          dpiScale: await page.evaluate(() => devicePixelRatio),
          errors,
        },
        null,
        2,
      ),
    );
    console.log(
      'Native P0: isolated first run, real SDK discovery/reuse, redacted summary, actual usage, IPC limits and restored window state passed.',
    );
    await invoke('close_resolve', {
      mode: 'exit',
      discard: false,
      stopRecordings: false,
      discardIds: [],
    }).catch(() => {});
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
