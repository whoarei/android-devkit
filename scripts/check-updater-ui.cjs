const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const currentVersion = require('../package.json').version;
const [major, minor, patch] = currentVersion.split('.').map(Number);
const latestVersion = `${major}.${minor}.${patch + 1}`;

(async () => {
  const browser = await chromium.launch({
    channel: process.env.PLAYWRIGHT_CHANNEL || 'msedge',
    headless: true,
  });
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    await require('./preferences-fixture.cjs')(page);
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.addInitScript(
      ({ currentVersion, latestVersion }) => {
        window.isTauri = true;
        window.__checks = 0;
        window.__mode = 'error';
        window.__installed = false;
        window.__blocker = '';
        window.__downloads = 0;
        window.__installs = [];
        window.__callbacks = {};
        window.__listeners = {};
        let callback = 0;
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
        window.__TAURI_INTERNALS__ = {
          metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
          transformCallback(fn) {
            window.__callbacks[++callback] = fn;
            return callback;
          },
          unregisterCallback() {},
          invoke: async (command, args) => {
            if (command === 'workspace_snapshot')
              return {
                settings: {
                  sdkRoot: 'E:\\SDK',
                  avdRoots: ['E:\\AVD'],
                  javaPath: '',
                  proxy: '',
                  bootTimeoutSecs: 180,
                  trayEnabled: false,
                  sdkSites: [],
                  sdkChannel: 0,
                },
                environment: { adb: 'fixture', warnings: [] },
                devices: [],
                avds: [],
                packages: [],
                tasks: [],
                server: 'fixture',
                serverEpoch: 1,
                freeBytes: 1024 ** 3,
              };
            if (command === 'window_state') return { maximized: false };
            if (command === 'check_app_update') {
              window.__checks++;
              if (window.__mode === 'error')
                throw { message: '无法检查更新，请检查网络或稍后重试。' };
              if (window.__mode === 'current') return null;
              if (window.__mode === 'hold')
                return new Promise((resolve) => {
                  window.__resolveCheck = resolve;
                });
              return {
                updateId: `check-${window.__checks}`,
                currentVersion,
                latestVersion,
                notes: 'Fixture update notes',
                publishedAt: '2026-10-20T00:00:00Z',
                canAutoInstall: window.__installed,
              };
            }
            if (command === 'close_status')
              return {
                tasks: window.__blocker === 'task' ? [{}] : [],
                unsaved: window.__blocker === 'file' ? [{}] : [],
                draining: false,
                trayEnabled: false,
              };
            if (command === 'open_app_download') {
              window.__downloads++;
              return;
            }
            if (command === 'install_app_update') {
              window.__installs.push(args);
              return new Promise((_, reject) => {
                window.__rejectInstall = reject;
              });
            }
            if (command === 'plugin:event|listen') {
              window.__listeners[args.event] = args.handler;
              return args.handler;
            }
            if (command === 'plugin:event|unlisten') {
              delete window.__listeners[args.event];
              return;
            }
            return null;
          },
        };
      },
      { currentVersion, latestVersion },
    );
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    await page.waitForFunction(() => window.__checks === 1);
    await page.getByRole('heading', { name: '设备工作台', exact: true }).waitFor();
    assert.equal(await page.getByRole('alert').count(), 0, 'Startup check failures are quiet');
    await page.locator('nav').getByRole('button', { name: '设置', exact: true }).click();
    const section = page.locator('.software-update');
    await section.getByRole('button', { name: '检查更新', exact: true }).click();
    await section.getByRole('alert').filter({ hasText: '无法检查更新' }).waitFor();
    await page.evaluate(() => {
      window.__mode = 'hold';
    });
    await section.getByRole('button', { name: '检查更新', exact: true }).click();
    await page.waitForFunction(() => typeof window.__resolveCheck === 'function');
    assert.equal(
      await section.getByRole('button', { name: '正在检查更新…', exact: true }).isDisabled(),
      true,
    );
    await page.evaluate(() => {
      window.__resolveCheck(null);
      window.__mode = 'available';
    });
    await section.getByRole('status').filter({ hasText: '当前已是最新版本' }).waitFor();
    await section.getByRole('button', { name: '检查更新', exact: true }).click();
    await section.getByText(`发现新版本：${latestVersion}`, { exact: true }).waitFor();
    assert.equal(
      await section.getByRole('button', { name: '下载并安装更新', exact: true }).count(),
      0,
      'Portable builds do not install',
    );
    await section.getByRole('button', { name: '前往下载页面', exact: true }).click();
    assert.equal(await page.evaluate(() => window.__downloads), 1);
    await page.evaluate(() => {
      window.__installed = true;
      window.__blocker = 'task';
    });
    await section.getByRole('button', { name: '检查更新', exact: true }).click();
    await section.getByRole('button', { name: '下载并安装更新', exact: true }).click();
    await section.getByRole('alert').filter({ hasText: '请先完成或停止所有任务' }).waitFor();
    await page.evaluate(() => {
      window.__blocker = 'file';
    });
    await section.getByRole('button', { name: '下载并安装更新', exact: true }).click();
    await section.getByRole('alert').filter({ hasText: '请先保存或清理待保存文件' }).waitFor();
    await page.evaluate(() => {
      window.__blocker = '';
    });
    await section.getByRole('button', { name: '下载并安装更新', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '安装软件更新', exact: true });
    await dialog.waitFor();
    await dialog
      .getByText(`将从 ${currentVersion} 更新到 ${latestVersion}。`, { exact: true })
      .waitFor();
    assert.equal(
      await page.evaluate(() => window.__installs.length),
      0,
      'Installation requires explicit confirmation',
    );
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    await section.getByRole('button', { name: '下载并安装更新', exact: true }).click();
    await dialog.getByRole('button', { name: '确认更新并重启', exact: true }).click();
    await page.waitForFunction(() => typeof window.__rejectInstall === 'function');
    assert.equal(await dialog.getByRole('button', { name: '关闭面板', exact: true }).count(), 0);
    await page.keyboard.press('Escape');
    await dialog.waitFor();
    assert.equal(
      await page.evaluate(() => window.__installs[0].updateId === `check-${window.__checks}`),
      true,
    );
    const emit = async (payload) =>
      page.evaluate((payload) => {
        window.__callbacks[window.__listeners['app-update-progress']]({
          event: 'app-update-progress',
          id: 1,
          payload,
        });
      }, payload);
    await emit({ phase: 'downloading', downloaded: 2 * 1024 ** 2, total: 10 * 1024 ** 2 });
    await dialog.getByText(/20%/).waitFor();
    assert.equal(await dialog.getByRole('progressbar').getAttribute('max'), String(10 * 1024 ** 2));
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(
      await dialog.evaluate((element) => element.scrollWidth > element.clientWidth),
      false,
    );
    const output = path.resolve(__dirname, '../.qa/app');
    fs.mkdirSync(output, { recursive: true });
    await page.screenshot({ path: path.join(output, 'updater-progress-compact.png') });
    await emit({ phase: 'verifying', downloaded: 0, total: null });
    await dialog.getByText('正在校验更新签名…', { exact: true }).waitFor();
    await emit({ phase: 'installing', downloaded: 10, total: 10 });
    await dialog.getByText('正在安装更新，即将重新打开应用…', { exact: true }).waitFor();
    await page.evaluate(() =>
      window.__rejectInstall({ message: '更新下载或签名校验失败，请重试或前往下载页面。' }),
    );
    await section.getByRole('alert').filter({ hasText: '签名校验失败' }).waitFor();
    assert.equal(await page.getByRole('dialog').count(), 0);
    await page.waitForFunction(() => !window.__listeners['app-update-progress']);
    await page.setViewportSize({ width: 1280, height: 800 });
    await section.getByRole('button', { name: '检查更新', exact: true }).click();
    await section.getByText('Fixture update notes', { exact: true }).waitFor();
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await section.getByRole('button', { name: 'Check for updates', exact: true }).waitFor();
    await page.getByRole('button', { name: 'Switch to dark theme', exact: true }).click();
    await page.screenshot({ path: path.join(output, 'updater-settings-dark.png') });
    await section
      .getByRole('checkbox', { name: 'Check for updates on startup', exact: true })
      .uncheck();
    assert.equal(
      await page.evaluate(() => window.__preferences.autoCheckUpdates),
      false,
    );
    await page.reload();
    await page.locator('nav').getByRole('button', { name: 'Settings', exact: true }).click();
    await section.getByRole('button', { name: 'Check for updates', exact: true }).waitFor();
    assert.equal(
      await page.evaluate(() => window.__checks),
      0,
      'Disabled startup preference persists',
    );
    assert.deepEqual(errors, []);
    console.log(
      'Software update UI: silent startup checks, manual checks, portable fallback, task/file blockers, confirmation, progress, failure recovery, listener cleanup, language/theme and compact layout passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
