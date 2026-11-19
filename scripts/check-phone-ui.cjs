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
    await require('./preferences-fixture.cjs')(page);
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
      window.__tasks = [];
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
              tasks: structuredClone(window.__tasks),
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
          if (command === 'device_tools')
            return [{ name: 'bundletool', version: '1.18.3', ready: true }];
          if (command === 'catalog_refresh') {
            window.__missingCatalog = false;
            return [];
          }
          if (command === 'phone_choose')
            return window.__holdPicker
              ? new Promise((resolve) => {
                  window.__resolvePicker = resolve;
                })
              : args.kind === 'apk_container'
                ? ['C:\\fixture.apks']
                : args.kind === 'apk_files'
                  ? ['C:\\base.apk', 'C:\\config.zh.apk']
                  : ['C:\\中文 空格'];
          if (command === 'apk_set_prepare') {
            if (window.__rejectApk) throw { message: 'Fixture signature mismatch' };
            return apk;
          }
          if (command === 'apk_container_prepare') {
            if (window.__rejectContainer)
              throw { code: 'APK_CONTAINER_PATH', message: 'Fixture unsafe archive' };
            return { ...apk, target: args.target };
          }
          if (command === 'file_choose' && args.kind === 'folder') return 'D:\\Exports';
          if (command === 'local_files') {
            const folder = args.path ?? 'C:\\Downloads';
            const subfolder = folder.endsWith('\\资料');
            return {
              path: folder,
              parent: subfolder
                ? folder.slice(0, folder.lastIndexOf('\\'))
                : `${folder.slice(0, 2)}\\`,
              entries: subfolder
                ? []
                : [
                    {
                      path: `${folder}\\资料`,
                      name: '资料',
                      directory: true,
                      link: false,
                      bytes: 0,
                      modified: 0,
                    },
                    {
                      path: `${folder}\\中文 空格.txt`,
                      name: '中文 空格.txt',
                      directory: false,
                      link: false,
                      bytes: 8,
                      modified: 1700000000,
                    },
                    {
                      path: `${folder}\\Local link`,
                      name: 'Local link',
                      directory: false,
                      link: true,
                      bytes: 0,
                      modified: 0,
                    },
                  ],
            };
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
                    name: 'another.txt',
                    path: '/sdcard/Download/another.txt',
                    directory: false,
                    link: false,
                    bytes: 17,
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
          if (command === 'transfer_prepare') {
            if (args.paths.length !== 1)
              throw { message: 'Fixture requires a single transfer item' };
            const root = args.paths[0].split(/[\\/]/).at(-1);
            return {
              id: `transfer-${(window.__transferCount = (window.__transferCount || 0) + 1)}`,
              direction: args.direction,
              target: args.target,
              directory: args.directory,
              roots: [root],
              items: [{ relative: root, bytes: 8 }],
              conflicts: window.__transferWithoutConflicts ? [] : [root],
            };
          }
          if (command === 'plan_create') {
            if (window.__rejectPlan)
              throw { code: 'FILE_CONFLICT', message: 'Fixture target changed' };
            const plan = {
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
            window.__lastOperation = structuredClone(args.operation);
            if (window.__holdPlan)
              return new Promise((resolve) => {
                window.__resolvePlan = () => resolve(plan);
              });
            return plan;
          }
          if (command === 'task_submit') {
            if (window.__loseSubmitResponse) {
              window.__loseSubmitResponse = false;
              throw { code: 'IPC_DISCONNECTED', message: 'Fixture response lost after submission' };
            }
            const task = {
              id: `submitted-${args.requestId}`,
              requestId: args.requestId,
              title: '上传文件',
              operation: window.__lastOperation,
              state: 'running',
              phase: 'transferring',
              message: 'Fixture transfer in progress',
              progress: 25,
              cancelRequested: false,
              cancellable: true,
              artifact: '',
              log: [],
              createdAt: 1,
              updatedAt: 1,
              confirmedLicenses: [],
              licensePrompt: '',
            };
            window.__tasks = [task];
            return structuredClone(task);
          }
          if (command === 'task_cancel') {
            window.__tasks[0].cancelRequested = true;
            return null;
          }
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
    await apkDialog.getByRole('button', { name: '导入 APKS / XAPK / APKM…' }).click();
    await apkDialog.getByText('此集合已绑定当前设备连接；连接变化后请重新导入。').waitFor();
    const containerCall = await page.evaluate(() =>
      window.__calls.find((c) => c.command === 'apk_container_prepare'),
    );
    assert.equal(containerCall.args.path, 'C:\\fixture.apks');
    assert.equal(containerCall.args.target.serial, 'phase2-phone');
    assert.equal(containerCall.args.target.transportId, '7');
    assert.equal(containerCall.args.target.serverEpoch, 1);
    await page.evaluate(() => {
      window.__rejectContainer = true;
    });
    await apkDialog.getByRole('button', { name: '导入 APKS / XAPK / APKM…' }).click();
    await apkDialog.getByRole('alert').filter({ hasText: 'Fixture unsafe archive' }).waitFor();
    assert.equal(await apkDialog.locator('[data-apk-set]').count(), 0);
    await page.evaluate(() => {
      window.__rejectContainer = false;
    });
    await apkDialog.getByRole('button', { name: '重新导入所选安装包' }).click();
    await apkDialog.locator('[data-apk-set]').waitFor();
    // Failed re-selection must not leave the previous APK ready to install.
    await page.evaluate(() => {
      window.__rejectApk = true;
    });
    await apkDialog.getByRole('button', { name: '选择多个 APK…' }).click();
    await apkDialog.getByRole('alert').filter({ hasText: 'Fixture signature mismatch' }).waitFor();
    assert.equal(await apkDialog.locator('[data-apk-set]').count(), 0);
    await page.evaluate(() => {
      window.__rejectApk = false;
    });
    await apkDialog.getByRole('button', { name: '选择多个 APK…' }).click();
    await apkDialog.locator('[data-apk-set]').waitFor();
    // Plan failures belong inside the initiating dialog; slow requests disable resubmission.
    await page.evaluate(() => {
      window.__rejectPlan = true;
    });
    await apkDialog.getByRole('button', { name: '确认分包安装计划' }).click();
    await apkDialog.getByRole('alert').filter({ hasText: 'Fixture target changed' }).waitFor();
    await page.evaluate(() => {
      window.__rejectPlan = false;
      window.__holdPlan = true;
    });
    await apkDialog.getByRole('button', { name: '确认分包安装计划' }).click();
    await page.waitForFunction(() => !!window.__resolvePlan);
    assert.equal(
      await apkDialog.getByRole('button', { name: '确认分包安装计划' }).isDisabled(),
      true,
    );
    await page.keyboard.press('Tab');
    assert.equal(
      await apkDialog.evaluate((el) => el.contains(document.activeElement)),
      true,
      'Busy dialogs keep keyboard focus inside',
    );
    await page.evaluate(() => {
      window.__holdPlan = false;
      window.__resolvePlan();
    });
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    let operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.kind, 'apk_set_install');
    assert.equal(operation.target.transportId, '7');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await apkDialog.getByRole('button', { name: '确认分包安装计划' }).waitFor({ state: 'visible' });
    const controls = apkDialog.locator(
      'button:not(:disabled),input:not(:disabled),select:not(:disabled)',
    );
    await controls.last().focus();
    await page.keyboard.press('Tab');
    assert.equal(await controls.first().evaluate((el) => el === document.activeElement), true);
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
    const local = files.getByRole('region', { name: '本地', exact: true });
    const android = files.getByRole('region', { name: 'Android', exact: true });
    const download = android.getByRole('button', {
      name: '将 中文 空格.txt 下载到本地',
      exact: true,
    });
    const upload = local.getByRole('button', { name: '将 资料 上传到 Android', exact: true });
    await local.locator('header').getByText('C:\\Downloads', { exact: true }).waitFor();
    await android.locator('header').getByText('/sdcard/Download', { exact: true }).waitFor();
    assert.equal(await files.locator('footer').count(), 0);
    assert.equal(await files.getByRole('checkbox').count(), 0);
    await local.getByRole('button', { name: '选择目录', exact: true }).click();
    await local.getByText('D:\\Exports', { exact: true }).waitFor();
    await local.getByRole('button', { name: '资料', exact: true }).click();
    await local.getByText('目录为空', { exact: true }).waitFor();
    await local.getByRole('button', { name: '上级目录', exact: true }).click();
    await local.getByText('D:\\Exports', { exact: true }).waitFor();
    assert.equal(
      await local.getByRole('button', { name: '将 Local link 上传到 Android' }).isDisabled(),
      true,
    );
    await android.getByRole('button', { name: '上级目录', exact: true }).click();
    const rootDownload = android.getByRole('button', {
      name: '将 Download 下载到本地',
      exact: true,
    });
    await rootDownload.waitFor();
    assert.equal(await rootDownload.isDisabled(), true);
    await android.getByRole('button', { name: 'Download', exact: true }).click();
    assert.equal(
      await android.getByRole('button', { name: '将 Link 下载到本地', exact: true }).isDisabled(),
      true,
    );
    await download.click();
    await files.locator('[data-transfer-id]').waitFor();
    assert.deepEqual(
      await page.evaluate(
        () => window.__calls.find((call) => call.command === 'transfer_prepare').args.directory,
      ),
      'D:\\Exports',
    );
    assert.deepEqual(
      await page.evaluate(
        () => window.__calls.find((call) => call.command === 'transfer_prepare').args.paths,
      ),
      ['/sdcard/Download/中文 空格.txt'],
    );
    const review = files.getByRole('button', { name: '确认传输计划' });
    assert.equal(await review.isDisabled(), true);
    await files.getByRole('radio', { name: '重命名', exact: true }).check();
    await files.getByLabel('新名称：中文 空格.txt').fill('renamed.txt');
    await review.click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.kind, 'file_download');
    assert.equal(operation.target.serial, 'phase2-phone');
    assert.equal(operation.args.conflicts, 'rename');
    assert.equal(JSON.parse(operation.args.renames)['中文 空格.txt'], 'renamed.txt');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await files.getByRole('radio', { name: '替换同名目标', exact: true }).check();
    await review.click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    operation = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(operation.args.conflicts, 'replace');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await files.getByRole('button', { name: '取消', exact: true }).click();
    await files.locator('[data-transfer-id]').waitFor({ state: 'detached' });
    const transferCount = () =>
      page.evaluate(() => window.__calls.filter((c) => c.command === 'transfer_prepare').length);
    const countBeforeDrop = await transferCount();
    await page.evaluate(() =>
      window.dispatchEvent(
        new CustomEvent('devkit-phone-drop', {
          detail: ['C:\\拖入\\中文 空格.txt', 'C:\\拖入\\another.txt'],
        }),
      ),
    );
    await files.getByRole('alert').filter({ hasText: '每次只能传输一个文件或目录。' }).waitFor();
    assert.equal(await transferCount(), countBeforeDrop);
    await page.evaluate(() =>
      window.dispatchEvent(
        new CustomEvent('devkit-phone-drop', { detail: ['C:\\拖入\\中文 空格.txt'] }),
      ),
    );
    await page.waitForFunction(() =>
      window.__calls.some((c) => c.command === 'transfer_prepare' && c.args.direction === 'upload'),
    );
    await files.getByRole('radio', { name: '重命名', exact: true }).check();
    await review.click();
    const execution = page.getByRole('dialog', { name: 'Fixture plan' });
    await page.evaluate(() => {
      window.__loseSubmitResponse = true;
    });
    await execution.getByRole('button', { name: 'Fixture plan', exact: true }).click();
    await execution
      .getByRole('alert')
      .filter({ hasText: 'Fixture response lost after submission' })
      .waitFor();
    await execution.getByRole('button', { name: 'Fixture plan', exact: true }).click();
    await files.getByText('Fixture transfer in progress', { exact: true }).waitFor();
    assert.equal(await files.locator('[data-transfer-id]').count(), 0);
    const submissions = await page.evaluate(() =>
      window.__calls.filter((c) => c.command === 'task_submit').map((c) => c.args.requestId),
    );
    assert.equal(submissions.length, 2);
    assert.equal(submissions[0], submissions[1]);
    assert.equal(await files.getByRole('progressbar').getAttribute('value'), '25');
    await files.getByRole('button', { name: '取消传输', exact: true }).click();
    assert.equal(
      await page.evaluate(() => window.__calls.filter((c) => c.command === 'task_cancel').length),
      1,
    );
    const readsBefore = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'device_files').length,
    );
    await page.evaluate(() => {
      window.__tasks[0].state = 'cancelled';
      window.__tasks[0].operation.args.backupsResolved = 'true';
      window.__callbacks[window.__listeners['workspace-changed']]({ payload: 2 });
    });
    await page.waitForFunction(
      (before) => window.__calls.filter((c) => c.command === 'device_files').length > before,
      readsBefore,
    );
    assert.equal(
      await files.getByRole('button', { name: '清理原传输暂存', exact: true }).count(),
      0,
    );
    // A transfer without conflicts has no extra dialog, and a retry uses the original request ID.
    await page.evaluate(() => {
      window.__transferWithoutConflicts = true;
      window.__loseSubmitResponse = true;
    });
    await upload.click();
    await files
      .getByRole('alert')
      .filter({ hasText: 'Fixture response lost after submission' })
      .waitFor();
    assert.equal(await page.getByRole('dialog').count(), 1);
    await files.getByRole('button', { name: '重试传输', exact: true }).click();
    await files.getByRole('progressbar').waitFor();
    const direct = await page.evaluate(() => ({
      submissions: window.__calls
        .filter((c) => c.command === 'task_submit')
        .slice(-2)
        .map((c) => c.args.requestId),
      transfer: window.__calls.filter((c) => c.command === 'transfer_prepare').at(-1).args,
    }));
    assert.equal(direct.submissions[0], direct.submissions[1]);
    assert.equal(direct.transfer.directory, '/sdcard/Download');
    assert.deepEqual(direct.transfer.paths, ['D:\\Exports\\资料']);
    assert.equal(direct.transfer.target.transportId, '7');
    await page.evaluate(() => {
      window.__tasks[0].state = 'succeeded';
      window.__tasks[0].message = '已传输 1 项';
      window.__tasks[0].operation.args.backupsResolved = 'true';
      window.__callbacks[window.__listeners['workspace-changed']]({ payload: 3 });
    });
    await files.getByRole('progressbar').waitFor({ state: 'detached' });
    await files.getByText('已传输 1 项', { exact: true }).waitFor();

    // The Android row arrow also starts a single download by keyboard.
    await android
      .getByRole('button', { name: '将 another.txt 下载到本地', exact: true })
      .press('Enter');
    await files.getByRole('progressbar').waitFor();
    assert.equal(await page.getByRole('dialog').count(), 1);
    const request = await page.evaluate(
      () => window.__calls.filter((call) => call.command === 'transfer_prepare').at(-1).args,
    );
    assert.equal(request.direction, 'download');
    assert.deepEqual(request.paths, ['/sdcard/Download/another.txt']);
    assert.equal(request.directory, 'D:\\Exports');
    await page.evaluate(() => {
      Object.assign(window.__tasks[0], { state: 'succeeded', message: '已传输 1 项' });
      window.__tasks[0].operation.args.backupsResolved = 'true';
      window.__callbacks[window.__listeners['workspace-changed']]({ payload: 4 });
      window.__transferWithoutConflicts = false;
    });
    await files.getByRole('progressbar').waitFor({ state: 'detached' });
    assert.equal(await files.locator('footer').count(), 0);
    assert.equal(await files.getByRole('checkbox').count(), 0);
    for (const width of [480, 1280]) {
      await page.setViewportSize({ width, height: 800 });
      assert.equal(await files.evaluate((el) => el.scrollWidth <= el.clientWidth + 1), true);
      const boxes = await Promise.all([local.boundingBox(), android.boundingBox()]);
      if (width === 1280) {
        assert.ok(boxes[0].x < boxes[1].x);
        assert.equal(boxes[0].y, boxes[1].y);
        assert.ok((await files.boundingBox()).width >= 960);
      } else {
        assert.ok(boxes[0].y < boxes[1].y);
      }
    }
    const dir = path.resolve('.qa/phase2-ui');
    fs.mkdirSync(dir, { recursive: true });
    await files.screenshot({ path: path.join(dir, 'files-1280.png') });
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
    assert.equal(await englishRecord.getByLabel('Duration (seconds)').inputValue(), '10');
    await englishRecord.getByRole('button', { name: 'Close panel' }).click();
    await page.getByRole('button', { name: 'File transfer', exact: true }).click();
    const englishFiles = page.getByRole('dialog', { name: 'File transfer', exact: true });
    const englishLocal = englishFiles.getByRole('region', { name: 'Local', exact: true });
    const englishUpload = englishLocal.getByRole('button', {
      name: 'Upload 中文 空格.txt to Android',
      exact: true,
    });
    await englishUpload.waitFor();
    await page.setViewportSize({ width: 480, height: 800 });
    await englishFiles.screenshot({ path: path.join(dir, 'files-english-480.png') });
    await englishUpload.click();
    await englishFiles.getByRole('radio', { name: 'Rename', exact: true }).check();
    assert.equal(await englishFiles.evaluate((el) => el.scrollWidth <= el.clientWidth + 1), true);
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
    await page.getByRole('button', { name: 'Back to editing', exact: true }).click();
    await page.evaluate(() => {
      window.__toolPackage = '';
      window.__tasks.push({
        id: 'tools-completed',
        operation: { kind: 'sdk_install', args: {} },
        state: 'succeeded',
      });
      window.__callbacks[window.__listeners['workspace-changed']]({ payload: 3 });
    });
    await englishApk
      .getByRole('button', { name: 'Choose APK files…', exact: true })
      .waitFor({ state: 'visible' });
    await page.waitForFunction(() => !document.querySelector('.phone-modal .button')?.disabled);
    assert.equal(
      await englishApk.getByRole('button', { name: 'Prepare Build-Tools plan' }).count(),
      0,
    );
    assert.deepEqual(errors, []);
    console.log(
      'Phone UI: split review, recording, dual-pane navigation, single-item transfers, rename/replace, retries, drop limits, reconnect, and 480/1280 layouts passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
