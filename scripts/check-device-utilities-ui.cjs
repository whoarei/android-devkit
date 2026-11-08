// Production UI with deterministic IPC. No SDK, real device, app or key is touched.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

(async () => {
  const browser = await chromium.launch({
    channel: process.env.PLAYWRIGHT_CHANNEL || 'msedge',
    headless: true,
  });
  const checks = [];
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    await require('./preferences-fixture.cjs')(page);
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.addInitScript(() => {
      const primary = {
        serial: 'utilities-phone-a',
        transportId: '21',
        serverEpoch: 8,
        model: 'Utilities phone A',
        state: 'ready',
        api: '29',
        abi: 'x86_64',
        connection: 'usb',
        avdName: '',
      };
      const secondary = {
        ...primary,
        serial: 'utilities-phone-b',
        transportId: '22',
        model: 'Utilities phone B',
      };
      window.isTauri = true;
      window.__calls = [];
      window.__callbacks = {};
      window.__listeners = {};
      window.__devices = [primary, secondary];
      window.__tasks = [];
      window.__toolReady = false;
      window.__apps = [
        {
          package: 'dev.example.alpha',
          versionCode: '10',
          uid: '10101',
          system: false,
          disabled: false,
        },
        {
          package: 'dev.example.beta',
          versionCode: '11',
          uid: '10102',
          system: false,
          disabled: false,
        },
        {
          package: 'com.android.settings',
          versionCode: '29',
          uid: '1000',
          system: true,
          disabled: false,
        },
      ];
      let callback = 0;
      let taskNumber = 0;
      window.__publish = () => {
        const handler = window.__callbacks[window.__listeners['workspace-changed']];
        if (handler) handler({ payload: Date.now() });
      };
      const detail = (args) => {
        const app = window.__apps.find((app) => app.package === args.package);
        if (!app) throw { code: 'APP_NOT_FOUND', message: 'Fixture app removed' };
        return {
          app: structuredClone(app),
          userId: 10,
          versionName: '1.2.3',
          minSdk: '23',
          targetSdk: '29',
          paths: [
            `/data/app/${args.package}/base.apk`,
            `/data/app/${args.package}/split_config.en.apk`,
          ],
          fingerprint: `fingerprint-${args.target.serial}-${args.package}`,
        };
      };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
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
              devices: structuredClone(window.__devices),
              tasks: structuredClone(window.__tasks),
              server: 'fixture',
              serverEpoch: 8,
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
          if (command === 'device_app_list') {
            if (window.__rejectList)
              throw { code: 'APP_LIST', message: 'Fixture application list denied' };
            return { userId: 10, apps: structuredClone(window.__apps) };
          }
          if (command === 'device_app_detail') {
            const result = detail(args);
            if (window.__holdDetail === args.package)
              return new Promise((resolve) => {
                window.__resolveDetail = () => resolve(result);
              });
            return result;
          }
          if (command === 'device_app_prepare') {
            if (window.__rejectPrepare)
              throw { code: 'APP_CHANGED', message: 'Fixture application changed before review' };
            const result = {
              package: args.package,
              userId: '10',
              version: '1.2.3',
              appFingerprint: detail(args).fingerprint,
              system: String(detail(args).app.system),
            };
            if (window.__holdPrepare)
              return new Promise((resolve) => {
                window.__resolvePrepare = () => resolve(result);
              });
            return result;
          }
          if (command === 'device_tools')
            return [
              { name: 'scrcpy', version: '5.0.1', ready: window.__toolReady },
              { name: 'bundletool', version: '1.18.3', ready: false },
            ];
          if (command === 'plan_create') {
            window.__lastOperation = structuredClone(args.operation);
            return {
              id: `plan-${taskNumber}`,
              hash: 'hash',
              title: 'Utilities fixture plan',
              operation: args.operation,
              effects: [`${args.operation.kind} · ${args.operation.args.package || 'scrcpy'}`],
              warnings: [],
              licenses: [],
              confirmationToken: 'token',
              createdAt: 1,
              settingsHash: 'hash',
            };
          }
          if (command === 'task_submit') {
            const operation = structuredClone(window.__lastOperation);
            if (operation.kind === 'device_tool_prepare') window.__toolReady = true;
            if (operation.kind === 'device_app_uninstall')
              window.__apps = window.__apps.filter((app) => app.package !== operation.args.package);
            const task = {
              id: `utilities-task-${++taskNumber}`,
              requestId: args.requestId,
              title: operation.kind,
              operation,
              state: operation.kind === 'mirror_start' ? 'running' : 'succeeded',
              phase: operation.kind === 'mirror_start' ? 'mirroring' : 'complete',
              message:
                operation.kind === 'mirror_start'
                  ? 'Fixture mirroring active'
                  : 'Fixture operation completed',
              progress: null,
              cancelRequested: false,
              cancellable: true,
              artifact: '',
              log: [],
              createdAt: taskNumber,
              updatedAt: taskNumber,
              confirmedLicenses: [],
              licensePrompt: '',
            };
            window.__tasks.push(task);
            queueMicrotask(window.__publish);
            return structuredClone(task);
          }
          if (command === 'task_cancel') {
            const task = window.__tasks.find((task) => task.id === args.taskId);
            if (task) {
              task.state = 'cancelled';
              task.cancelRequested = true;
              task.updatedAt++;
            }
            queueMicrotask(window.__publish);
            return null;
          }
          if (command === 'task_list')
            return {
              tasks: window.__tasks,
              total: window.__tasks.length,
              page: 0,
              pageSize: 30,
              counts: {},
            };
          if (command === 'artifacts_list') return [];
          if (command === 'plugin:event|listen') {
            window.__listeners[args.event] = args.handler;
            return args.handler;
          }
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    await page.locator('.device-row').filter({ hasText: 'Utilities phone A' }).click();
    await page.getByRole('button', { name: '应用管理', exact: true }).click();
    let apps = page.getByRole('dialog', { name: '应用管理', exact: true });
    await apps.getByRole('option', { name: /dev.example.alpha/ }).waitFor();
    assert.equal(await apps.getByRole('option').count(), 2);
    assert.match(await apps.innerText(), /Android 用户 10/);
    assert.match(await apps.innerText(), /utilities-phone-a/);
    checks.push('explicit device and Android user; third-party apps by default');

    await apps.getByLabel('搜索已安装应用').fill('BETA');
    assert.equal(await apps.getByRole('option').count(), 1);
    await apps.getByLabel('搜索已安装应用').fill('');
    await apps.getByRole('option', { name: /dev.example.alpha/ }).click();
    await apps.getByRole('heading', { name: 'dev.example.alpha', exact: true }).waitFor();
    await apps.getByRole('option', { name: /dev.example.alpha/ }).focus();
    await page.keyboard.press('ArrowDown');
    await apps.getByRole('heading', { name: 'dev.example.beta', exact: true }).waitFor();
    assert.equal(
      await apps.getByRole('option', { name: /dev.example.beta/ }).getAttribute('aria-selected'),
      'true',
    );
    checks.push('case-insensitive search and keyboard app selection');

    // Late metadata for a previous selection must not replace the current app.
    await page.evaluate(() => {
      window.__holdDetail = 'dev.example.alpha';
    });
    await apps.getByRole('option', { name: /dev.example.alpha/ }).click();
    await page.waitForFunction(() => !!window.__resolveDetail);
    await apps.getByRole('option', { name: /dev.example.beta/ }).click();
    await apps.getByRole('heading', { name: 'dev.example.beta', exact: true }).waitFor();
    await page.evaluate(() => {
      window.__holdDetail = '';
      window.__resolveDetail();
    });
    assert.equal(
      await apps.getByRole('heading', { name: 'dev.example.alpha', exact: true }).count(),
      0,
    );
    checks.push('stale detail response cannot replace a newer selection');

    const lastOperation = () => page.evaluate(() => window.__lastOperation);
    const back = () => page.getByRole('button', { name: '返回修改', exact: true }).click();
    for (const [label, action] of [
      ['启动应用', 'launch'],
      ['停止应用', 'stop'],
      ['导出 APK', 'export'],
      ['清除应用数据', 'clear'],
      ['卸载应用', 'uninstall'],
    ]) {
      await apps.getByRole('heading', { name: 'dev.example.beta', exact: true }).waitFor();
      await apps.getByRole('button', { name: label, exact: true }).click();
      await page.getByRole('dialog', { name: 'Utilities fixture plan', exact: true }).waitFor();
      const operation = await lastOperation();
      assert.equal(operation.kind, `device_app_${action}`);
      assert.equal(operation.args.package, 'dev.example.beta');
      assert.equal(operation.args.userId, '10');
      assert.equal(operation.target.serial, 'utilities-phone-a');
      assert.equal(operation.target.transportId, '21');
      assert.equal(operation.target.serverEpoch, 8);
      await back();
    }
    assert.match(await apps.innerText(), /清除数据不可恢复/);
    checks.push('all five app actions review the exact package, Android user and frozen transport');

    await page.evaluate(() => {
      window.__rejectPrepare = true;
    });
    await apps.getByRole('button', { name: '停止应用', exact: true }).click();
    await apps
      .getByRole('alert')
      .filter({ hasText: 'Fixture application changed before review' })
      .waitFor();
    assert.equal(
      await page.getByRole('dialog', { name: 'Utilities fixture plan', exact: true }).count(),
      0,
    );
    await page.evaluate(() => {
      window.__rejectPrepare = false;
      window.__holdPrepare = true;
    });
    const preparesBefore = await page.evaluate(
      () => window.__calls.filter((call) => call.command === 'device_app_prepare').length,
    );
    await apps.getByRole('button', { name: '停止应用', exact: true }).evaluate((button) => {
      button.click();
      button.click();
    });
    await page.waitForFunction(() => !!window.__resolvePrepare);
    assert.equal(
      await apps.getByRole('button', { name: '停止应用', exact: true }).isDisabled(),
      true,
    );
    await page.keyboard.press('Tab');
    assert.equal(await apps.evaluate((element) => element.contains(document.activeElement)), true);
    assert.equal(
      await page.evaluate(
        () => window.__calls.filter((call) => call.command === 'device_app_prepare').length,
      ),
      preparesBefore + 1,
    );
    await page.evaluate(() => {
      window.__holdPrepare = false;
      window.__resolvePrepare();
    });
    await page.getByRole('dialog', { name: 'Utilities fixture plan', exact: true }).waitFor();
    await back();
    checks.push('inline changed-app error, synchronous duplicate guard and busy focus containment');

    await apps.getByLabel('显示系统应用').check();
    await apps.getByRole('option', { name: /com.android.settings/ }).click();
    await apps.getByRole('heading', { name: 'com.android.settings', exact: true }).waitFor();
    for (const name of ['停止应用', '清除应用数据', '卸载应用'])
      assert.equal(await apps.getByRole('button', { name, exact: true }).isDisabled(), true);
    assert.equal(
      await apps.getByRole('button', { name: '导出 APK', exact: true }).isEnabled(),
      true,
    );
    checks.push(
      'system app mutations disabled while details, launch and APK export remain available',
    );

    // Completion invalidates the list and removes an uninstalled selection.
    await apps.getByRole('option', { name: /dev.example.beta/ }).click();
    await apps.getByRole('heading', { name: 'dev.example.beta', exact: true }).waitFor();
    await apps.getByRole('button', { name: '卸载应用', exact: true }).click();
    await page
      .getByRole('dialog', { name: 'Utilities fixture plan', exact: true })
      .getByRole('button', { name: 'Utilities fixture plan', exact: true })
      .click();
    await apps.getByRole('option', { name: /dev.example.beta/ }).waitFor({ state: 'detached' });
    assert.equal(
      await apps.getByRole('heading', { name: 'dev.example.beta', exact: true }).count(),
      0,
    );
    checks.push('settled app task refreshes list and clears removed selection');

    // Closing a pending read and selecting another device must discard its response.
    await page.evaluate(() => {
      window.__holdDetail = 'dev.example.alpha';
      window.__resolveDetail = null;
    });
    await apps.getByRole('option', { name: /dev.example.alpha/ }).click();
    await page.waitForFunction(() => !!window.__resolveDetail);
    await apps.getByRole('button', { name: '关闭面板', exact: true }).click();
    await page.locator('.device-row').filter({ hasText: 'Utilities phone B' }).click();
    await page.getByRole('button', { name: '应用管理', exact: true }).click();
    apps = page.getByRole('dialog', { name: '应用管理', exact: true });
    await apps.getByRole('option', { name: /dev.example.alpha/ }).waitFor();
    await page.evaluate(() => {
      window.__holdDetail = '';
      window.__resolveDetail();
    });
    assert.match(await apps.innerText(), /utilities-phone-b/);
    assert.equal(
      await apps.getByRole('heading', { name: 'dev.example.alpha', exact: true }).count(),
      0,
    );
    checks.push('closing and switching devices discards pending metadata');
    await apps.getByRole('option', { name: /dev.example.alpha/ }).click();
    await apps.getByRole('heading', { name: 'dev.example.alpha', exact: true }).waitFor();
    await page.evaluate(() => {
      window.__devices[1].state = 'offline';
      window.__publish();
    });
    await apps.getByText('目标设备暂不可用，请等待原连接恢复或重新打开操作面板。').waitFor();
    for (const name of ['启动应用', '停止应用', '导出 APK', '清除应用数据', '卸载应用'])
      assert.equal(await apps.getByRole('button', { name, exact: true }).isDisabled(), true);
    await page.evaluate(() => {
      window.__devices[1].state = 'ready';
      window.__publish();
    });
    await apps.getByRole('heading', { name: 'dev.example.alpha', exact: true }).waitFor();
    checks.push('disconnected target disables every app action without switching devices');
    await apps.getByRole('button', { name: '关闭面板', exact: true }).click();

    await page.getByRole('button', { name: '投屏与控制', exact: true }).click();
    let mirror = page.getByRole('dialog', { name: '投屏与控制', exact: true });
    await mirror.getByRole('button', { name: '准备投屏工具', exact: true }).waitFor();
    assert.equal(
      await mirror.getByRole('button', { name: '开始投屏', exact: true }).isDisabled(),
      true,
    );
    await mirror.getByRole('button', { name: '准备投屏工具', exact: true }).click();
    await page.getByRole('dialog', { name: 'Utilities fixture plan', exact: true }).waitFor();
    let operation = await lastOperation();
    assert.equal(operation.kind, 'device_tool_prepare');
    assert.equal(operation.args.tool, 'scrcpy');
    assert.equal(operation.target, null);
    await page
      .getByRole('dialog', { name: 'Utilities fixture plan', exact: true })
      .getByRole('button', { name: 'Utilities fixture plan', exact: true })
      .click();
    await mirror.getByText('投屏工具已就绪', { exact: false }).waitFor();
    checks.push('missing scrcpy blocks mirroring and opens reviewed private-tool preparation');

    await page.evaluate(() => {
      for (const [id, change] of [
        ['other-transport', { transportId: '999' }],
        ['other-epoch', { serverEpoch: 9 }],
      ])
        window.__tasks.push({
          id,
          operation: {
            kind: 'mirror_start',
            args: {},
            target: { ...window.__devices[1], ...change },
          },
          state: 'running',
          updatedAt: 99,
        });
      window.__publish();
    });
    await mirror.getByRole('button', { name: '开始投屏', exact: true }).waitFor();
    assert.equal(await mirror.getByRole('button', { name: '停止投屏', exact: true }).count(), 0);
    await page.evaluate(() => {
      window.__tasks = window.__tasks.filter(
        (task) => !['other-transport', 'other-epoch'].includes(task.id),
      );
      window.__publish();
    });
    checks.push(
      'same serial on another transport or ADB epoch cannot claim the current mirroring session',
    );

    await mirror.getByLabel('控制模式', { exact: true }).selectOption('false');
    await mirror.getByLabel('画面最长边', { exact: true }).selectOption('1920');
    await mirror.getByLabel('最高帧率', { exact: true }).selectOption('30');
    await mirror.getByRole('button', { name: '开始投屏', exact: true }).click();
    await page.getByRole('dialog', { name: 'Utilities fixture plan', exact: true }).waitFor();
    operation = await lastOperation();
    assert.equal(operation.kind, 'mirror_start');
    assert.deepEqual(operation.args, { control: 'false', maxSize: '1920', maxFps: '30' });
    assert.equal(operation.target.serial, 'utilities-phone-b');
    assert.equal(operation.target.transportId, '22');
    await page
      .getByRole('dialog', { name: 'Utilities fixture plan', exact: true })
      .getByRole('button', { name: 'Utilities fixture plan', exact: true })
      .click();
    await mirror.getByRole('button', { name: '停止投屏', exact: true }).waitFor();
    assert.equal(await mirror.getByLabel('控制模式', { exact: true }).isDisabled(), true);
    await mirror.getByRole('button', { name: '停止投屏', exact: true }).click();
    await mirror.getByRole('button', { name: '开始投屏', exact: true }).waitFor();
    assert.equal(
      await page.evaluate(
        () => window.__calls.filter((call) => call.command === 'task_cancel').at(-1).args.taskId,
      ),
      'utilities-task-3',
    );
    checks.push(
      'read-only, resolution and FPS options preserve the target; stop cancels only owned session',
    );

    await mirror.getByRole('button', { name: '关闭面板', exact: true }).click();
    await page.getByRole('button', { name: '切换为暗色主题', exact: true }).click();
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await page.getByRole('button', { name: 'App management', exact: true }).click();
    const englishApps = page.getByRole('dialog', { name: 'App management', exact: true });
    await englishApps.getByRole('option', { name: /dev.example.alpha/ }).click();
    await englishApps.getByRole('heading', { name: 'dev.example.alpha', exact: true }).waitFor();
    assert.equal(
      await englishApps.getByRole('button', { name: 'Clear app data', exact: true }).count(),
      1,
    );
    await page.setViewportSize({ width: 480, height: 900 });
    assert.equal(
      await englishApps.evaluate((element) => element.scrollWidth <= element.clientWidth + 1),
      true,
    );
    const output = path.resolve('.qa/app/device-utilities');
    fs.mkdirSync(output, { recursive: true });
    await page.screenshot({ path: path.join(output, 'apps-en-dark-480.png') });
    const controls = englishApps.locator(
      'button:not(:disabled),input:not(:disabled),select:not(:disabled),summary',
    );
    await controls.last().focus();
    await page.keyboard.press('Tab');
    assert.equal(
      await englishApps.evaluate((element) => element.contains(document.activeElement)),
      true,
    );
    await englishApps.getByRole('button', { name: 'Close panel', exact: true }).click();
    await page.getByRole('button', { name: 'Screen mirroring and control', exact: true }).click();
    const englishMirror = page.getByRole('dialog', {
      name: 'Screen mirroring and control',
      exact: true,
    });
    await englishMirror.getByRole('button', { name: 'Start mirroring', exact: true }).waitFor();
    assert.equal(
      await englishMirror.getByLabel('Control mode', { exact: true }).inputValue(),
      'true',
    );
    assert.equal(
      await englishMirror.evaluate((element) => element.scrollWidth <= element.clientWidth + 1),
      true,
    );
    await page.screenshot({ path: path.join(output, 'mirror-en-dark-480.png') });
    checks.push('English, dark theme, 480px layout, focus trap and closed-form reset');
    assert.deepEqual(errors, []);
    fs.writeFileSync(
      path.join(output, 'report.json'),
      JSON.stringify({ checks, pageErrors: errors }, null, 2),
    );
    console.log(
      `Device utilities UI: ${checks.length} groups passed (apps, frozen target, mutations, async races, dependencies, mirroring, English and narrow dark layout).`,
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
