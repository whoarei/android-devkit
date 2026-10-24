// Deterministic IPC fixture: exercises review/target binding and never touches a real SDK/device.
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
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.addInitScript(() => {
      const target = {
        serial: 'phase2-phone',
        transportId: '7',
        serverEpoch: 1,
        model: 'Phase 2 phone',
        state: 'ready',
        api: '29',
        abi: 'x86_64',
        connection: 'usb',
        avdName: '',
      };
      window.isTauri = true;
      window.__calls = [];
      window.__callbacks = {};
      window.__listeners = {};
      window.__holdPicker = false;
      window.__rejectApk = false;
      window.__device = target;
      const apk = {
        id: 'set-one',
        package: 'dev.androiddevkit.fixture',
        version: '1.0',
        versionCode: '1',
        signers: ['a'.repeat(64)],
        members: [
          { fileName: 'base.apk', split: '', bytes: 1024, minSdk: 23, abis: [], sha256: 'base' },
          {
            fileName: 'config.zh.apk',
            split: 'config.zh',
            bytes: 512,
            minSdk: 23,
            abis: [],
            sha256: 'split',
          },
        ],
      };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      let callback = 0;
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback(fn) {
          window.__callbacks[++callback] = fn;
          return callback;
        },
        unregisterCallback() {},
        invoke: async (command, args) => {
          window.__calls.push({ command, args: structuredClone(args) });
          if (command === 'workspace_snapshot')
            return {
              setupRequired: false,
              settings: { sdkRoot: 'E:\\SDK', avdRoots: [] },
              environment: { adb: 'fixture', warnings: [] },
              packages: [],
              avds: [],
              devices: [structuredClone(window.__device)],
              tasks: [],
              server: 'fixture',
              serverEpoch: 1,
              freeBytes: 20 * 1024 ** 3,
            };
          if (command === 'window_state') return { maximized: false };
          if (command === 'log_read')
            return {
              rows: [],
              latest: 0,
              oldest: 0,
              dropped: 0,
              buffered: 0,
              active: false,
              status: 'fixture',
              processStatus: '',
              pids: [],
            };
          if (command === 'apk_set_tools') {
            if (window.__missingCatalog)
              throw { code: 'CATALOG_REQUIRED', message: 'Refresh catalog' };
            return window.__toolPackage || '';
          }
          if (command === 'catalog_refresh') {
            window.__missingCatalog = false;
            return [];
          }
          if (command === 'phone_choose')
            return window.__holdPicker
              ? new Promise((resolve) => {
                  window.__resolvePicker = resolve;
                })
              : args.kind === 'download_directory'
                ? ['C:\\Downloads']
                : args.kind === 'apk_files'
                  ? ['C:\\base.apk', 'C:\\config.zh.apk']
                  : ['C:\\中文 空格'];
          if (command === 'apk_set_prepare') {
            if (window.__rejectApk) throw { message: 'Fixture signature mismatch' };
            return apk;
          }
          if (command === 'device_files')
            return args.path === '/sdcard'
              ? [
                  {
                    name: 'Download',
                    path: '/sdcard/Download',
                    directory: true,
                    link: false,
                    bytes: 0,
                    modified: 0,
                  },
                ]
              : [
                  {
                    name: '中文 空格.txt',
                    path: '/sdcard/Download/中文 空格.txt',
                    directory: false,
                    link: false,
                    bytes: 8,
                    modified: 1700000000,
                  },
                  {
                    name: 'Link',
                    path: '/sdcard/Download/Link',
                    directory: true,
                    link: true,
                    bytes: 0,
                    modified: 0,
                  },
                ];
          if (command === 'transfer_prepare')
            return {
              id: 'transfer-one',
              direction: args.direction,
              target: args.target,
              directory: args.directory,
              roots: ['中文 空格.txt', 'another.txt'],
              items: [{ bytes: 8 }],
              conflicts: ['中文 空格.txt'],
            };
          if (command === 'plan_create')
            return {
              id: 'plan-one',
              hash: 'hash',
              title: 'Fixture plan',
              operation: args.operation,
              effects: ['Fixture review'],
              warnings: [],
              licenses: [],
              confirmationToken: 'token',
              createdAt: 1,
              settingsHash: 'hash',
            };
          if (command === 'plugin:event|listen') {
            window.__listeners[args.event] = args.handler;
            return args.handler;
          }
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    await page.locator('.device-row').filter({ hasText: 'Phase 2 phone' }).click();
    await page.getByRole('button', { name: '分包 APK 安装', exact: true }).click();
    const apkDialog = page.getByRole('dialog', { name: '分包 APK 安装', exact: true });
    await apkDialog.getByRole('button', { name: '选择多个 APK…' }).click();
    await apkDialog.locator('[data-apk-set]').waitFor();
    assert.match(await apkDialog.innerText(), /config.zh.apk/);
    await apkDialog.getByRole('button', { name: '确认分包安装计划' }).click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    let operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.kind, 'apk_set_install');
    assert.equal(operation.target.transportId, '7');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await apkDialog.getByRole('button', { name: '关闭面板' }).click();
    await page.getByRole('button', { name: '录屏', exact: true }).click();
    const record = page.getByRole('dialog', { name: '录屏', exact: true });
    assert.equal(await record.getByLabel('录屏时长（秒）').inputValue(), '30');
    await record.getByLabel('录屏时长（秒）').fill('181');
    assert.equal(await record.getByRole('button', { name: '确认录屏计划' }).isDisabled(), true);
    await record.getByLabel('录屏时长（秒）').fill('10');
    await record.getByLabel('分辨率', { exact: true }).selectOption('720p');
    await record.getByRole('button', { name: '确认录屏计划' }).click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.kind, 'screen_record');
    assert.deepEqual(operation.args, { seconds: '10', resolution: '720p' });
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await record.getByRole('button', { name: '关闭面板' }).click();
    await page.getByRole('button', { name: '文件传输', exact: true }).click();
    const files = page.getByRole('dialog', { name: '文件传输', exact: true });
    await files.getByRole('button', { name: 'Download', exact: true }).click();
    await files.getByLabel('选择：中文 空格.txt', { exact: true }).check();
    assert.equal(await files.getByLabel('选择：Link', { exact: true }).isDisabled(), true);
    await files.getByRole('button', { name: '下载所选…' }).click();
    await files.locator('[data-transfer-id]').waitFor();
    const review = files.getByRole('button', { name: '确认传输计划' });
    assert.equal(await review.isDisabled(), true);
    await files.getByLabel('跳过全部同名冲突项，保留现有文件').check();
    await review.click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.kind, 'file_download');
    assert.equal(operation.args.conflicts, 'skip');
    assert.equal(operation.target.serial, 'phase2-phone');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await files.getByLabel('为冲突项指定新名称').check();
    await files.getByLabel('新名称：中文 空格.txt').fill('renamed.txt');
    await review.click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.args.conflicts, 'rename');
    assert.equal(JSON.parse(operation.args.renames)['中文 空格.txt'], 'renamed.txt');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await files.getByLabel('替换已核对的同名目标及其全部内容').check();
    await review.click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.args.conflicts, 'replace');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await page.evaluate(() =>
      window.dispatchEvent(new CustomEvent('devkit-phone-drop', { detail: ['C:\\drag folder'] })),
    );
    await page.waitForFunction(() =>
      window.__calls.some((c) => c.command === 'transfer_prepare' && c.args.direction === 'upload'),
    );
    for (const width of [480, 1280]) {
      await page.setViewportSize({ width, height: 800 });
      assert.equal(await files.evaluate((el) => el.scrollWidth <= el.clientWidth + 1), true);
    }
    const dir = path.resolve('.qa/phase2-ui');
    fs.mkdirSync(dir, { recursive: true });
    await page.screenshot({ path: path.join(dir, 'files-1280.png') });
    await files.getByRole('button', { name: '关闭面板' }).click();
    // A picker returning after reconnect stays bound to the old transport and is rejected/abandoned.
    await page.evaluate(() => {
      window.__holdPicker = true;
    });
    await page.getByRole('button', { name: '分包 APK 安装', exact: true }).click();
    await apkDialog.getByRole('button', { name: '选择多个 APK…' }).click();
    await page.waitForFunction(() => !!window.__resolvePicker);
    await page.evaluate(() => {
      window.__device.transportId = '8';
      window.__resolvePicker(['C:\\base.apk']);
    });
    await page.getByRole('button', { name: '重新扫描', exact: true }).click();
    await page.waitForFunction(() => !document.querySelector('.detail-panel'));
    await page.evaluate(() => {
      window.__holdPicker = false;
    });
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await page.locator('.device-row').filter({ hasText: 'Phase 2 phone' }).click();
    await page.getByRole('button', { name: 'Screen recording', exact: true }).click();
    const englishRecord = page.getByRole('dialog', { name: 'Screen recording', exact: true });
    assert.equal(await englishRecord.getByLabel('Duration (seconds)').inputValue(), '30');
    await englishRecord.getByRole('button', { name: 'Close panel' }).click();
    await page.getByRole('button', { name: 'File transfer', exact: true }).click();
    const englishFiles = page.getByRole('dialog', { name: 'File transfer', exact: true });
    await englishFiles.getByRole('button', { name: 'Download', exact: true }).click();
    await englishFiles.getByRole('button', { name: 'Upload files…', exact: true }).click();
    await englishFiles.getByLabel('Rename conflicting items').check();
    await page.setViewportSize({ width: 480, height: 800 });
    assert.equal(await englishFiles.evaluate((el) => el.scrollWidth <= el.clientWidth + 1), true);
    await page.screenshot({ path: path.join(dir, 'files-english-480.png') });
    await englishFiles.getByRole('button', { name: 'Close panel' }).click();
    await page.evaluate(() => {
      window.__missingCatalog = true;
      window.__toolPackage = 'build-tools;35.0.0';
    });
    await page.getByRole('button', { name: 'Install split APKs', exact: true }).click();
    const englishApk = page.getByRole('dialog', { name: 'Install split APKs', exact: true });
    await englishApk
      .getByRole('button', { name: 'Check verification tools and refresh missing catalog' })
      .click();
    await englishApk.getByRole('button', { name: 'Prepare Build-Tools plan' }).click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.kind, 'sdk_install');
    assert.equal(operation.args.packages, 'build-tools;35.0.0');
    assert.deepEqual(errors, []);
    console.log(
      'Phone UI: split review, recording bounds/options, frozen target, shared-storage navigation, multiselect, link blocking, keep/rename/replace, native drop, reconnect, English and 480/1280 layouts passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
