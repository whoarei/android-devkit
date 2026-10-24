// Actual xterm renderer with isolated, byte-stream IPC fixtures. No SDK writes.
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
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.addInitScript(() => {
      window.isTauri = true;
      window.__commands = [];
      window.__callbacks = {};
      window.__terminals = {};
      window.__nextGeneration = 0;
      window.__rejectCreate = false;
      window.__holdCreate = false;
      window.__devices = [
        {
          serial: 'pixel-usb',
          transportId: '13',
          serverEpoch: 1,
          model: 'Pixel 8',
          state: 'ready',
          connection: 'usb',
          avdName: '',
          api: '35',
          abi: 'arm64-v8a',
        },
        {
          serial: 'emulator-5554',
          transportId: '14',
          serverEpoch: 1,
          model: 'Tablet',
          state: 'ready',
          connection: 'emulator',
          avdName: '',
          api: '35',
          abi: 'x86_64',
        },
      ];
      let callbackId = 0;
      window.__emitTerminal = (id, event) => {
        const terminal = window.__terminals[id];
        window.__callbacks[terminal.channel]({
          index: terminal.index++,
          message: {
            generation: terminal.info.generation,
            kind: 'data',
            offset: terminal.offset,
            data: [],
            state: '',
            reason: '',
            exitCode: null,
            ...event,
          },
        });
      };
      window.__outputTerminal = (id, text) => {
        const bytes = Array.from(new TextEncoder().encode(text));
        window.__terminals[id].offset += bytes.length;
        window.__emitTerminal(id, { data: bytes, offset: window.__terminals[id].offset });
      };
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
              devices: structuredClone(window.__devices),
              avds: [],
              server: 'fixture',
              serverEpoch: 1,
              freeBytes: 20 * 1024 ** 3,
            };
          if (command === 'terminal_list') return [];
          if (command === 'window_state') return { maximized: false };
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
            if (window.__holdCreate)
              await new Promise((resolve) => {
                window.__releaseCreate = resolve;
              });
            if (window.__rejectCreate)
              throw { code: 'TERMINAL_FAILED', message: 'Fixture PTY failure' };
            const info = {
              id: args.id,
              generation: `generation-${++window.__nextGeneration}`,
              kind: args.kind,
              label: args.kind === 'device' ? 'Pixel 8' : '通用命令行',
              target: args.target,
              sdkRoot: 'E:\\SDK',
              state: 'active',
              reason: '',
              exitCode: null,
            };
            window.__terminals[args.id] = {
              info,
              channel: args.onEvent.id,
              index: 0,
              offset: 0,
              input: [],
              sequence: 0,
              closed: false,
            };
            // Output before create returns covers the registration race.
            window.__outputTerminal(
              args.id,
              args.kind === 'device' ? 'Android Shell\r\nhusky:/ $ ' : '\x1b[6nPS E:\\SDK> ',
            );
            return info;
          }
          if (command === 'terminal_write') {
            const terminal = window.__terminals[args.id];
            if (args.generation !== terminal.info.generation)
              throw { code: 'TERMINAL_STALE', message: 'Stale input' };
            if (args.sequence !== terminal.sequence + 1)
              throw { code: 'TERMINAL_SEQUENCE', message: 'Out of order input' };
            terminal.sequence = args.sequence;
            terminal.input.push(...args.data);
            window.__outputTerminal(args.id, new TextDecoder().decode(Uint8Array.from(args.data)));
          }
          if (command === 'terminal_close') {
            if (window.__terminals[args.id]) window.__terminals[args.id].closed = true;
          }
          if (command === 'plugin:event|listen') return args.handler;
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    const row = page.locator('.device-row').filter({ hasText: 'Pixel 8' });
    const nav = () =>
      page.locator('.sidebar').getByRole('button', { name: 'ADB 终端', exact: true });
    const detail = () => page.locator('.detail-panel');
    const currentId = () =>
      page.evaluate(() =>
        Object.keys(window.__terminals)
          .filter((id) => !window.__terminals[id].closed)
          .at(-1),
      );
    const input = () => page.locator('.terminal-surface:not([hidden]) .xterm-helper-textarea');
    const countCreates = () =>
      page.evaluate(() => window.__commands.filter((c) => c.command === 'terminal_create').length);
    await row.waitFor();
    assert.equal(await row.getByRole('button', { name: 'ADB 终端', exact: true }).count(), 0);
    await row.click();
    await detail().getByRole('button', { name: 'ADB 终端', exact: true }).click();
    await page.locator('.terminal-state.active').waitFor();
    assert.equal(await page.getByRole('dialog').count(), 0);
    assert.equal(await countCreates(), 1);
    assert.match(await page.locator('.terminal-context').textContent(), /pixel-usb/);
    const pixel = await currentId();
    await input().pressSequentially('pwd');
    await input().press('Enter');
    await page.waitForFunction(
      (id) =>
        new TextDecoder().decode(Uint8Array.from(window.__terminals[id].input)).includes('pwd\r'),
      pixel,
    );
    await input().press('Control+c');
    await page.waitForFunction((id) => window.__terminals[id].input.includes(3), pixel);
    // Toolbar interruption remains a raw control byte when a shell enables bracketed paste.
    await page.evaluate((id) => window.__outputTerminal(id, '\x1b[?2004h'), pixel);
    await page.waitForTimeout(40);
    const beforeInterrupt = await page.evaluate((id) => window.__terminals[id].input.length, pixel);
    await page.getByRole('button', { name: '中断', exact: true }).click();
    await page.waitForFunction(({ id, offset }) => window.__terminals[id].input.length > offset, {
      id: pixel,
      offset: beforeInterrupt,
    });
    assert.deepEqual(
      await page.evaluate(({ id, offset }) => window.__terminals[id].input.slice(offset), {
        id: pixel,
        offset: beforeInterrupt,
      }),
      [3],
    );
    await page.evaluate((id) => window.__outputTerminal(id, '\x1b[?2004l'), pixel);
    await page.getByRole('button', { name: '常用命令', exact: true }).click();
    await page
      .locator('.terminal-snippets')
      .getByRole('button', { name: 'top', exact: true })
      .click();
    await page.waitForFunction(
      (id) =>
        new TextDecoder().decode(Uint8Array.from(window.__terminals[id].input)).includes('top'),
      pixel,
    );
    await page.waitForFunction(() => window.__commands.some((c) => c.command === 'terminal_ack'));
    await page.locator('.sidebar').getByRole('button', { name: '设备工作台', exact: true }).click();
    await detail().getByRole('button', { name: 'ADB 终端', exact: true }).click();
    assert.equal(await countCreates(), 1, 'Device entry should reuse the live session');
    // Output parsing and ACK continue while the entire terminal page is hidden.
    await page.locator('.sidebar').getByRole('button', { name: '设备工作台', exact: true }).click();
    await page.evaluate((id) => window.__outputTerminal(id, '\r\n后台中文输出\r\n'), pixel);
    await page.waitForFunction(
      (id) =>
        window.__commands.some(
          (c) =>
            c.command === 'terminal_ack' &&
            c.args.id === id &&
            c.args.offset === window.__terminals[id].offset,
        ),
      pixel,
    );
    await nav().click();
    await page
      .locator('.page-actions')
      .getByRole('button', { name: '新建会话', exact: true })
      .click();
    const menu = page.getByRole('menu', { name: '新建会话', exact: true });
    assert.equal(await menu.getByRole('textbox').count(), 0);
    await menu.getByRole('menuitem').filter({ hasText: '通用命令行' }).click();
    await page.waitForFunction(() => Object.keys(window.__terminals).length === 2);
    await page.locator('.terminal-state.active').waitFor();
    assert.equal(await page.getByRole('dialog').count(), 0);
    await page.locator('.terminal-tab.selected .icon-button').click();
    assert.equal(await page.locator('.terminal-tab').count(), 1);
    assert.equal(await page.getByRole('dialog').count(), 0);
    // Exit keeps the output, reopening changes generation and rejects old callbacks.
    const oldChannel = await page.evaluate((id) => window.__terminals[id].channel, pixel);
    await page.evaluate(
      (id) => window.__emitTerminal(id, { kind: 'state', state: 'exited', exitCode: 0 }),
      pixel,
    );
    const oldIndex = await page.evaluate((id) => window.__terminals[id].index, pixel);
    await page.getByRole('button', { name: '重新打开 Shell', exact: true }).click();
    await page.locator('.terminal-state.active').waitFor();
    assert.equal(await countCreates(), 3);
    await page.evaluate(
      ({ index, channel }) =>
        window.__callbacks[channel]({
          index,
          message: {
            generation: 'generation-1',
            kind: 'state',
            state: 'failed',
            reason: 'Stale failure',
            data: [],
            offset: 0,
            exitCode: 1,
          },
        }),
      { index: oldIndex, channel: oldChannel },
    );
    assert.equal(await page.locator('.terminal-state').textContent(), '已连接');
    await page.evaluate(() => {
      window.__devices[0].transportId = '99';
    });
    await page.locator('.sidebar').getByRole('button', { name: '设备工作台', exact: true }).click();
    await page.getByRole('button', { name: '重新扫描', exact: true }).click();
    await page.locator('[data-device-key="device:1:99:pixel-usb"]').waitFor();
    await nav().click();
    await page.evaluate(
      (id) =>
        window.__emitTerminal(id, {
          kind: 'state',
          state: 'disconnected',
          reason: '原设备连接已断开或变化，请重新连接',
        }),
      pixel,
    );
    await page.getByRole('button', { name: '重新连接', exact: true }).click();
    await page.locator('.terminal-state.active').waitFor();
    assert.equal(
      await page.evaluate(
        () =>
          window.__commands.filter((c) => c.command === 'terminal_create').at(-1).args.target
            .transportId,
      ),
      '99',
    );
    // Close while creation is in flight must close the eventual native child too.
    await page.evaluate(() => {
      window.__holdCreate = true;
    });
    await page
      .locator('.page-actions')
      .getByRole('button', { name: '新建会话', exact: true })
      .click();
    await menu.getByRole('menuitem').filter({ hasText: '通用命令行' }).click();
    await page.waitForFunction(() => typeof window.__releaseCreate === 'function');
    await page.locator('.terminal-tab.selected .icon-button').click();
    await page.evaluate(() => {
      window.__holdCreate = false;
      window.__releaseCreate();
    });
    await page.waitForFunction(
      () => window.__commands.filter((c) => c.command === 'terminal_close').length >= 4,
    );
    const out = path.resolve(__dirname, '../.qa/app');
    fs.mkdirSync(out, { recursive: true });
    await page.screenshot({ path: path.join(out, 'terminal-ui.png') });
    await page.setViewportSize({ width: 480, height: 700 });
    await page.waitForTimeout(150);
    await page.screenshot({ path: path.join(out, 'terminal-ui-compact.png') });
    assert.equal(
      await page.locator('.terminal-page').evaluate((el) => el.scrollWidth > el.clientWidth + 1),
      false,
    );
    await page.screenshot({ path: path.join(out, 'terminal-ui-compact.png') });
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    assert.equal(await page.locator('h1').textContent(), 'ADB Terminal');
    assert.deepEqual(errors, []);
    console.log(
      'Terminal UI: actual xterm input, early output, background ACK, direct Shell, detail entry/reuse, menu creation, immediate close, reopen generation, reconnect, pending-create cleanup, compact layout and English passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
