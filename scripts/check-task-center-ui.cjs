// Tests the UI with isolated IPC fixtures; never reads or alters real tasks/devices.
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
      const task = (id, state, title, extra = {}) => ({
        id,
        requestId: id,
        title,
        state,
        phase: 'fixture',
        message: '已验证设备状态 · 原始目标保持不变',
        operation: {
          kind: 'network_connect',
          args: { address: '192.168.1.20:5555' },
          target: null,
        },
        createdAt: Math.floor(Date.now() / 1000),
        updatedAt: Math.floor(Date.now() / 1000),
        progress: null,
        log: ['完整诊断输出：device-target-preserved'],
        artifact: '',
        licensePrompt: '',
        cancellable: true,
        cancelRequested: false,
        confirmedLicenses: [],
        ...extra,
      });
      window.__tasks = [
        task('run', 'running', '下载 Android SDK', { progress: 45 }),
        task('queue', 'queued', '安装系统镜像'),
        task('license', 'waiting_confirmation', '等待官方许可', {
          licensePrompt: 'Fixture license terms',
        }),
        task('fail', 'failed', '连接 Pixel 网络 ADB', {
          message: '连接被拒绝，请检查设备 IP 与端口。'.repeat(18),
        }),
        task('review', 'needs_review', '任务结果待检查'),
        task('cancel', 'cancelled', '已取消的组件下载'),
        task('capture', 'succeeded', '设备截图', {
          artifact: 'fixture.png',
          operation: {
            kind: 'capture',
            args: {},
            target: { serial: 'Pixel-USB', serverEpoch: 1, transportId: '2' },
          },
        }),
        task('recording', 'succeeded', '文件采集结束', {
          artifact: 'fixture.json',
          operation: { kind: 'log_record', args: {}, target: null },
        }),
        ...Array.from({ length: 65 }, (_, i) =>
          task(`history-${i}`, 'succeeded', `历史任务 ${i + 1}`),
        ),
      ];
      window.__calls = [];
      window.isTauri = true;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: () => 1,
        unregisterCallback() {},
        invoke: async (command, args = {}) => {
          window.__calls.push({ command, args });
          if (command === 'workspace_snapshot')
            return {
              settings: { sdkRoot: 'E:\\Android\\sdk' },
              environment: { adb: 'fixture' },
              packages: [],
              avds: [],
              tasks: structuredClone(window.__tasks),
              devices: [],
              server: 'Fixture',
              serverEpoch: 1,
              freeBytes: 1024 ** 3,
            };
          if (command === 'window_state') return { maximized: false };
          if (command === 'artifacts_list') return [];
          if (command === 'task_list') {
            const { keyword, state, offset } = args.query;
            const active = (state) => ['running', 'queued', 'waiting_confirmation'].includes(state);
            const matched = window.__tasks.filter((task) =>
              `${task.id}\n${task.title}\n${task.message}\n${task.operation.args.address || ''}`
                .toLowerCase()
                .includes(keyword.trim().toLowerCase()),
            );
            const counts = {};
            for (const task of matched) counts[task.state] = (counts[task.state] || 0) + 1;
            const filtered = matched.filter(
              (task) =>
                state === 'all' || (state === 'active' ? active(task.state) : task.state === state),
            );
            const start = Math.min(offset, Math.floor(Math.max(0, filtered.length - 1) / 30) * 30);
            return {
              tasks: structuredClone(filtered.slice(start, start + 30)),
              total: filtered.length,
              counts,
              offset: start,
              pageSize: 30,
            };
          }
          if (command === 'task_cancel') {
            // Model asynchronous native IPC so the test waits for the rendered refresh.
            await new Promise((resolve) => setTimeout(resolve, 25));
            window.__tasks.find((task) => task.id === args.taskId).state = 'cancelled';
            return null;
          }
          if (command === 'license_respond') {
            window.__tasks.find((task) => task.id === args.taskId).licensePrompt = '';
            return null;
          }
          if (command === 'task_retry') {
            const task = window.__tasks.find((task) => task.id === args.taskId);
            return {
              id: 'plan',
              title: '重试原任务',
              operation: task.operation,
              effects: [task.title],
              warnings: [],
              licenses: [],
            };
          }
          if (command === 'recording_segments')
            return { files: [{ name: 'segment-1.log', bytes: 100 }] };
          if (command.startsWith('plugin:event|')) return 1;
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    await page.locator('nav').getByRole('button', { name: '任务中心', exact: true }).click();
    const rows = page.locator('.task-card');
    const filters = page.getByRole('group', { name: '任务状态筛选' });
    const waitForTasks = (ids) =>
      page.waitForFunction((expected) => {
        const list = document.querySelector('.task-list');
        const actual = [...document.querySelectorAll('.task-card')].map(
          (row) => row.dataset.taskId,
        );
        return (
          list?.getAttribute('aria-busy') === 'false' &&
          JSON.stringify(actual) === JSON.stringify(expected)
        );
      }, ids);
    const select = async (name, ids) => {
      await filters.getByRole('button', { name: new RegExp(`^${name}`) }).click();
      await waitForTasks(ids);
    };
    await page.locator('[data-task-id="run"]').waitFor();
    assert.equal(await rows.count(), 30);
    const failed = page.locator('[data-task-id="fail"]');
    const collapsedHeight = (await failed.boundingBox()).height;
    assert(collapsedHeight <= 85, `compact row was ${collapsedHeight}px`);
    assert.equal(await failed.locator('.task-log').count(), 0);
    await failed.getByRole('button', { name: '展开详情', exact: true }).click();
    await failed.locator('.task-log').getByText('完整诊断输出：device-target-preserved').waitFor();
    assert((await failed.boundingBox()).height > collapsedHeight);
    await failed.getByRole('button', { name: '收起详情', exact: true }).click();
    await failed.getByRole('button', { name: '重试', exact: true }).click();
    await page.getByRole('dialog', { name: '重试原任务', exact: true }).waitFor();
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    const license = page.locator('[data-task-id="license"]');
    assert.equal(
      await license
        .getByRole('button', { name: '展开详情', exact: true })
        .getAttribute('aria-expanded'),
      'false',
    );
    await license.getByRole('button', { name: '接受当前许可', exact: true }).click();
    await page.getByRole('button', { name: '下一页', exact: true }).click();
    await page.locator('[data-task-id="history-22"]').waitFor();
    await select('失败', ['fail']);
    assert.equal(await rows.count(), 1);
    assert.equal(await rows.first().getAttribute('data-task-id'), 'fail');
    const search = page.getByRole('textbox', { name: '搜索任务', exact: true });
    await search.fill('pixel');
    await page.waitForFunction(() =>
      window.__calls.some(
        (call) => call.command === 'task_list' && call.args.query.keyword === 'pixel',
      ),
    );
    await page.locator('.task-list[aria-busy="false"]').waitFor();
    assert.equal(await rows.count(), 1);
    await search.fill('不存在的任务');
    await page.getByRole('heading', { name: '没有匹配的任务', exact: true }).waitFor();
    await page.getByRole('button', { name: '清除筛选', exact: true }).click();
    await page.locator('[data-task-id="run"]').waitFor();
    await select('进行中', ['run', 'queue', 'license']);
    assert.equal(await rows.count(), 3);
    await page
      .locator('[data-task-id="run"]')
      .getByRole('button', { name: '取消', exact: true })
      .click();
    await page.waitForFunction(() =>
      window.__calls.some((call) => call.command === 'task_cancel' && call.args.taskId === 'run'),
    );
    await waitForTasks(['queue', 'license']);
    assert.equal(await rows.count(), 2);
    for (const [name, expected] of [
      ['待确认', ['license']],
      ['待检查', ['review']],
      ['已取消', ['run', 'cancel']],
    ]) {
      await select(name, expected);
      assert.equal(await rows.count(), expected.length);
    }
    await select('已完成', [
      'capture',
      'recording',
      ...Array.from({ length: 28 }, (_, i) => `history-${i}`),
    ]);
    const recording = page.locator('[data-task-id="recording"]');
    assert.equal(
      await recording.getByRole('button', { name: '选择采集段导出', exact: true }).count(),
      0,
    );
    await recording.getByRole('button', { name: '展开详情', exact: true }).click();
    await recording.getByRole('button', { name: '选择采集段导出', exact: true }).click();
    await recording.getByText('segment-1.log', { exact: false }).waitFor();
    await recording.getByRole('button', { name: '收起详情', exact: true }).click();
    const capture = page.locator('[data-task-id="capture"]');
    await capture.getByRole('button', { name: '展开详情', exact: true }).click();
    await capture.getByRole('button', { name: '保存截图', exact: true }).click();
    assert(
      await page.evaluate(() =>
        window.__calls.some(
          (call) => call.command === 'artifact_save' && call.args.taskId === 'capture',
        ),
      ),
    );
    await capture.getByRole('button', { name: '收起详情', exact: true }).click();
    await select('全部', [
      'run',
      'queue',
      'license',
      'fail',
      'review',
      'cancel',
      'capture',
      'recording',
      ...Array.from({ length: 22 }, (_, i) => `history-${i}`),
    ]);
    const output = path.resolve(__dirname, '../.qa/app');
    fs.mkdirSync(output, { recursive: true });
    await page.screenshot({ path: path.join(output, 'task-center-compact-list.png') });
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
      false,
    );
    await page.screenshot({ path: path.join(output, 'task-center-narrow.png') });
    assert.deepEqual(errors, []);
    console.log(
      `Task center: ${collapsedHeight}px desktop rows; filters, combined search, pagination reset, details, cancellation, retry, license, capture and recording actions, narrow layout passed.`,
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
