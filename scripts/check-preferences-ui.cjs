const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const sourceRoot = path.join(__dirname, '../src');
for (const file of fs
  .readdirSync(sourceRoot, { recursive: true })
  .filter((file) => /\.tsx?$/.test(file) && file !== 'preferences.ts')) {
  assert(
    !/\b(?:localStorage|sessionStorage)\b/.test(
      fs.readFileSync(path.join(sourceRoot, file), 'utf8'),
    ),
    `${file} must use the central preferences store`,
  );
}
(async () => {
  const browser = await chromium.launch({
    channel: process.env.PLAYWRIGHT_CHANNEL || 'msedge',
    headless: true,
  });
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await require('./preferences-fixture.cjs')(page);
    await page.addInitScript(() => {
      if (!localStorage.getItem('fixture.seeded')) {
        localStorage.setItem('android-devkit.theme', 'dark');
        localStorage.setItem('android-devkit.locale', 'en-US');
        localStorage.setItem('android-devkit.default-target', 'device:usb:fixture');
        localStorage.setItem('android-devkit.auto-check-update', 'false');
        localStorage.setItem('fixture.seeded', 'true');
      }
      window.__failPreferences = localStorage.getItem('fixture.fail');
      window.isTauri = true;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: () => 1,
        unregisterCallback() {},
        invoke: async (command) => {
          if (command === 'workspace_snapshot')
            return {
              setupRequired: false,
              settings: {
                sdkRoot: 'E:\\fixture-sdk',
                avdRoots: ['E:\\fixture-avd'],
                javaPath: '',
                proxy: '',
                bootTimeoutSecs: 180,
                trayEnabled: false,
                sdkSites: [],
                sdkChannel: 0,
                sdkDownload: { kind: 'google', name: '', url: '', verifyOfficial: true },
              },
              environment: { adb: 'fixture', java: '', warnings: [] },
              devices: [
                {
                  serial: 'fixture',
                  transportId: '1',
                  serverEpoch: 1,
                  state: 'ready',
                  model: 'Fixture phone',
                  connection: 'usb',
                  avdName: '',
                  api: '35',
                  abi: 'arm64-v8a',
                },
              ],
              avds: [],
              packages: [],
              tasks: [],
              server: 'fixture',
              serverEpoch: 1,
              freeBytes: 1024 ** 3,
            };
          if (command === 'window_state') return { maximized: false };
          if (command === 'terminal_list' || command === 'sdk_sites_status') return [];
          if (command === 'task_list')
            return {
              tasks: [],
              total: 0,
              counts: {},
              offset: 0,
              pageSize: 30,
              deletion: { blocked: {}, eligible: 0 },
            };
          if (command === 'log_read')
            return {
              rows: [],
              latest: 0,
              oldest: 0,
              dropped: 0,
              buffered: 0,
              active: false,
              status: '',
              processStatus: '',
              pids: [],
            };
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    await page.getByRole('heading', { name: 'Device Workbench', exact: true }).waitFor();
    assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
    assert.deepEqual(
      await page.evaluate(() => [
        window.__preferences.locale,
        window.__preferences.defaultTarget,
        window.__preferences.autoCheckUpdates,
      ]),
      ['en-US', 'device:usb:fixture', false],
    );
    assert.equal(await page.evaluate(() => localStorage.getItem('android-devkit.theme')), null);
    await page.getByRole('button', { name: 'Switch to light theme', exact: true }).click();
    await page.getByRole('button', { name: 'Switch interface language', exact: true }).click();
    await page.locator('nav').getByRole('button', { name: 'SDK 管理', exact: true }).click();
    await page.getByRole('button', { name: 'SDK Tools', exact: true }).click();
    await page.getByLabel('组件状态筛选', { exact: true }).selectOption('installed');
    await page
      .getByRole('checkbox', { name: '隐藏过时组件（保留已安装项）', exact: true })
      .uncheck();
    await page.reload();
    await page.getByRole('heading', { name: 'SDK 管理', exact: true }).waitFor();
    assert.equal(await page.getByLabel('组件状态筛选', { exact: true }).inputValue(), 'installed');
    assert.equal(
      await page
        .getByRole('checkbox', { name: '隐藏过时组件（保留已安装项）', exact: true })
        .isChecked(),
      false,
    );
    assert.equal(await page.evaluate(() => window.__preferences.sdkTab), 'tools');
    // Selection-dependent views reset while durable filters survive.
    await page.getByLabel('组件状态筛选', { exact: true }).selectOption('selected');
    await page.reload();
    await page.getByLabel('组件状态筛选', { exact: true }).waitFor();
    assert.equal(await page.getByLabel('组件状态筛选', { exact: true }).inputValue(), 'installed');
    await page.locator('nav').getByRole('button', { name: '设备工作台', exact: true }).click();
    await page.getByRole('button', { name: /^真机/ }).click();
    await page.locator('.device-row').filter({ hasText: 'Fixture phone' }).click();
    await page.getByRole('button', { name: '录屏', exact: true }).click();
    const recording = page.getByRole('dialog', { name: '录屏', exact: true });
    await page.evaluate(() => {
      window.__preferenceDelay = 80;
    });
    await recording.getByLabel('录屏时长（秒）', { exact: true }).fill('45');
    await recording.getByLabel('录屏时长（秒）', { exact: true }).fill('30');
    await recording.getByLabel('分辨率', { exact: true }).selectOption('720p');
    await page.waitForFunction(
      () =>
        window.__preferences.recordingSeconds === 30 &&
        window.__preferences.recordingResolution === '720p',
    );
    await recording.getByRole('button', { name: '关闭面板', exact: true }).click();
    await page.reload();
    await page.getByRole('heading', { name: '设备工作台', exact: true }).waitFor();
    assert.equal(await page.evaluate(() => window.__preferences.deviceFilter), 'usb');
    await page.locator('.device-row').filter({ hasText: 'Fixture phone' }).click();
    await page.getByRole('button', { name: '录屏', exact: true }).click();
    assert.equal(await recording.getByLabel('录屏时长（秒）', { exact: true }).inputValue(), '30');
    assert.equal(await recording.getByLabel('分辨率', { exact: true }).inputValue(), '720p');
    await recording.getByRole('button', { name: '关闭面板', exact: true }).click();
    await page.evaluate(() => {
      window.__failPreferences = 'save';
    });
    await page.getByRole('button', { name: '切换为暗色主题', exact: true }).click();
    await page.getByText('用户配置保存失败，本次修改尚未保存。', { exact: true }).waitFor();
    assert.equal(await page.locator('html').getAttribute('data-theme'), 'light');
    await page.evaluate(() => {
      window.__failPreferences = null;
    });
    await page.getByRole('button', { name: '重试保存配置', exact: true }).click();
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    await page.evaluate(() => {
      localStorage.setItem('fixture.fail', 'load');
      localStorage.setItem('android-devkit.theme', 'light');
    });
    await page.reload();
    await page.getByText('用户配置读取失败，请重试。', { exact: true }).waitFor();
    assert.equal(await page.evaluate(() => localStorage.getItem('android-devkit.theme')), 'light');
    await page.evaluate(() => {
      window.__failPreferences = null;
      localStorage.removeItem('fixture.fail');
    });
    await page.getByRole('button', { name: '重试保存配置', exact: true }).click();
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    assert.equal(await page.evaluate(() => localStorage.getItem('android-devkit.theme')), null);
    assert.deepEqual(errors, []);
    console.log(
      'Preferences UI: legacy migration, SQLite precedence, navigation/filter/recording persistence, transient selection reset, rapid edits, read/write failure and retry passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
