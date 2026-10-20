// Isolated UI fixtures: no real device commands or user browser sessions.
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
      window.isTauri = true;
      window.__operations = [];
      window.__rejectPlan = false;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: () => 1,
        unregisterCallback() {},
        invoke: async (command, args) => {
          if (command === 'workspace_snapshot')
            return {
              settings: { sdkRoot: 'E:\\Android\\sdk' },
              environment: { adb: 'fixture' },
              packages: [],
              avds: [],
              tasks: [],
              server: 'UI fixture',
              serverEpoch: 1,
              freeBytes: 1024 ** 3,
              devices: [
                {
                  serial: '192.168.1.10:5555',
                  transportId: '2',
                  serverEpoch: 1,
                  model: 'Network fixture',
                  state: 'unauthorized',
                  connection: 'network',
                  avdName: '',
                  api: '',
                  abi: '',
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
          if (command === 'plan_create') {
            window.__operations.push(args.operation);
            if (window.__rejectPlan)
              throw { code: 'INVALID_ADDRESS', message: '请输入有效 IP 和 1～65535 端口' };
            const titles = {
              network_connect: '连接网络 ADB 设备',
              network_disconnect: '断开网络 ADB 连接',
              wireless_pair: '无线设备配对',
              wireless_connect: '连接无线设备',
            };
            return {
              id: 'fixture',
              hash: 'fixture',
              confirmationToken: 'fixture',
              title: titles[args.operation.kind],
              operation: args.operation,
              effects: [args.operation.args.address],
              warnings: [],
              licenses: [],
            };
          }
          if (command.startsWith('plugin:event|')) return 1;
          return null;
        },
      };
    });
    const url = process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327';
    await page.goto(url);
    await page.getByRole('heading', { name: '设备工作台', exact: true }).waitFor();
    const row = page.locator('.device-row').filter({ hasText: 'Network fixture' });
    await row.getByText('网络 ADB', { exact: false }).waitFor();
    await row.click();
    await page.getByText('请解锁设备并允许此电脑进行 ADB 调试；授权后将自动更新。').waitFor();
    assert.equal(await row.getByRole('button', { name: '安装 APK' }).isDisabled(), true);
    await page.getByRole('button', { name: '断开网络连接', exact: true }).click();
    await page.getByRole('dialog', { name: '断开网络 ADB 连接', exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.__operations.at(-1)), {
      kind: 'network_disconnect',
      args: { address: '192.168.1.10:5555' },
      target: null,
    });
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await page.getByRole('button', { name: '连接设备', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '连接设备', exact: true });
    await dialog.getByLabel('设备 IP 地址', { exact: true }).fill(' 192.168.1.10 ');
    await dialog.getByLabel('配对端口', { exact: true }).fill('37123');
    await dialog.getByLabel('6 位配对码', { exact: true }).fill('123456');
    await dialog.getByRole('button', { name: '配对', exact: true }).click();
    await page.getByRole('dialog', { name: '无线设备配对', exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.__operations.at(-1).args), {
      address: '192.168.1.10:37123',
      code: '123456',
    });
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await dialog.getByRole('button', { name: '网络 ADB', exact: true }).click();
    assert.equal(await dialog.getByLabel('连接端口', { exact: true }).inputValue(), '5555');
    assert.equal(await dialog.getByLabel('6 位配对码', { exact: true }).count(), 0);
    await dialog.getByRole('button', { name: '连接并验证', exact: true }).click();
    await page.getByRole('dialog', { name: '连接网络 ADB 设备', exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.__operations.at(-1)), {
      kind: 'network_connect',
      args: { address: '192.168.1.10:5555' },
      target: null,
    });
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await dialog.getByLabel('连接端口', { exact: true }).fill('7777');
    await dialog.getByLabel('设备 IP 地址', { exact: true }).fill('2001:db8::1');
    await dialog.getByRole('button', { name: '连接并验证', exact: true }).click();
    await page.getByRole('dialog', { name: '连接网络 ADB 设备', exact: true }).waitFor();
    assert.equal(
      await page.evaluate(() => window.__operations.at(-1).args.address),
      '[2001:db8::1]:7777',
    );
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await page.evaluate(() => {
      window.__rejectPlan = true;
    });
    await dialog.getByLabel('连接端口', { exact: true }).fill('0');
    await dialog.getByRole('button', { name: '连接并验证', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '1～65535' }).waitFor();
    await dialog.getByRole('button', { name: '2. 已配对连接', exact: true }).click();
    assert.equal(await dialog.getByLabel('连接端口', { exact: true }).inputValue(), '');
    assert.equal(await dialog.getByRole('alert').count(), 0);
    await page.evaluate(() => {
      window.__rejectPlan = false;
    });
    await dialog.getByLabel('连接端口', { exact: true }).fill('40001');
    await dialog.getByRole('button', { name: '连接并验证', exact: true }).click();
    await page.getByRole('dialog', { name: '连接无线设备', exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.__operations.at(-1).args), {
      address: '[2001:db8::1]:40001',
    });
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await dialog.getByRole('button', { name: '网络 ADB', exact: true }).click();
    await dialog.getByLabel('设备 IP 地址', { exact: true }).fill('192.168.1.10');
    await dialog.getByLabel('连接端口', { exact: true }).fill('5555');
    const output = path.resolve(__dirname, '../.qa/app');
    fs.mkdirSync(output, { recursive: true });
    await page.screenshot({ path: path.join(output, 'network-adb-form.png') });
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(
      await dialog.evaluate((element) => element.scrollWidth > element.clientWidth),
      false,
    );
    await page.screenshot({ path: path.join(output, 'network-adb-compact.png') });
    assert.deepEqual(errors, []);
    console.log(
      'Network ADB UI: pairing, direct IPv4/IPv6, port isolation, errors, unauthorized state, endpoint disconnect and compact layout passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
