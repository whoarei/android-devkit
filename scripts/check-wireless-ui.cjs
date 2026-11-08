// Isolated wireless pairing UI: IPC doubles only; no phones, LAN traffic, or camera access.
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
      window.isTauri = true;
      window.__services = [];
      window.__tasks = [];
      window.__calls = [];
      window.__callbacks = {};
      window.__listeners = {};
      window.__discoveryError = false;
      window.__holdQr = false;
      let counter = 0;
      const makeTask = (kind) => ({
        id: `task-${++counter}`,
        requestId: 'fixture',
        title: kind === 'wireless_qr_pair' ? '扫码配对设备' : '无线设备配对',
        operation: { kind, args: {}, target: null },
        state: 'running',
        phase: kind === 'wireless_qr_pair' ? 'waiting_scan' : 'pairing',
        message:
          kind === 'wireless_qr_pair' ? '请使用手机无线调试中的扫码入口扫描二维码' : '正在配对设备',
        cancellable: true,
        cancelRequested: false,
        log: [],
        confirmedLicenses: [],
        artifact: '',
        licensePrompt: '',
        createdAt: 1,
        updatedAt: 1,
        progress: null,
      });
      window.__emitWorkspace = () => {
        const callback = window.__callbacks[window.__listeners['workspace-changed']];
        if (callback) callback({ event: 'workspace-changed', id: 1, payload: 1 });
      };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback(callback) {
          window.__callbacks[++counter] = callback;
          return counter;
        },
        unregisterCallback() {},
        invoke: async (command, args) => {
          window.__calls.push({ command, args });
          if (command === 'workspace_snapshot')
            return {
              settings: { sdkRoot: 'E:\\SDK' },
              environment: { adb: 'fixture', warnings: [] },
              packages: [],
              avds: [],
              devices: [],
              tasks: structuredClone(window.__tasks),
              server: 'fixture',
              serverEpoch: 1,
              freeBytes: 1024 ** 3,
            };
          if (command === 'window_state') return { maximized: false };
          if (command === 'wireless_discover') {
            if (window.__discoveryError)
              throw { message: '无法自动发现设备，请检查 ADB 的 mDNS 支持或使用手动输入' };
            return structuredClone(window.__services);
          }
          if (command === 'wireless_pair_start') {
            const task = makeTask('wireless_pair');
            window.__tasks.push(task);
            return structuredClone(task);
          }
          if (command === 'wireless_qr_start') {
            const create = () => {
              const task = makeTask('wireless_qr_pair');
              window.__tasks.push(task);
              return {
                task: structuredClone(task),
                expiresAt: Math.floor(Date.now() / 1000) + 120,
                image:
                  'data:image/svg+xml;base64,' +
                  btoa(
                    '<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240"><rect width="240" height="240" fill="white"/><path d="M20 20h60v60H20zM160 20h60v60h-60zM20 160h60v60H20zM110 110h30v30h-30z" fill="black"/></svg>',
                  ),
              };
            };
            if (window.__holdQr)
              return new Promise((resolve) => {
                window.__resolveQr = () => resolve(create());
              });
            return create();
          }
          if (command === 'task_cancel') {
            const task = window.__tasks.find((item) => item.id === args.taskId);
            if (task) {
              task.state = 'cancelled';
              task.cancelRequested = true;
              task.message = '已停止连接流程；已建立的配对授权请在手机中检查';
            }
            window.__emitWorkspace();
            return;
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
    const open = () => page.getByRole('button', { name: '连接设备', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '连接设备', exact: true });
    await open();
    assert.equal(
      await dialog
        .getByRole('button', { name: '扫码配对', exact: true })
        .getAttribute('aria-pressed'),
      'true',
    );
    await dialog.getByRole('button', { name: '配对码', exact: true }).click();
    await dialog.getByText('正在查找设备，请打开手机的配对码页面…').waitFor();
    assert.equal(await dialog.getByRole('button', { name: '配对并连接' }).isDisabled(), true);
    await page.evaluate(() => {
      window.__services = [
        {
          name: 'adb-phone-one',
          serviceType: '_adb-tls-pairing._tcp',
          address: '192.0.2.10:37123',
        },
        {
          name: 'adb-phone-two',
          serviceType: '_adb-tls-pairing._tcp',
          address: '192.0.2.11:37124',
        },
      ];
    });
    await dialog.getByRole('button', { name: '重新扫描' }).click();
    await dialog.getByRole('radio').first().waitFor();
    assert.equal(await dialog.getByRole('radio', { checked: true }).count(), 0);
    await dialog.getByRole('radio').nth(1).check();
    await dialog.getByLabel('6 位配对码', { exact: true }).fill('123');
    assert.equal(await dialog.getByRole('button', { name: '配对并连接' }).isDisabled(), true);
    await dialog.getByLabel('6 位配对码', { exact: true }).fill('123456');
    // A selected service disappearing invalidates both selection and its code.
    await page.evaluate(() => {
      window.__services.pop();
    });
    await dialog.getByRole('button', { name: '重新扫描' }).click();
    await dialog.getByRole('alert').filter({ hasText: '所选设备已停止配对' }).waitFor();
    assert.equal(await dialog.getByLabel('6 位配对码', { exact: true }).inputValue(), '');
    assert.equal(await dialog.getByRole('button', { name: '配对并连接' }).isDisabled(), true);
    await dialog.getByRole('radio').first().check();
    await dialog.getByLabel('6 位配对码', { exact: true }).fill('654321');
    await dialog.getByRole('button', { name: '配对并连接' }).click();
    await dialog.getByText('正在配对设备', { exact: true }).waitFor();
    const pair = await page.evaluate(() =>
      window.__calls.find((call) => call.command === 'wireless_pair_start'),
    );
    assert.equal(pair.args.service.address, '192.0.2.10:37123');
    assert.equal(pair.args.code, '654321');
    assert.equal(await dialog.getByLabel('6 位配对码', { exact: true }).inputValue(), '');
    assert.equal(
      await dialog.getByRole('button', { name: '扫码配对', exact: true }).isDisabled(),
      true,
    );
    await page.evaluate(() => {
      Object.assign(window.__tasks.at(-1), {
        phase: 'connecting',
        message: '配对成功，正在发现并验证无线连接',
      });
      window.__emitWorkspace();
    });
    await dialog.getByText('配对成功，正在发现并验证无线连接', { exact: true }).waitFor();
    assert.equal(await dialog.isVisible(), true);
    await page.evaluate(() => {
      Object.assign(window.__tasks.at(-1), {
        state: 'failed',
        message: 'PAIRED_NOT_CONNECTED：配对成功，但无线连接未就绪',
      });
      window.__emitWorkspace();
    });
    await dialog
      .getByText('PAIRED_NOT_CONNECTED：配对成功，但无线连接未就绪', { exact: true })
      .waitFor();
    assert.equal(await dialog.isVisible(), true);
    await dialog.getByRole('radio').first().check();
    await dialog.getByLabel('6 位配对码', { exact: true }).fill('654321');
    await dialog.getByRole('button', { name: '配对并连接' }).click();
    await dialog.getByText('正在配对设备', { exact: true }).waitFor();
    const codeTaskId = await page.evaluate(() => window.__tasks.at(-1).id);
    await page.evaluate(() => {
      Object.assign(window.__tasks.at(-1), {
        state: 'succeeded',
        phase: 'complete',
        message: '配对成功，无线连接已验证，设备可用',
      });
      window.__emitWorkspace();
    });
    await dialog.waitFor({ state: 'detached' });
    assert.equal(
      await page.evaluate(
        (id) =>
          window.__calls.some((call) => call.command === 'task_cancel' && call.args.taskId === id),
        codeTaskId,
      ),
      false,
    );
    // Reopening after a successful old task must keep the new panel open.
    await open();
    await dialog.getByRole('button', { name: '扫码配对', exact: true }).waitFor();
    await dialog.getByRole('button', { name: '生成配对二维码', exact: true }).dblclick();
    await dialog.getByRole('img', { name: '无线调试配对二维码' }).waitFor();
    assert.equal(
      await page.evaluate(
        () => window.__calls.filter((call) => call.command === 'wireless_qr_start').length,
      ),
      1,
    );
    const output = path.resolve(__dirname, '../.qa/wireless');
    fs.mkdirSync(output, { recursive: true });
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(
      await dialog.evaluate((element) => element.scrollWidth > element.clientWidth),
      false,
    );
    await page.screenshot({ path: path.join(output, 'qr-compact.png') });
    const qrTaskId = await page.evaluate(() => window.__tasks.at(-1).id);
    await page.evaluate(() => {
      Object.assign(window.__tasks.at(-1), {
        state: 'succeeded',
        phase: 'complete',
        message: '配对成功，无线连接已验证，设备可用',
      });
      window.__emitWorkspace();
    });
    await dialog.waitFor({ state: 'detached' });
    assert.equal(
      await page.evaluate(
        (id) =>
          window.__calls.some((call) => call.command === 'task_cancel' && call.args.taskId === id),
        qrTaskId,
      ),
      false,
    );
    await open();
    await dialog.getByRole('button', { name: '生成配对二维码', exact: true }).click();
    await dialog.getByRole('img', { name: '无线调试配对二维码' }).waitFor();
    await dialog.getByRole('button', { name: '停止并关闭', exact: true }).click();
    await dialog.waitFor({ state: 'detached' });
    assert.equal(await page.evaluate(() => window.__tasks.at(-1).state), 'cancelled');
    // Closing before the start response arrives must cancel the late-created task.
    await open();
    await page.evaluate(() => {
      window.__holdQr = true;
    });
    await dialog.getByRole('button', { name: '生成配对二维码', exact: true }).click();
    await dialog.getByRole('button', { name: '关闭连接面板', exact: true }).click();
    await page.evaluate(() => window.__resolveQr());
    await page.waitForFunction(() => window.__tasks.at(-1).state === 'cancelled');
    await page.evaluate(() => {
      window.__holdQr = false;
      window.__discoveryError = true;
    });
    await open();
    await dialog.getByRole('button', { name: '配对码', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '无法自动发现设备' }).waitFor();
    await dialog.getByRole('button', { name: '手动连接', exact: true }).click();
    await dialog.getByLabel('设备 IP 地址', { exact: true }).fill('192.0.2.10');
    assert.equal(await dialog.getByLabel('配对端口', { exact: true }).count(), 1);
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await page.getByRole('button', { name: 'Connect device', exact: true }).click();
    const english = page.getByRole('dialog', { name: 'Connect device', exact: true });
    await english.getByRole('button', { name: 'Pairing code', exact: true }).click();
    await english.getByText('Discovery is unavailable. You can connect manually.').waitFor();
    assert.equal(
      await english.evaluate((element) => /[\u3400-\u9fff]/.test(element.innerText)),
      false,
    );
    await page.screenshot({ path: path.join(output, 'code-en-compact.png') });
    assert.deepEqual(errors, []);
    console.log(
      'Wireless UI: discovery, multiple phones, stale selection, code clearing, QR/code success auto-close without cancellation, failure retention, reopen, QR submission deduplication, close cancellation, late response, fallback, English and 480px layout passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
