// Isolated IPC fixtures; never download or install into the user's SDK.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

(async () => {
  const browser = await chromium.launch({
    channel: process.env.PLAYWRIGHT_CHANNEL || 'msedge',
    headless: true,
  });
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.addInitScript(() => {
      window.isTauri = true;
      window.__requests = [];
      window.__mode = 'normal';
      window.__fixture = {
        settings: {
          sdkRoot: 'E:\\SDK',
          avdRoots: ['E:\\AVD'],
          javaPath: '',
          proxy: '',
          bootTimeoutSecs: 180,
          trayEnabled: false,
          sdkSites: [],
          sdkChannel: 0,
          sdkDownload: { kind: 'google', name: '', url: '', verifyOfficial: true },
        },
        environment: { adb: 'fixture', warnings: [] },
        devices: [],
        tasks: [],
        avds: [],
        server: 'fixture',
        serverEpoch: 1,
        freeBytes: 20 * 1024 ** 3,
        packages: [
          {
            id: 'platform-tools',
            name: 'Android SDK Platform-Tools',
            version: '37.0.1',
            installed: false,
            installedVersion: '',
            category: 'tool',
            api: '',
            abi: '',
            tag: '',
            size: 1024,
            url: 'https://dl.google.com/android/repository/fixture.zip',
            checksum: 'a'.repeat(40),
            checksumType: 'sha1',
            license: '',
            licenseId: '',
            dependencies: [],
            dependencyMinVersions: {},
            obsolete: false,
            channel: 0,
            sourceUrl: '',
            metadataKey: '',
            officialChecksum: 'a'.repeat(40),
            officialStatus: 'ready',
            officialCheckedAt: 1,
          },
        ],
      };
      const base = (source) =>
        source.kind === 'google'
          ? 'https://dl.google.com/android/repository/'
          : source.kind === 'tencent'
            ? 'https://mirrors.cloud.tencent.com/AndroidSDK/'
            : source.url;
      const sites = (source) =>
        ['Android SDK', 'AOSP 系统镜像', 'Google APIs 系统镜像', 'Google Play 系统镜像'].map(
          (name, i) => ({
            name,
            url:
              base(source) +
              [
                'repository2-3.xml',
                'sys-img/android/sys-img2-3.xml',
                'sys-img/google_apis/sys-img2-3.xml',
                'sys-img/google_apis_playstore/sys-img2-3.xml',
              ][i],
            enabled: true,
            builtin: true,
            checkedAt: 1,
            packageCount: 10,
            error: window.__mode === 'network' && i === 3 ? '连接超时' : '',
          }),
        );
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: () => 1,
        unregisterCallback() {},
        invoke: async (command, args) => {
          if (command === 'workspace_snapshot') return structuredClone(window.__fixture);
          if (command === 'terminal_list') return [];
          if (command === 'window_state') return { maximized: false };
          if (command === 'sdk_sites_status') return sites(window.__fixture.settings.sdkDownload);
          if (command === 'sdk_source_check') {
            window.__requests.push({ command, source: args.source });
            if (
              args.source.kind === 'custom' &&
              (!args.source.url.startsWith('https://') || args.source.url.includes('.xml'))
            )
              throw {
                code: 'INVALID_SDK_SOURCE',
                message: '请输入有效的 HTTPS 仓库根地址，不能填写 XML 文件',
              };
            if (window.__mode === 'hold')
              await new Promise((resolve) => {
                window.__resolveCheck = resolve;
              });
            return {
              sites: sites(args.source),
              archiveUrl: base(args.source) + 'fixture.zip',
              archiveError: '',
              checkedAt: 1,
            };
          }
          if (command === 'sdk_source_apply') {
            window.__requests.push({ command, source: args.source });
            if (window.__mode === 'applyFailure')
              throw { code: 'SDK_SOURCE_UNAVAILABLE', message: '刷新失败，已保留原下载源与目录' };
            window.__fixture.settings.sdkDownload = structuredClone(args.source);
            window.__fixture.packages[0].url = base(args.source) + 'fixture.zip';
            return [];
          }
          if (command === 'task_list')
            return { tasks: [], total: 0, offset: 0, pageSize: 25, counts: {} };
          if (command === 'settings_save') {
            window.__fixture.settings = args.settings;
            return null;
          }
          if (command === 'check_app_update') return null;
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19328');
    await page.locator('nav').getByRole('button', { name: 'SDK 管理', exact: true }).click();
    await page.getByRole('button', { name: 'SDK Tools', exact: true }).click();
    await page.getByLabel('platform-tools 变更', { exact: true }).selectOption('install');
    await page.getByRole('button', { name: '更换来源', exact: true }).click();
    const panel = page.getByRole('region', { name: 'SDK 下载源', exact: true });
    const choice = (name) => panel.getByRole('radio', { name: new RegExp(name) });
    const apply = panel.getByRole('button', { name: '应用并刷新', exact: true });
    const check = async () => {
      await panel.getByRole('button', { name: '检查连接', exact: true }).click();
      await panel.getByRole('button', { name: '检查连接', exact: true }).waitFor();
    };
    assert.equal(
      await panel
        .getByRole('checkbox', { name: '使用 Google 官方校验值验证安装包', exact: true })
        .isChecked(),
      true,
    );
    await choice('腾讯云镜像').check();
    assert.match(await page.locator('.sdk-source-bar').innerText(), /Google 官方/);
    assert.equal(await apply.isDisabled(), true);
    await page.evaluate(() => {
      window.__mode = 'hold';
    });
    await panel.getByRole('button', { name: '检查连接', exact: true }).click();
    await page.waitForFunction(() => typeof window.__resolveCheck === 'function');
    assert.equal(await choice('自定义源').isDisabled(), true);
    await page.evaluate(() => {
      window.__mode = 'normal';
      window.__resolveCheck();
    });
    await panel.getByText('连接检查通过', { exact: true }).waitFor();
    assert.equal(await apply.isEnabled(), true);
    await apply.click();
    const dialog = page.getByRole('dialog', { name: '应用新的下载源？', exact: true });
    await dialog.getByText(/清空当前 1 项/).waitFor();
    await dialog.getByRole('button', { name: '继续编辑', exact: true }).click();
    assert.equal(await page.evaluate(() => window.__fixture.settings.sdkDownload.kind), 'google');
    await apply.click();
    await dialog.getByRole('button', { name: '应用并刷新目录', exact: true }).click();
    await page.waitForFunction(() =>
      document.querySelector('.sdk-source-bar')?.textContent.includes('腾讯云镜像'),
    );
    assert.equal(await page.locator('.change-bar').count(), 0);
    const out = path.resolve('.qa/app');
    fs.mkdirSync(out, { recursive: true });
    await page.screenshot({ path: path.join(out, 'sdk-mirror-tencent.png') });

    await choice('自定义源').check();
    await panel.getByLabel('仓库根地址', { exact: true }).fill('http://sdk.example/repo/');
    await check();
    await panel.getByRole('alert').filter({ hasText: 'HTTPS' }).waitFor();
    assert.equal(await apply.isDisabled(), true);
    await panel.getByLabel('源名称', { exact: true }).fill('团队 SDK 镜像');
    await panel.getByLabel('仓库根地址', { exact: true }).fill('https://sdk.example/repository');
    await check();
    assert.equal(
      await panel.getByLabel('仓库根地址', { exact: true }).inputValue(),
      'https://sdk.example/repository/',
    );
    assert.equal(await apply.isEnabled(), true);
    await panel
      .getByRole('checkbox', { name: '使用 Google 官方校验值验证安装包', exact: true })
      .uncheck();
    assert.equal(
      await apply.isDisabled(),
      true,
      'Changing verification invalidates the previous check',
    );
    await check();
    await apply.click();
    await panel.getByText('下载源已切换，目录已刷新', { exact: true }).waitFor();
    assert.equal(
      await page.evaluate(() => window.__fixture.settings.sdkDownload.verifyOfficial),
      false,
    );
    assert.match(await panel.innerText(), /仍按所选下载源提供的校验值验证完整性/);

    await choice('Google 官方').check();
    await page.evaluate(() => {
      window.__mode = 'network';
    });
    await check();
    assert.equal(await apply.isDisabled(), true);
    assert.equal(await page.evaluate(() => window.__fixture.settings.sdkDownload.kind), 'custom');
    await page.evaluate(() => {
      window.__mode = 'normal';
    });
    await check();
    await page.evaluate(() => {
      window.__mode = 'applyFailure';
    });
    await apply.click();
    await panel.getByRole('alert').filter({ hasText: '已保留原下载源' }).waitFor();
    assert.equal(await page.evaluate(() => window.__fixture.settings.sdkDownload.kind), 'custom');
    assert.equal(await apply.isDisabled(), true);

    await page.evaluate(() => {
      window.__mode = 'normal';
      window.__fixture.tasks = [
        {
          id: 'sdk-active',
          state: 'running',
          message: '安装 SDK',
          operation: { kind: 'sdk_install', args: {}, target: null },
        },
      ];
    });
    await check();
    await panel
      .getByText('SDK 安装任务进行中，结束后可应用', { exact: true })
      .waitFor({ timeout: 10000 });
    assert.equal(await apply.isDisabled(), true);
    await page.evaluate(() => {
      window.__fixture.tasks = [];
    });
    await page.waitForFunction(
      () => !document.querySelector('.sdk-source-actions .primary').disabled,
      { timeout: 10000 },
    );
    await apply.click();
    await page.waitForFunction(() =>
      document.querySelector('.sdk-source-bar')?.textContent.includes('Google 官方'),
    );
    await page.locator('nav').getByRole('button', { name: '设置', exact: true }).click();
    await page.getByRole('button', { name: '管理下载源', exact: true }).click();
    await panel.waitFor();
    await page.setViewportSize({ width: 820, height: 1000 });
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
      false,
    );
    await page.screenshot({ path: path.join(out, 'sdk-mirror-compact.png') });
    assert.deepEqual(errors, []);
    console.log(
      'SDK mirror fixtures passed: draft/apply, confirmation, custom URL, optional official verification, rollback, active task guard, settings entry and compact layout.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
