// Isolated device interactions: mock IPC, no file dialogs or real device writes.
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
    page.on('pageerror', (error) => errors.push(error.message));
    await page.addInitScript(() => {
      const device = (serial, transportId, model, extra = {}) => ({
        serial,
        transportId,
        serverEpoch: 1,
        model,
        state: 'ready',
        connection: 'usb',
        avdName: '',
        api: '35',
        abi: 'arm64-v8a',
        ...extra,
      });
      window.isTauri = true;
      window.__devices = [
        device('primary-usb', '1', 'Primary phone'),
        device('192.168.1.10:5555', '2', 'Second phone', { connection: 'network' }),
        device('emulator-5554', '3', 'Emulator', {
          connection: 'emulator',
          avdName: 'Fixture_AVD',
          abi: 'x86_64',
        }),
      ];
      window.__operations = [];
      window.__commands = [];
      window.__callbacks = {};
      window.__listeners = {};
      window.__pickedPath = 'C:\\fixture.apk';
      window.__holdPicker = false;
      window.__rejectPrepare = false;
      window.__captureTasks = [];
      const liveLogs = new Set();
      let callbackId = 0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback(callback) {
          window.__callbacks[++callbackId] = callback;
          return callbackId;
        },
        unregisterCallback() {},
        invoke: async (command, args) => {
          window.__commands.push({ command, args });
          if (command === 'workspace_snapshot')
            return {
              settings: { sdkRoot: 'E:\\SDK', avdRoots: ['E:\\AVD'] },
              environment: { adb: 'fixture', warnings: [] },
              packages: [],
              tasks: [],
              server: 'fixture',
              serverEpoch: 1,
              freeBytes: 20 * 1024 ** 3,
              devices: structuredClone(window.__devices),
              avds: [
                {
                  id: 'fixture-avd',
                  name: 'Fixture_AVD',
                  displayName: 'Fixture emulator',
                  iniPath: 'E:\\AVD\\Fixture_AVD.ini',
                  dataPath: 'E:\\AVD\\Fixture_AVD.avd',
                  image: 'system-images;android-35;google_apis;x86_64',
                  api: '35',
                  abi: 'x86_64',
                  memory: '2048',
                  disk: '6G',
                  gpu: 'auto',
                  fingerprint: 'fixture',
                  status: 'ready',
                  error: '',
                  serial: 'emulator-5554',
                  owned: true,
                  config: {},
                },
              ],
            };
          if (command === 'window_state') return { maximized: false };
          if (command === 'log_read') {
            const { target, action, filter } = args;
            if (action === 'start') liveLogs.add(target.serial);
            if (action === 'stop') liveLogs.delete(target.serial);
            const active = liveLogs.has(target.serial);
            const rows = active
              ? Array.from({ length: 20 }, (_, i) => ({
                  raw: `11-20 10:42:18.${100 + i}  1264  1264 I FixtureApp: heartbeat ${i + 1} on ${target.model}`,
                }))
              : [];
            return {
              rows: filter.after ? [] : rows,
              latest: rows.length,
              oldest: active ? 1 : 0,
              dropped: 0,
              buffered: rows.length,
              active,
              status: active ? '正在采集' : '尚未开始采集',
              processStatus: '',
              pids: [],
            };
          }
          if (command === 'file_choose')
            return window.__holdPicker
              ? new Promise((resolve) => {
                  window.__resolvePicker = resolve;
                })
              : window.__pickedPath;
          if (command === 'apk_prepare') {
            if (window.__rejectPrepare) throw { message: 'Fixture invalid APK' };
            return {
              id: 'fixture-apk',
              path: args.path,
              fileName: 'fixture.apk',
              size: 1024 * 1024,
              sha256: 'fixture',
              package: 'com.example.fixture',
              version: '1.0',
              minSdk: '23',
              abis: [],
              warning: '',
            };
          }
          if (command === 'plan_create') {
            window.__operations.push(args.operation);
            const capture = args.operation.kind === 'capture';
            const recording = args.operation.kind === 'log_record';
            return {
              id: capture ? 'fixture-capture-plan' : 'fixture-plan',
              hash: 'fixture-hash',
              confirmationToken: 'fixture-token',
              title: recording ? 'Start file recording' : capture ? '截图' : 'Fixture install plan',
              operation: args.operation,
              effects: capture || recording ? [] : ['Install fixture.apk'],
              warnings: [],
              licenses: [],
              createdAt: Date.now(),
              settingsHash: 'fixture',
            };
          }
          if (command === 'task_submit' && args.planId === 'fixture-capture-plan') {
            return new Promise((resolve, reject) => {
              window.__finishCapture = (message) => {
                delete window.__finishCapture;
                if (message) {
                  reject({ message });
                  return;
                }
                const task = {
                  id: `fixture-capture-${window.__captureTasks.length + 1}`,
                  state: 'queued',
                  message: '',
                };
                window.__captureTasks.push(task);
                resolve(structuredClone(task));
              };
            });
          }
          if (command === 'task_list')
            return {
              tasks: structuredClone(
                window.__captureTasks.filter((task) => task.id === args.query.keyword),
              ),
            };
          if (command === 'artifact_copy' && args.preview)
            return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=';
          if (command === 'export_prepare')
            return {
              artifact: { id: 'fixture-logs', title: 'Current log view', bytes: 17 },
              text: 'Fixture log export',
              offset: 0,
              nextOffset: null,
              rows: 1,
              redacted: args.redacted,
              truncated: false,
              warning: '',
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
    const primary = page.locator('.device-row').filter({ hasText: 'Primary phone' });
    const secondary = page.locator('.device-row').filter({ hasText: 'Second phone' });
    const emulator = page.locator('.device-row').filter({ hasText: 'Fixture emulator' });
    const apkDialog = page.getByRole('dialog', { name: '安装 APK', exact: true });
    const details = page.locator('.detail-panel');
    const assertUnselected = async () => {
      assert.equal(await details.count(), 0);
      assert.equal(await page.locator('.device-row.selected').count(), 0);
      assert.equal(await page.locator('.log-panel').count(), 0);
    };
    const pickerSettled = () =>
      page.waitForFunction(() => !document.querySelector('.add-device-hint button').disabled);
    const cancelApk = () => apkDialog.getByRole('button', { name: '取消', exact: true }).click();
    const rescan = () => page.getByRole('button', { name: '重新扫描', exact: true }).click();
    const openQuick = async (row) => {
      await row.getByRole('button', { name: '安装 APK', exact: true }).click();
      await apkDialog.waitFor();
      await assertUnselected();
    };
    await primary.waitFor();

    // Both real devices and AVDs toggle selection with mouse, Enter and Space.
    for (const row of [primary, emulator]) {
      assert.equal(await row.getAttribute('title'), '选中设备');
      await row.click();
      await details.waitFor();
      assert.equal(await row.getAttribute('title'), '取消选中设备');
      await row.click();
      await assertUnselected();
      await row.focus();
      await row.press('Enter');
      await details.waitFor();
      await row.press('Enter');
      await assertUnselected();
      await row.press('Space');
      await details.waitFor();
      await row.press('Space');
      await assertUnselected();
    }
    await primary.click();
    await secondary.click();
    assert.equal(await details.locator('h3').textContent(), 'Second phone');
    assert.equal(await page.locator('.device-row.selected').count(), 1);
    await page.getByRole('button', { name: '关闭设备详情', exact: true }).click();
    await assertUnselected();

    // Cancelling the native chooser, including keyboard activation, opens no drawer.
    await page.evaluate(() => {
      window.__pickedPath = null;
    });
    await primary.getByRole('button', { name: '安装 APK', exact: true }).click();
    await pickerSettled();
    await assertUnselected();
    assert.equal(await apkDialog.count(), 0);
    await primary.getByRole('button', { name: '安装 APK', exact: true }).press('Enter');
    await pickerSettled();
    await assertUnselected();
    await page.evaluate(() => {
      window.__pickedPath = 'C:\\fixture.apk';
    });

    // Quick install uses its own target; changing it never selects a device row.
    await openQuick(primary);
    assert.equal(await apkDialog.getByLabel('目标设备').inputValue(), 'device:1:1:primary-usb');
    await apkDialog.getByLabel('目标设备').selectOption('device:1:2:192.168.1.10:5555');
    await assertUnselected();
    await apkDialog.getByRole('button', { name: '确认安装计划', exact: true }).click();
    await page.getByRole('dialog', { name: 'Fixture install plan' }).waitFor();
    assert.equal(await page.getByRole('dialog').count(), 1);
    assert.equal(
      await page.evaluate(() => window.__operations.at(-1).target.serial),
      '192.168.1.10:5555',
    );
    await page.keyboard.press('Escape');
    await apkDialog.waitFor();
    assert.equal(
      await apkDialog.getByLabel('目标设备').inputValue(),
      'device:1:2:192.168.1.10:5555',
    );
    await cancelApk();
    await assertUnselected();
    await openQuick(emulator);
    assert.equal(await apkDialog.getByLabel('目标设备').inputValue(), 'device:1:3:emulator-5554');
    await cancelApk();
    await page.getByRole('button', { name: '选择 APK', exact: true }).click();
    await apkDialog.waitFor();
    assert.equal(await apkDialog.getByLabel('目标设备').inputValue(), '');
    assert.equal(await apkDialog.getByRole('button', { name: '确认安装计划' }).isDisabled(), true);
    await cancelApk();

    // Preserve a pre-existing detail selection when installing to a different device.
    await primary.click();
    await secondary.getByRole('button', { name: '安装 APK', exact: true }).click();
    await apkDialog.waitFor();
    assert.equal(
      await apkDialog.getByLabel('目标设备').inputValue(),
      'device:1:2:192.168.1.10:5555',
    );
    await cancelApk();
    assert.equal(await details.locator('h3').textContent(), 'Primary phone');
    await primary.click();

    // A reconnect during file selection cannot silently retarget to the new transport.
    await page.evaluate(() => {
      window.__holdPicker = true;
    });
    await primary.getByRole('button', { name: '安装 APK', exact: true }).click();
    await page.waitForFunction(() => typeof window.__resolvePicker === 'function');
    await assertUnselected();
    await page.evaluate(() => {
      window.__devices[0].transportId = '99';
    });
    await rescan();
    await page.locator('[data-device-key="device:1:99:primary-usb"]').waitFor();
    await page.evaluate(() => {
      window.__holdPicker = false;
      window.__resolvePicker('C:\\fixture.apk');
    });
    await apkDialog.waitFor();
    await apkDialog.getByRole('status').waitFor();
    assert.equal(await apkDialog.getByLabel('目标设备').inputValue(), 'device:1:1:primary-usb');
    assert.equal(await apkDialog.getByRole('button', { name: '确认安装计划' }).isDisabled(), true);
    await apkDialog.getByLabel('目标设备').selectOption('device:1:99:primary-usb');
    await apkDialog.getByRole('button', { name: '确认安装计划' }).click();
    await page.getByRole('dialog', { name: 'Fixture install plan' }).waitFor();
    assert.equal(await page.evaluate(() => window.__operations.at(-1).target.transportId), '99');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await cancelApk();
    await assertUnselected();

    // Native drop callbacks must resolve against the latest workspace and selection.
    await page.waitForFunction(() => window.__listeners['tauri://drag-drop']);
    await primary.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      window.__callbacks[window.__listeners['tauri://drag-drop']]({
        event: 'tauri://drag-drop',
        id: 1,
        payload: {
          paths: ['C:\\dropped.apk'],
          position: {
            x: (rect.left + 12) * devicePixelRatio,
            y: (rect.top + 12) * devicePixelRatio,
          },
        },
      });
    });
    await apkDialog.waitFor();
    assert.equal(await apkDialog.getByLabel('目标设备').inputValue(), 'device:1:99:primary-usb');
    await assertUnselected();
    await cancelApk();

    // Parse errors also leave the workspace unselected; compact install stays usable.
    await page.evaluate(() => {
      window.__rejectPrepare = true;
    });
    await primary.getByRole('button', { name: '安装 APK', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Fixture invalid APK' }).waitFor();
    await assertUnselected();
    await page.evaluate(() => {
      window.__rejectPrepare = false;
    });
    await page.setViewportSize({ width: 480, height: 640 });
    await openQuick(primary);
    assert.equal(
      await apkDialog.evaluate((element) => element.scrollWidth > element.clientWidth),
      false,
    );
    const output = path.resolve(__dirname, '../.qa/app');
    fs.mkdirSync(output, { recursive: true });
    await page.screenshot({ path: path.join(output, 'apk-quick-install-compact.png') });
    await cancelApk();
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    assert.equal(await primary.getAttribute('title'), 'Select device');
    await primary.click();
    assert.equal(await primary.getAttribute('title'), 'Deselect device');
    await primary.click();
    await assertUnselected();
    assert.equal(
      await page.evaluate(() => window.__commands.some((c) => c.command === 'task_submit')),
      false,
    );

    // Screenshot submits immediately for the selected session and ignores a rapid second click.
    await secondary.click();
    const capture = details.getByRole('button', { name: 'Screenshot', exact: true });
    const waitForCaptureSubmission = () =>
      page.waitForFunction(() => typeof window.__finishCapture === 'function');
    await capture.dblclick();
    await waitForCaptureSubmission();
    assert.equal(await capture.isDisabled(), true);
    assert.equal(await page.getByRole('dialog').count(), 0);
    const { plans, submissions } = await page.evaluate(() => ({
      plans: window.__operations.filter(({ kind }) => kind === 'capture'),
      submissions: window.__commands.filter(({ command }) => command === 'task_submit'),
    }));
    assert.equal(plans.length, 1);
    assert.equal(plans[0].target.serial, '192.168.1.10:5555');
    assert.equal(plans[0].target.transportId, '2');
    assert.equal(plans[0].target.serverEpoch, 1);
    assert.equal(submissions.length, 1);
    assert.equal(submissions[0].args.planId, 'fixture-capture-plan');
    assert.equal(submissions[0].args.planHash, 'fixture-hash');
    assert.equal(submissions[0].args.confirmationToken, 'fixture-token');
    await page.evaluate(() => window.__finishCapture());
    await page.waitForFunction(() =>
      window.__commands.some(({ command }) => command === 'task_list'),
    );
    assert.equal(await page.getByRole('dialog').count(), 0);
    assert.equal(
      await page.evaluate(() =>
        window.__commands.some(({ command }) => command === 'artifact_copy'),
      ),
      false,
    );

    // The preview follows this capture even if the user has left the device page.
    const sidebar = page.locator('.sidebar');
    await sidebar.getByRole('button', { name: 'Settings', exact: true }).click();
    await details.waitFor({ state: 'detached' });
    await page.evaluate(() => {
      window.__captureTasks[0].state = 'succeeded';
    });
    const preview = page.getByRole('dialog', { name: 'Device screenshot', exact: true });
    await preview
      .getByRole('img', { name: 'Device screenshot preview' })
      .evaluate((img) => img.decode());
    assert.equal(await sidebar.locator('button.active').getAttribute('aria-label'), 'Settings');
    assert.deepEqual(
      await page.evaluate(() =>
        window.__commands
          .filter(({ command }) => command === 'artifact_copy')
          .map(({ args }) => args),
      ),
      [{ taskId: 'fixture-capture-1', preview: true }],
    );
    await preview.getByRole('button', { name: 'Close panel' }).click();
    await preview.waitFor({ state: 'detached' });
    await sidebar.getByRole('button', { name: 'Device Workbench', exact: true }).click();

    await capture.click();
    await waitForCaptureSubmission();
    await page.evaluate(() => {
      window.__finishCapture();
      Object.assign(window.__captureTasks[1], {
        state: 'failed',
        message: 'Fixture device disconnected',
      });
    });
    await page.getByRole('alert').filter({ hasText: 'Fixture device disconnected' }).waitFor();
    assert.equal(await page.getByRole('dialog').count(), 0);

    await capture.click();
    await waitForCaptureSubmission();
    await page.evaluate(() => window.__finishCapture('Fixture capture submission failed'));
    await page
      .getByRole('alert')
      .filter({ hasText: 'Fixture capture submission failed' })
      .waitFor();
    await page.waitForFunction(() => !document.querySelector('.detail-actions [aria-busy]'));
    assert.equal(await capture.isEnabled(), true);
    assert.equal(await page.getByRole('dialog').count(), 0);

    // Logs have their own navigation and target, independent of the selected device row.
    await page.getByRole('button', { name: 'Dismiss error', exact: true }).click();
    assert.equal(await page.locator('.log-panel').count(), 0);
    assert.equal(await details.getByRole('button', { name: 'View logs', exact: true }).count(), 0);
    const logs = sidebar.getByRole('button', { name: 'Live Logs', exact: true });
    await logs.click();
    await page.getByRole('main', { name: 'Live Logs', exact: true }).waitFor();
    assert.equal(await logs.getAttribute('aria-current'), 'page');
    const logTarget = page.getByLabel('Target device', { exact: true });
    assert.equal(await logTarget.inputValue(), 'device:1:2:192.168.1.10:5555');
    await logTarget.selectOption('device:1:99:primary-usb');
    await page.getByRole('button', { name: 'Start viewing', exact: true }).click();
    await page.locator('.log-lines').filter({ hasText: 'heartbeat 1 on Primary phone' }).waitFor();
    await page.getByLabel('Log tag', { exact: true }).fill('FixtureApp');
    await page.waitForFunction(() =>
      window.__commands.some(
        ({ command, args }) =>
          command === 'log_read' && args.action === 'read' && args.filter.tag === 'FixtureApp',
      ),
    );
    await page.getByRole('button', { name: 'Pause view', exact: true }).click();
    await page.getByRole('status').filter({ hasText: 'View paused' }).waitFor();
    await page.getByRole('button', { name: 'Resume view', exact: true }).click();
    await page
      .getByRole('button', { name: 'Preview and export current view', exact: true })
      .click();
    const logExport = page.getByRole('dialog', { name: 'Export preview', exact: true });
    await logExport.getByText('Fixture log export', { exact: true }).waitFor();
    assert.deepEqual(
      await page
        .evaluate(
          () => window.__commands.findLast(({ command }) => command === 'export_prepare').args,
        )
        .then(({ target, filter, redacted }) => [
          target.serial,
          filter.tag,
          filter.before,
          redacted,
        ]),
      ['primary-usb', 'FixtureApp', 20, true],
    );
    await logExport.getByRole('button', { name: 'Close panel', exact: true }).click();
    await page.getByRole('button', { name: 'Switch interface language', exact: true }).click();
    await page.getByRole('main', { name: '实时日志', exact: true }).waitFor();
    await page.screenshot({ path: path.join(output, 'live-logs-desktop.png') });
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await page.getByRole('main', { name: 'Live Logs', exact: true }).waitFor();
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth), 480);
    await page.screenshot({ path: path.join(output, 'live-logs-compact.png'), fullPage: true });
    await page.getByRole('button', { name: 'Record to file…', exact: true }).click();
    await page.getByLabel('Recording quota', { exact: true }).fill('8');
    await page.getByRole('button', { name: 'Review recording plan', exact: true }).click();
    await page.getByRole('dialog', { name: 'Start file recording', exact: true }).waitFor();
    assert.deepEqual(
      await page.evaluate(() => {
        const { kind, target, args } = window.__operations.findLast(
          ({ kind }) => kind === 'log_record',
        );
        return [kind, target.serial, args.quotaMiB, JSON.parse(args.filter).tag];
      }),
      ['log_record', 'primary-usb', '8', 'FixtureApp'],
    );
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Clear view', exact: true }).click();
    await page.getByText('No logs match the current filter.', { exact: true }).waitFor();
    await sidebar.getByRole('button', { name: 'Device Workbench', exact: true }).click();
    await page.waitForFunction(() =>
      window.__commands.some(
        ({ command, args }) =>
          command === 'log_read' && args.action === 'stop' && args.target.serial === 'primary-usb',
      ),
    );
    assert.equal(await details.locator('h3').textContent(), 'Second phone');
    await logs.click();
    assert.equal(await logTarget.inputValue(), 'device:1:99:primary-usb');
    await page.evaluate(() => {
      window.__devices[0].transportId = '100';
      window.__callbacks[window.__listeners['workspace-changed']]({ payload: null });
    });
    await page.getByText('The target device is unavailable.', { exact: false }).waitFor();
    assert.equal(await logTarget.inputValue(), '');
    assert.equal(await page.getByRole('button', { name: 'Start viewing', exact: true }).count(), 0);
    await page.reload();
    await page.getByRole('main', { name: 'Live Logs', exact: true }).waitFor();
    await page.getByText('Select a log target', { exact: true }).waitFor();
    await page.waitForFunction(() => window.__listeners['workspace-changed']);
    await page.evaluate(() => {
      window.__devices = [];
      window.__callbacks[window.__listeners['workspace-changed']]({ payload: null });
    });
    await page.getByRole('heading', { name: 'No available devices', exact: true }).waitFor();
    assert.equal(await logTarget.isDisabled(), true);
    await page.getByRole('button', { name: 'Go to Device Workbench', exact: true }).click();
    await page.getByRole('main', { name: 'Device Workbench', exact: true }).waitFor();

    assert.deepEqual(errors, []);
    console.log(
      'Device UI: selection, installation, reconnect, native drop, screenshots, and independent live logs passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
