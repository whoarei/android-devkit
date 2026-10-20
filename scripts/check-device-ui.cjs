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
            return {
              id: 'fixture-plan',
              hash: 'fixture',
              confirmationToken: 'fixture',
              title: 'Fixture install plan',
              operation: args.operation,
              effects: ['Install fixture.apk'],
              warnings: [],
              licenses: [],
              createdAt: Date.now(),
              settingsHash: 'fixture',
            };
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
    const primary = page.locator('.device-row').filter({ hasText: 'Primary phone' });
    const secondary = page.locator('.device-row').filter({ hasText: 'Second phone' });
    const emulator = page.locator('.device-row').filter({ hasText: 'Fixture emulator' });
    const apkDialog = page.getByRole('dialog', { name: '安装 APK', exact: true });
    const details = page.locator('.detail-panel');
    const assertUnselected = async () => {
      assert.equal(await details.count(), 0);
      assert.equal(await page.locator('.device-row.selected').count(), 0);
      assert.equal(await page.locator('#logs').count(), 0);
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
    assert.deepEqual(errors, []);
    console.log(
      'Device UI: selection toggle, keyboard actions, quick APK install/cancel, independent targets, reconnect, native drop, errors and compact layout passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
