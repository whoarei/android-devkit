// Isolated P0 fixtures: no SDK processes, clipboard, or user data are accessed.
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
      window.__commands = [];
      window.__operations = [];
      window.__copied = [];
      window.__scanFailure = false;
      window.__copyFailure = false;
      window.__callback = 0;
      window.__callbacks = {};
      window.__listeners = {};
      const settings = {
        sdkRoot: 'E:\\SDK',
        avdRoots: ['E:\\AVD'],
        javaPath: '',
        proxy: '',
        bootTimeoutSecs: 180,
        trayEnabled: false,
        sdkSites: [],
        sdkChannel: 0,
        sdkDownload: { kind: 'google', name: '', url: '', verifyOfficial: true },
      };
      window.__fixture = {
        setupRequired: !sessionStorage.getItem('p0-configured'),
        settings,
        environment: { path: settings.sdkRoot, adb: 'fixture', warnings: [] },
        packages: [],
        tasks: [],
        server: 'fixture',
        serverEpoch: 1,
        freeBytes: 20 * 1024 ** 3,
        avds: [
          {
            id: 'avd-a',
            name: 'AVD_A',
            displayName: 'Booting emulator',
            iniPath: 'E:\\AVD\\A.ini',
            dataPath: 'E:\\AVD\\A.avd',
            image: 'system-images;android-35;default;x86_64',
            api: '35',
            abi: 'x86_64',
            memory: '2048',
            disk: '6G',
            gpu: 'auto',
            fingerprint: 'fixture',
            status: 'booting',
            error: '',
            serial: 'emulator-5554',
            owned: true,
            config: {},
          },
        ],
        devices: [
          {
            serial: 'USB_A',
            transportId: '1',
            serverEpoch: 1,
            model: 'Phone A',
            connection: 'usb',
            state: 'ready',
            avdName: '',
            api: '35',
            abi: 'arm64-v8a',
          },
          {
            serial: 'emulator-5554',
            transportId: '2',
            serverEpoch: 1,
            model: 'Emulator A',
            connection: 'emulator',
            state: 'booting',
            avdName: 'AVD_A',
            api: '35',
            abi: 'x86_64',
          },
        ],
      };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback(callback) {
          const id = ++window.__callback;
          window.__callbacks[id] = callback;
          return id;
        },
        unregisterCallback() {},
        invoke: async (command, args) => {
          window.__commands.push({ command, args });
          if (command === 'workspace_snapshot') return structuredClone(window.__fixture);
          if (command === 'environment_scan') {
            if (window.__scanFailure) throw { message: 'Scan fixture failed' };
            return [
              {
                path: 'C:\\Users\\fixture\\Android SDK',
                source: 'ANDROID_HOME',
                adb: 'C:\\Users\\fixture\\Android SDK\\platform-tools\\adb.exe',
                emulator: '',
                commandTools: '',
                java: '',
                writable: true,
                freeBytes: 20 * 1024 ** 3,
                warnings: [],
              },
              {
                path: 'D:\\Other SDK',
                source: '常用目录',
                adb: '',
                emulator: '',
                commandTools: '',
                java: '',
                writable: false,
                freeBytes: 0,
                warnings: [],
              },
            ];
          }
          if (command === 'settings_save') {
            sessionStorage.setItem('p0-configured', 'true');
            window.__fixture.settings = structuredClone(args.settings);
            window.__fixture.setupRequired = false;
            return null;
          }
          if (command === 'window_state') return { maximized: false };
          if (command === 'avd_usage')
            return { bytes: 1536 * 1024 ** 2, files: 5, complete: false };
          if (command === 'text_copy') {
            if (window.__copyFailure) throw { message: 'Clipboard fixture busy' };
            window.__copied.push(args.text);
            return null;
          }
          if (command === 'environment_summary')
            return args.redacted
              ? 'SDK [USER_HOME]\\Android SDK\nDevice [DEVICE_1]'
              : 'SDK C:\\Users\\fixture\\Android SDK\nDevice USB_A';
          if (command === 'diagnostics')
            return { acceleration: 'fixture', sdkRoot: window.__fixture.settings.sdkRoot };
          if (command === 'file_choose') return args.kind === 'apk' ? 'C:\\fixture.apk' : 'E:\\SDK';
          if (command === 'apk_prepare')
            return {
              id: 'apk-a',
              path: args.path,
              fileName: 'fixture.apk',
              size: 1024,
              sha256: 'fixture',
              package: 'com.example.fixture',
              version: '1.0',
              minSdk: '23',
              abis: [],
              warning: '',
            };
          if (command === 'plan_create') {
            window.__operations.push(args.operation);
            return {
              id: 'plan-a',
              hash: 'fixture',
              confirmationToken: 'fixture',
              operation: args.operation,
              title:
                args.operation.kind === 'avd_wait'
                  ? '继续等待系统就绪'
                  : args.operation.kind === 'apk_install_wait'
                    ? '就绪后安装 APK'
                    : '安装 APK',
              effects: ['fixture'],
              warnings: [],
              licenses: [],
              createdAt: 1,
              settingsHash: 'fixture',
            };
          }
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
          if (command === 'plugin:event|listen') {
            window.__listeners[args.event] = args.handler;
            return args.handler;
          }
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    const setup = page.getByRole('dialog', { name: '选择或创建 SDK 环境', exact: true });
    await setup.waitFor();
    await setup.getByText('C:\\Users\\fixture\\Android SDK', { exact: true }).click();
    assert.match(await setup.textContent(), /20.00 GiB/);
    assert.match(await setup.textContent(), /不可写/);
    // Scan failure remains visible and can be retried without changing the selection.
    await page.evaluate(() => {
      window.__scanFailure = true;
    });
    await setup.getByRole('button', { name: '重新发现环境', exact: true }).click();
    await setup.getByRole('alert').filter({ hasText: 'Scan fixture failed' }).waitFor();
    await page.evaluate(() => {
      window.__scanFailure = false;
    });
    await setup.getByRole('button', { name: '重新发现环境', exact: true }).click();
    await setup.getByRole('button', { name: '应用所选环境', exact: true }).click();
    await setup.waitFor({ state: 'detached' });
    assert.equal(
      await page.evaluate(() => window.__fixture.settings.sdkRoot),
      'C:\\Users\\fixture\\Android SDK',
    );
    assert.equal(
      await page.evaluate(() => window.__operations.length),
      0,
      'Reusing an SDK never submits an install',
    );
    const phone = page.locator('.device-row').filter({ hasText: 'Phone A' });
    const emulator = page.locator('.device-row').filter({ hasText: 'Booting emulator' });
    const details = page.locator('.detail-panel');
    await phone.focus();
    await phone.press('ArrowDown');
    assert.equal(await emulator.evaluate((node) => document.activeElement === node), true);
    await details.getByText('1.50 GiB（仅已统计部分）', { exact: false }).waitFor();
    await emulator.press('Home');
    assert.equal(await phone.evaluate((node) => document.activeElement === node), true);
    await details.getByRole('button', { name: '复制设备标识', exact: true }).click();
    assert.deepEqual(await page.evaluate(() => window.__copied), ['USB_A']);
    await details.getByRole('button', { name: '设为默认目标', exact: true }).click();
    assert.equal(
      await page.evaluate(() => window.__preferences.defaultTarget),
      'device:usb:USB_A',
    );
    await phone.press('Escape');
    await page.getByRole('button', { name: '选择 APK', exact: true }).click();
    const apk = page.getByRole('dialog', { name: '安装 APK', exact: true });
    await apk.waitFor();
    assert.equal(
      await apk.getByLabel('目标设备', { exact: true }).inputValue(),
      'device:1:1:USB_A',
    );
    await apk.getByRole('button', { name: '取消', exact: true }).click();
    await page.reload();
    await page.getByRole('button', { name: '选择 APK', exact: true }).click();
    await apk.waitFor();
    assert.equal(
      await apk.getByLabel('目标设备', { exact: true }).inputValue(),
      'device:1:1:USB_A',
      'Default preference survives a reload',
    );
    await apk.getByRole('button', { name: '取消', exact: true }).click();
    // An unavailable preferred device never selects an unrelated online device.
    await page.evaluate(() => {
      window.__fixture.devices = window.__fixture.devices.filter(
        (device) => device.serial !== 'USB_A',
      );
    });
    await page.getByRole('button', { name: '重新扫描', exact: true }).click();
    await phone.waitFor({ state: 'detached' });
    await page.getByRole('button', { name: '选择 APK', exact: true }).click();
    await apk.waitFor();
    assert.equal(await apk.getByLabel('目标设备', { exact: true }).inputValue(), '');
    await apk.getByLabel('目标设备', { exact: true }).selectOption('avdwait:avd-a');
    await page.evaluate(() => {
      window.__fixture.devices.find((device) => device.serial === 'emulator-5554').transportId =
        '22';
      const id = window.__listeners['workspace-changed'];
      window.__callbacks[id]({ event: 'workspace-changed', id: 1, payload: 1 });
    });
    await apk.getByRole('button', { name: '确认安装计划', exact: true }).click();
    const frozenPlan = page.getByRole('dialog', { name: '就绪后安装 APK', exact: true });
    await frozenPlan.waitFor();
    assert.equal(
      await page.evaluate(() => window.__operations.at(-1).target.transportId),
      '2',
      'Selecting a waiting AVD freezes its existing connection',
    );
    await frozenPlan.getByRole('button', { name: '返回修改', exact: true }).click();
    await apk.getByRole('button', { name: '取消', exact: true }).click();
    await emulator.click();
    await details.getByRole('button', { name: '复制完整路径', exact: true }).click();
    assert.equal(await page.evaluate(() => window.__copied.at(-1)), 'E:\\AVD\\A.avd');
    await details.getByRole('button', { name: '继续等待系统就绪', exact: true }).click();
    const waitPlan = page.getByRole('dialog', { name: '继续等待系统就绪', exact: true });
    await waitPlan.waitFor();
    assert.equal(await page.evaluate(() => window.__operations.at(-1).kind), 'avd_wait');
    await waitPlan.getByRole('button', { name: '返回修改', exact: true }).click();
    await details.getByRole('button', { name: '安装 APK', exact: true }).click();
    await apk.waitFor();
    assert.equal(await apk.getByLabel('目标设备', { exact: true }).inputValue(), 'avdwait:avd-a');
    await apk.getByRole('button', { name: '确认安装计划', exact: true }).click();
    const installPlan = page.getByRole('dialog', { name: '就绪后安装 APK', exact: true });
    await installPlan.waitFor();
    assert.equal(await page.evaluate(() => window.__operations.at(-1).args.avdId), 'avd-a');
    assert.equal(
      await page.evaluate(() => window.__operations.at(-1).target.serial),
      'emulator-5554',
    );
    await installPlan.getByRole('button', { name: '返回修改', exact: true }).click();
    await apk.getByRole('button', { name: '取消', exact: true }).click();
    await page.getByRole('button', { name: '环境诊断', exact: true }).click();
    await page.getByRole('button', { name: '预览并复制环境摘要', exact: true }).click();
    const summary = page.getByRole('dialog', { name: '环境摘要预览', exact: true });
    await summary.waitFor();
    assert.match(await summary.textContent(), /\[USER_HOME\]/);
    assert.equal((await summary.textContent()).includes('USB_A'), false);
    await page.evaluate(() => {
      window.__copyFailure = true;
    });
    await summary.getByRole('button', { name: '复制环境摘要', exact: true }).click();
    await page.getByText('Clipboard fixture busy', { exact: true }).waitFor();
    await page.evaluate(() => {
      window.__copyFailure = false;
    });
    await summary.getByRole('button', { name: '复制环境摘要', exact: true }).click();
    assert.match(await page.evaluate(() => window.__copied.at(-1)), /\[DEVICE_1\]/);
    assert.equal(
      await page.evaluate(
        () => window.__commands.filter((entry) => entry.command === 'export_prepare').length,
      ),
      0,
      'Summary copying creates no unsaved export',
    );
    await summary.getByRole('button', { name: '关闭面板', exact: true }).click();
    await page.getByLabel('包含原始标识', { exact: true }).check();
    await page.getByRole('button', { name: '预览并复制环境摘要', exact: true }).click();
    await summary.waitFor();
    assert.match(await summary.textContent(), /USB_A/);
    await summary.getByRole('button', { name: '关闭面板', exact: true }).click();
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page.getByLabel('界面语言', { exact: true }).selectOption('en-US');
    await page.getByRole('button', { name: 'Discover and select SDK', exact: true }).click();
    await page
      .getByRole('dialog', { name: 'Select or create SDK environment', exact: true })
      .waitFor();
    await page.setViewportSize({ width: 480, height: 800 });
    await page.getByRole('button', { name: 'Configure later', exact: true }).waitFor();
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
      false,
    );
    fs.mkdirSync(path.join('.qa', 'p0'), { recursive: true });
    await page.screenshot({ path: path.join('.qa', 'p0', 'environment-compact.png') });
    assert.deepEqual(errors, []);
    const report = {
      environmentDiscovery: true,
      explicitReuse: true,
      scanRecovery: true,
      keyboardNavigation: true,
      diskUsage: true,
      preferredTargetPersists: true,
      unavailableDefaultDoesNotFallback: true,
      continueWaiting: true,
      waitInstallationBindsTarget: true,
      copyFailureRecovery: true,
      summaryRedactionAndPreview: true,
      englishAndCompact: true,
      errors,
    };
    fs.writeFileSync(path.join('.qa', 'p0', 'ui-report.json'), JSON.stringify(report, null, 2));
    console.log(
      'P0 UI: environment discovery/retry/reuse, preferences/reload, keyboard, usage, safe waiting, clipboard recovery, redacted preview, English and compact layout passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
