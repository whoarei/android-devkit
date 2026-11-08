// Isolated appearance/IPC regression. Never accesses SDK tools or user devices.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const output = path.resolve(__dirname, '../.qa/appearance');
const palettes = [
  'default',
  'blue',
  'green',
  'yellow',
  'pink',
  'orange',
  'purple',
  'black',
  'custom',
];
const names = ['默认', '蓝色', '绿色', '黄色', '粉色', '橙色', '紫色', '黑色', '自定义'];
const modes = ['light', 'dark', 'system'];
const brandColors = require('./fixtures/brand-icon-colors.json');

(async () => {
  fs.mkdirSync(output, { recursive: true });
  const browser = await chromium.launch({
    channel: process.env.PLAYWRIGHT_CHANNEL || 'msedge',
    headless: true,
  });
  try {
    const page = await browser.newPage({
      viewport: { width: 1280, height: 900 },
      colorScheme: 'light',
    });
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await require('./preferences-fixture.cjs')(page);
    await page.addInitScript(() => {
      window.isTauri = true;
      window.__appearanceCalls = [];
      window.__terminalCreates = 0;
      const settings = {
        sdkRoot: 'E:\\Android\\sdk',
        avdRoots: ['E:\\Android\\avd'],
        javaPath: 'E:\\Java\\bin\\java.exe',
        proxy: '',
        bootTimeoutSecs: 180,
        sdkSites: [],
        sdkChannel: 0,
        sdkDownload: { kind: 'google', name: '', url: '', verifyOfficial: true },
      };
      const devices = [
        {
          serial: 'appearance-fixture',
          transportId: '1',
          serverEpoch: 1,
          state: 'ready',
          model: 'Pixel 9 Pro',
          connection: 'usb',
          avdName: '',
          api: '36',
          abi: 'arm64-v8a',
        },
      ];
      let callback = 0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: () => ++callback,
        unregisterCallback() {},
        invoke: async (command, args) => {
          window.__appearanceCalls.push({ command, args });
          if (command === 'workspace_snapshot')
            return {
              setupRequired: false,
              settings,
              devices,
              avds: [],
              packages: [],
              tasks: [],
              environment: { adb: 'fixture', java: settings.javaPath, warnings: [] },
              server: 'fixture',
              serverEpoch: 1,
              freeBytes: 20 * 1024 ** 3,
            };
          if (
            ['terminal_list', 'sdk_sites_status', 'signing_records', 'artifacts_list'].includes(
              command,
            )
          )
            return [];
          if (command === 'window_state') return { maximized: false };
          if (command === 'plugin:event|listen') return args.handler;
          if (command === 'task_list')
            return {
              tasks: [],
              total: 0,
              counts: {},
              offset: 0,
              pageSize: 30,
              deletion: { blocked: {}, eligible: 0 },
            };
          if (command === 'java_check')
            return {
              path: settings.javaPath,
              source: 'configured',
              status: 'ready',
              version: '21',
              vendor: 'Fixture',
              architecture: 'amd64',
              message: 'Java 已就位',
              toolsReady: true,
              compatibilityChecked: true,
            };
          if (command === 'signing_environment')
            return {
              java: settings.javaPath,
              keytool: 'E:\\Java\\bin\\keytool.exe',
              apksigner: 'E:\\Android\\sdk\\apksigner.jar',
              zipalign: 'E:\\Android\\sdk\\zipalign.exe',
            };
          if (command === 'diagnostics')
            return {
              sdkRoot: settings.sdkRoot,
              acceleration: 'Windows Hypervisor Platform',
              java: 'Java 21',
              environment: { adb: 'ready' },
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
          if (command === 'terminal_create') {
            window.__terminalCreates++;
            return {
              id: args.id,
              generation: 'appearance-terminal',
              kind: args.kind,
              label: '通用命令行',
              target: args.target ?? null,
              sdkRoot: settings.sdkRoot,
              state: 'active',
              reason: '',
              exitCode: null,
            };
          }
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    await page.getByRole('heading', { name: '设备工作台', exact: true }).waitFor();
    const nav = (name) => page.locator('nav').getByRole('button', { name, exact: true });
    const root = page.locator('html');
    await nav('设置').click();
    const mode = page.getByLabel('界面主题', { exact: true });
    const choose = async (palette) => {
      await page
        .locator('.settings-panel .appearance-color')
        .filter({ hasText: names[palettes.indexOf(palette)] })
        .click();
      await page.waitForFunction(
        (value) => document.documentElement.dataset.colorTheme === value,
        palette,
      );
    };
    const assertContrast = async () => {
      const pairs = await page.evaluate(() => {
        return [
          '.settings-panel .button.primary',
          'nav button.active',
          '.appearance-color[aria-pressed="true"]',
        ].map((selector) => {
          const element = document.querySelector(selector),
            style = getComputedStyle(element);
          return { selector, foreground: style.color, background: style.backgroundColor };
        });
      });
      const light = (color) =>
        color
          .match(/[\d.]+/g)
          .slice(0, 3)
          .map(Number)
          .map((value) => value / 255)
          .map((value) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4))
          .reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
      for (const pair of pairs) {
        const a = light(pair.foreground),
          b = light(pair.background);
        assert((Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05) >= 4.5, JSON.stringify(pair));
      }
    };
    const assertBrand = async () => {
      const brand = await page.locator('.brand-mark').evaluate((node) => {
        const style = getComputedStyle(document.documentElement);
        return {
          palette: document.documentElement.dataset.colorTheme,
          dark: document.documentElement.dataset.theme === 'dark',
          customColor: window.__preferences.customColor,
          fill: style.getPropertyValue('--brand-fill').trim(),
          foreground: style.getPropertyValue('--brand-foreground').trim(),
          matrix: node.querySelector('feColorMatrix')?.getAttribute('values'),
          artwork: node.querySelector('image')?.getAttribute('href'),
        };
      });
      const expected = brandColors.find(
        (item) =>
          item.palette === brand.palette &&
          item.dark === brand.dark &&
          (item.palette !== 'custom' ||
            item.customColor.toLowerCase() === brand.customColor.toLowerCase()),
      );
      assert(expected, JSON.stringify(brand));
      assert.equal(brand.fill, expected.fill);
      assert.equal(brand.foreground, expected.foreground);
      assert.equal(brand.artwork, '/android-devkit.png', 'Keep the original brand artwork');
      const hex = (value) => [1, 3, 5].map((index) => parseInt(value.slice(index, index + 2), 16));
      const fill = hex(expected.fill),
        foreground = hex(expected.foreground);
      const matrix = fill.flatMap((value, index) => [
        foreground[index] / 255 - value / 128,
        value / 128,
        0,
        0,
        0,
      ]);
      matrix.push(0, 0, 0, 1, 0);
      assert.deepEqual(
        brand.matrix.split(/\s+/).map(Number),
        matrix,
        'Logo and tray share the same recoloring matrix',
      );
    };
    const statusColors = [];
    for (const palette of palettes) {
      await choose(palette);
      for (const value of modes) {
        await mode.selectOption(value);
        await page.waitForFunction(
          (value) => document.documentElement.dataset.themeMode === value,
          value,
        );
        await assertContrast();
        await assertBrand();
        statusColors.push(
          await page.evaluate(() => {
            const style = getComputedStyle(document.documentElement);
            return ['--success', '--warning', '--danger']
              .map((name) => style.getPropertyValue(name).trim())
              .join('|');
          }),
        );
      }
    }
    assert.equal(new Set(statusColors).size, 2, 'status colors are independent of accent');
    for (const color of ['#000000', '#FFFFFF', '#FFFF00', '#808080', '#F55CA6']) {
      await page.getByLabel('十六进制颜色', { exact: true }).fill(color);
      await page.getByLabel('十六进制颜色', { exact: true }).press('Enter');
      await page.waitForFunction(
        (color) => window.__preferences.customColor === color.toLowerCase(),
        color,
      );
      for (const value of ['light', 'dark']) {
        await mode.selectOption(value);
        await assertContrast();
        await assertBrand();
      }
    }
    await page.getByLabel('十六进制颜色', { exact: true }).fill('#GGGGGG');
    await page.getByLabel('十六进制颜色', { exact: true }).press('Tab');
    assert.equal(await page.getByLabel('十六进制颜色', { exact: true }).inputValue(), '#F55CA6');
    const draft = page.getByLabel('当前 SDK 根目录', { exact: true });
    await draft.fill('E:\\Unsaved-appearance-draft');
    await choose('purple');
    await mode.selectOption('system');
    const actionsBefore = await page.evaluate(
      () => window.__appearanceCalls.filter((call) => call.command === 'window_action').length,
    );
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    await assertBrand();
    await page.emulateMedia({ colorScheme: 'light' });
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'light');
    await assertBrand();
    assert.equal(await page.evaluate(() => window.__preferences.theme), 'system');
    assert.equal(
      await page.evaluate(
        () => window.__appearanceCalls.filter((call) => call.command === 'window_action').length,
      ),
      actionsBefore,
    );
    assert.equal(await draft.inputValue(), 'E:\\Unsaved-appearance-draft');
    await page.reload();
    await mode.waitFor();
    assert.equal(await mode.inputValue(), 'system');
    assert.equal(await root.getAttribute('data-color-theme'), 'purple');
    await assertBrand();
    assert.equal(
      await page.evaluate(
        () => JSON.parse(localStorage.getItem('android-devkit.appearance.v1')).colorTheme,
      ),
      'purple',
    );
    // A stale paint cache never overrides the authoritative preference response.
    await page.evaluate(() =>
      localStorage.setItem(
        'android-devkit.appearance.v1',
        JSON.stringify({ theme: 'dark', colorTheme: 'yellow' }),
      ),
    );
    await page.reload();
    await mode.waitFor();
    assert.equal(await root.getAttribute('data-color-theme'), 'purple');
    assert.equal(await mode.inputValue(), 'system');
    await mode.selectOption('dark');
    await choose('purple');
    for (const name of [
      '设备工作台',
      'ADB 终端',
      'SDK 管理',
      '签名与 Keystore',
      '任务中心',
      '环境诊断',
      '设置',
    ]) {
      await nav(name).click();
      await page.getByRole('heading', { name, exact: true }).waitFor();
      assert.equal(await root.getAttribute('data-color-theme'), 'purple');
      assert.equal(await root.getAttribute('data-theme'), 'dark');
      assert.equal(
        await page
          .locator('nav button.active')
          .evaluate((node) => getComputedStyle(node).boxShadow),
        'none',
      );
      assert.equal(
        await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
        false,
      );
      await page.screenshot({
        path: path.join(
          output,
          `page-${['设备工作台', 'ADB 终端', 'SDK 管理', '签名与 Keystore', '任务中心', '环境诊断', '设置'].indexOf(name)}-dark.png`,
        ),
      });
    }
    await nav('ADB 终端').click();
    await page.getByRole('button', { name: '新建会话', exact: true }).first().click();
    await page
      .getByRole('menuitem', { name: /通用命令行/ })
      .first()
      .click();
    await page.locator('.xterm').waitFor();
    const before = await page
      .locator('.terminal-views')
      .evaluate((node) => getComputedStyle(node).backgroundColor);
    await page.getByRole('button', { name: '外观', exact: true }).click();
    let popover = page.getByRole('dialog', { name: '外观设置', exact: true });
    await popover.getByLabel('界面主题', { exact: true }).selectOption('light');
    await popover.getByRole('button', { name: '绿色', exact: true }).click();
    assert.notEqual(
      await page
        .locator('.terminal-views')
        .evaluate((node) => getComputedStyle(node).backgroundColor),
      before,
    );
    assert.equal(await page.evaluate(() => window.__terminalCreates), 1);
    await popover.getByRole('button', { name: '关闭外观设置', exact: true }).click();
    await page.setViewportSize({ width: 480, height: 640 });
    await page.getByRole('button', { name: '外观', exact: true }).click();
    popover = page.getByRole('dialog', { name: '外观设置', exact: true });
    assert.equal(await popover.evaluate((node) => node.scrollWidth > node.clientWidth), false);
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
      false,
    );
    await page.screenshot({ path: path.join(output, 'appearance-480.png') });
    await page.keyboard.press('Escape');
    assert.equal(await popover.count(), 0);
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
      false,
    );
    assert(
      await page
        .locator('.window-close')
        .evaluate((node) => node.getBoundingClientRect().right <= innerWidth),
      'English titlebar controls must remain visible at 480px',
    );
    await page.getByRole('button', { name: 'Appearance', exact: true }).click();
    await page
      .getByRole('dialog', { name: 'Appearance settings', exact: true })
      .getByLabel('Interface theme', { exact: true })
      .selectOption('system');
    await page.getByRole('button', { name: 'Custom', exact: true }).click();
    assert(
      await page
        .getByRole('button', { name: 'Custom', exact: true })
        .locator('span')
        .nth(1)
        .evaluate((node) => node.getBoundingClientRect().height < 24),
      'The selected English color label must fit on one line',
    );
    await page.getByLabel('Hex color', { exact: true }).fill('#35B8BD');
    await page.getByLabel('Hex color', { exact: true }).press('Enter');
    await page.screenshot({ path: path.join(output, 'appearance-english-480.png') });
    await page.keyboard.press('Escape');
    assert.deepEqual(errors, []);
    console.log(
      'Appearance UI: all 27 palette/mode combinations, logo/tray color contract, contrast, custom colors, semantic status colors, system changes, persistence/cache precedence, seven pages, drafts, live terminal, English and 480px layout passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
