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
    await require('./preferences-fixture.cjs')(page);
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
        task('review', 'needs_review', '任务结果待检查', {
          phase: 'reconciled',
          operation: { kind: 'avd_start', args: { avdId: 'fixture-avd' }, target: null },
        }),
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
      window.__artifacts = Array.from({ length: 7 }, (_, i) => ({
        id: i === 0 ? 'capture' : `export-${i}`,
        kind: i === 0 ? 'screenshot' : 'export',
        title: i === 0 ? '设备截图' : i < 3 ? '所选采集段' : '当前日志视图',
        taskId: i === 0 ? 'capture' : '',
        bytes: i === 0 ? 40000 : 3 * 1024 * 1024,
        sha256: 'fixture',
        createdAt: 1792483200 + i * 60,
        acknowledged: false,
      }));
      window.__saveSucceeds = false;
      window.__rejectDiscard = false;
      window.__calls = [];
      window.__deleteFailures = [];
      window.__deleteRace = '';
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
          if (command === 'artifacts_list') return structuredClone(window.__artifacts);
          if (command === 'export_page')
            return {
              artifact: structuredClone(window.__artifacts.find((a) => a.id === args.artifactId)),
              text: 'Fixture export content',
              offset: 0,
              nextOffset: null,
              rows: 1,
              redacted: true,
              truncated: false,
              warning: '',
            };
          if (command === 'export_save') {
            if (!window.__saveSucceeds) return null;
            window.__artifacts.find((a) => a.id === args.artifactId).acknowledged = true;
            return 'C:\\saved-export.txt';
          }
          if (command === 'artifacts_discard') {
            await new Promise((resolve) => setTimeout(resolve, 25));
            if (window.__rejectDiscard)
              throw { code: 'IO_ERROR', message: 'Fixture cache file is locked' };
            window.__artifacts = window.__artifacts.filter((a) => !args.artifactIds.includes(a.id));
            if (args.artifactIds.includes('capture'))
              window.__tasks.find((t) => t.id === 'capture').artifact = '';
            return null;
          }
          if (command === 'artifact_copy')
            return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=';
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
            const deletion = { eligible: 0, active: 0, files: 0, review: 0, busy: 0, blocked: {} };
            for (const task of filtered) {
              let reason = '';
              if (active(task.state)) {
                deletion.active++;
                reason = '任务仍在进行，请先取消并等待结束';
              } else if (task.operation.kind === 'log_record' && task.artifact) {
                deletion.files++;
                reason = '原始采集仍在，请先处理采集文件';
              } else if (window.__artifacts.some((a) => a.taskId === task.id && !a.acknowledged)) {
                deletion.files++;
                reason = '有待保存文件，请先处理';
              } else if (task.state === 'needs_review') deletion.review++;
              else deletion.eligible++;
              if (reason) deletion.blocked[task.id] = reason;
            }
            return {
              tasks: structuredClone(filtered.slice(start, start + 30)),
              total: filtered.length,
              counts,
              offset: start,
              pageSize: 30,
              deletion,
            };
          }
          if (command === 'task_delete') {
            await new Promise((resolve) => setTimeout(resolve, 35));
            const request = args.request;
            const filtered = request.query
              ? window.__tasks.filter(
                  (task) =>
                    `${task.id}\n${task.title}\n${task.message}\n${task.operation.args.address || ''}`
                      .toLowerCase()
                      .includes(request.query.keyword.toLowerCase()) &&
                    (request.query.state === 'all' ||
                      (request.query.state === 'active'
                        ? ['running', 'queued', 'waiting_confirmation'].includes(task.state)
                        : task.state === request.query.state)),
                )
              : window.__tasks.filter((task) => request.taskIds.includes(task.id));
            const response = { deleted: 0, alreadyDeleted: 0, issues: [] };
            const deleted = [];
            for (const task of filtered) {
              if (task.id === window.__deleteRace) task.state = 'running';
              let reason = '';
              let retryable = false;
              if (['running', 'queued', 'waiting_confirmation'].includes(task.state))
                reason = '任务仍在进行，请先取消并等待结束';
              else if (
                (task.operation.kind === 'log_record' && task.artifact) ||
                window.__artifacts.some((a) => a.taskId === task.id && !a.acknowledged)
              )
                reason = '请先处理任务文件';
              else if (request.query && task.state === 'needs_review')
                reason = '结果待检查，已保留';
              else if (window.__deleteFailures.includes(task.id)) {
                reason = 'Fixture storage locked';
                retryable = true;
              }
              if (reason)
                response.issues.push({ taskId: task.id, title: task.title, reason, retryable });
              else {
                response.deleted++;
                deleted.push(task.id);
              }
            }
            window.__tasks = window.__tasks.filter((task) => !deleted.includes(task.id));
            return response;
          }
          if (command === 'recording_discard') {
            window.__tasks.find((task) => task.id === args.taskId).artifact = '';
            return null;
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
    await page.getByRole('button', { name: '待保存文件：7 项', exact: true }).waitFor();
    assert.equal(await page.locator('.artifact-list').count(), 0);
    assert.equal(await page.locator('.artifact-item').count(), 0);
    assert.equal(await rows.count(), 30);
    const failed = page.locator('[data-task-id="fail"]');
    await page
      .locator('[data-task-id="review"]')
      .getByRole('button', { name: '继续等待系统就绪', exact: true })
      .waitFor();
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
      await license.getByRole('button', { name: '删除任务记录', exact: true }).isDisabled(),
      true,
    );
    const pendingCapture = page.locator('[data-task-id="capture"]');
    assert.equal(
      await pendingCapture.getByRole('button', { name: '删除任务记录', exact: true }).isDisabled(),
      true,
    );
    await pendingCapture.getByRole('button', { name: '处理文件', exact: true }).click();
    let scopedFiles = page.getByRole('dialog', { name: '待保存文件', exact: true });
    await scopedFiles.locator('.artifact-item').first().waitFor();
    assert.equal(await scopedFiles.locator('.artifact-item').count(), 1);
    await scopedFiles.getByRole('button', { name: '关闭面板', exact: true }).click();
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
    await capture.getByRole('button', { name: '预览截图', exact: true }).click();
    await page.locator('.screenshot').waitFor();
    await page.getByRole('dialog').getByRole('button', { name: '关闭面板', exact: true }).click();
    assert.equal(await page.getByRole('dialog').count(), 0);
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
    const inbox = () => page.locator('.artifact-inbox');
    await inbox().click();
    let files = page.getByRole('dialog', { name: '待保存文件', exact: true });
    await files.locator('.artifact-item').first().waitFor();
    assert.equal(await files.locator('.artifact-item').count(), 7);
    assert.equal(await files.locator('time').count(), 7);
    await files.getByText('39.1 KiB', { exact: false }).waitFor();
    assert.equal(await files.evaluate((el) => el.scrollWidth > el.clientWidth), false);
    await page.screenshot({ path: path.join(output, 'artifact-inbox-narrow.png') });
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.getByRole('button', { name: '切换为暗色主题', exact: true }).click();
    await page.screenshot({ path: path.join(output, 'artifact-inbox-dark.png') });
    await page.getByRole('button', { name: '切换为亮色主题', exact: true }).click();
    await files
      .locator('[data-artifact-id="export-1"]')
      .getByRole('button', { name: '预览', exact: true })
      .click();
    const exportDialog = page.getByRole('dialog', { name: '导出内容预览', exact: true });
    await exportDialog.getByText('Fixture export content').waitFor();
    await exportDialog.getByRole('button', { name: '保存到本地…', exact: true }).click();
    await exportDialog.getByRole('button', { name: '关闭面板', exact: true }).click();
    await page.getByRole('button', { name: '待保存文件：7 项', exact: true }).waitFor();
    files = page.getByRole('dialog', { name: '待保存文件', exact: true });
    await files.waitFor();
    assert.equal(await page.getByRole('dialog').count(), 1);
    // The list remains mounted and keeps its scroll position behind the preview.
    await page.setViewportSize({ width: 480, height: 640 });
    const lastPreview = files
      .locator('[data-artifact-id="export-6"]')
      .getByRole('button', { name: '预览', exact: true });
    await lastPreview.scrollIntoViewIfNeeded();
    const scrollBeforePreview = await files.evaluate((el) => el.scrollTop);
    assert(scrollBeforePreview > 0);
    await lastPreview.click();
    await exportDialog.waitFor();
    assert.equal(await page.getByRole('dialog').count(), 1);
    await page.keyboard.press('Escape');
    await files.waitFor();
    assert.equal(await files.evaluate((el) => el.scrollTop), scrollBeforePreview);
    await page.waitForFunction(
      () =>
        document.activeElement?.closest('[data-artifact-id]')?.getAttribute('data-artifact-id') ===
        'export-6',
    );
    await page.keyboard.press('Escape');
    await files.waitFor({ state: 'hidden' });
    assert.equal(await page.getByRole('dialog').count(), 0);
    await page.setViewportSize({ width: 1280, height: 800 });
    await inbox().click();
    await page.evaluate(() => {
      window.__saveSucceeds = true;
    });
    await files
      .locator('[data-artifact-id="export-1"]')
      .getByRole('button', { name: '保存…', exact: true })
      .click();
    await files.locator('[data-artifact-id="export-1"]').waitFor({ state: 'detached' });
    await page.getByRole('button', { name: '待保存文件：6 项', exact: true }).waitFor();
    await files
      .locator('[data-artifact-id="export-2"]')
      .getByRole('button', { name: '忽略并清理', exact: true })
      .click();
    await files.locator('[data-artifact-id="export-2"]').waitFor({ state: 'detached' });
    await page.getByRole('button', { name: '待保存文件：5 项', exact: true }).waitFor();
    const single = await page.evaluate(() =>
      window.__calls.filter((c) => c.command === 'artifacts_discard').at(-1),
    );
    assert.deepEqual(single.args.artifactIds, ['export-2']);
    await files
      .locator('[data-artifact-id="capture"]')
      .getByRole('button', { name: '预览', exact: true })
      .click();
    await page.locator('.screenshot').waitFor();
    await page.getByRole('dialog').getByRole('button', { name: '关闭面板', exact: true }).click();
    files = page.getByRole('dialog', { name: '待保存文件', exact: true });
    await files.waitFor();
    for (const closeWith of ['escape', 'backdrop']) {
      await files
        .locator('[data-artifact-id="capture"]')
        .getByRole('button', { name: '预览', exact: true })
        .click();
      const imageDialog = page.getByRole('dialog', { name: '设备截图', exact: true });
      await imageDialog.waitFor();
      assert.equal(await page.getByRole('dialog').count(), 1);
      if (closeWith === 'escape') await page.keyboard.press('Escape');
      else await imageDialog.locator('..').click({ position: { x: 4, y: 4 } });
      await files.waitFor();
      assert.equal(await page.getByRole('dialog').count(), 1);
      assert.equal(await files.locator('.artifact-item').count(), 5);
    }
    await files
      .locator('[data-artifact-id="export-3"]')
      .getByRole('button', { name: '预览', exact: true })
      .click();
    await exportDialog.getByRole('button', { name: '保存到本地…', exact: true }).click();
    await exportDialog.getByRole('status').waitFor();
    await exportDialog.locator('..').click({ position: { x: 4, y: 4 } });
    await files.waitFor();
    await files.locator('[data-artifact-id="export-3"]').waitFor({ state: 'detached' });
    await page.getByRole('button', { name: '待保存文件：4 项', exact: true }).waitFor();
    await page.evaluate(() => {
      window.__rejectDiscard = true;
    });
    await files.getByRole('button', { name: '全部忽略并清理', exact: true }).click();
    await files.getByRole('alert').filter({ hasText: 'Fixture cache file is locked' }).waitFor();
    assert.equal(await files.locator('.artifact-item').count(), 4);
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    files = page.getByRole('dialog', { name: 'Pending files', exact: true });
    await files.getByRole('button', { name: 'Discard all and clear cache', exact: true }).waitFor();
    await page.evaluate(() => {
      window.__rejectDiscard = false;
    });
    await files.getByRole('button', { name: 'Discard all and clear cache', exact: true }).click();
    await files.getByText('No pending files', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Pending files: 0', exact: true }).waitFor();
    assert.equal(await page.locator('.artifact-count').count(), 0);
    assert.equal(await files.locator('.artifact-item').count(), 0);
    const batch = await page.evaluate(() =>
      window.__calls.filter((c) => c.command === 'artifacts_discard').at(-1),
    );
    assert.deepEqual(batch.args.artifactIds, ['capture', 'export-4', 'export-5', 'export-6']);
    await files.getByRole('button', { name: 'Close panel', exact: true }).click();
    await page.locator('[data-task-id="capture"]').waitFor();
    assert.equal(
      await page
        .locator('[data-task-id="capture"]')
        .getByRole('button', { name: 'Preview screenshot', exact: true })
        .count(),
      0,
    );
    await inbox().click();
    await page
      .getByRole('dialog', { name: 'Pending files', exact: true })
      .getByText('No pending files', { exact: true })
      .waitFor();
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Close panel', exact: true })
      .click();
    await page.getByRole('button', { name: 'Switch interface language', exact: true }).click();
    const assertRightActions = async () => {
      const checks = await rows.evaluateAll((items) =>
        items.map((row) => {
          const info = row.querySelector('.task-overview').getBoundingClientRect();
          const actions = row.querySelector('.task-row-actions').getBoundingClientRect();
          const box = row.getBoundingClientRect();
          return {
            right: actions.left >= info.right,
            center: Math.abs((actions.top + actions.bottom - info.top - info.bottom) / 2),
            gap: box.right - actions.right,
          };
        }),
      );
      for (const check of checks) {
        assert(check.right);
        assert(check.center < 2);
        assert(check.gap <= 14);
      }
    };
    await assertRightActions();
    await page.setViewportSize({ width: 480, height: 740 });
    await assertRightActions();
    await page.screenshot({ path: path.join(output, 'task-deletion-narrow.png') });
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.screenshot({ path: path.join(output, 'task-deletion-desktop.png') });
    // Deletion never opens a confirmation, including a needs-review record.
    await page
      .locator('[data-task-id="review"]')
      .getByRole('button', { name: '删除任务记录', exact: true })
      .click();
    assert.equal(await page.getByRole('dialog').count(), 0);
    await page.locator('[data-task-id="review"]').waitFor({ state: 'detached' });
    await page
      .locator('.task-delete-feedback')
      .getByText('已删除 1 条任务记录，保留 0 条', { exact: true })
      .waitFor();
    await page.getByRole('button', { name: '管理任务', exact: true }).click();
    await page.getByRole('checkbox', { name: '全选本页可删除项', exact: true }).check();
    await page.getByRole('button', { name: '下一页', exact: true }).click();
    await page.locator('.task-selection').getByText('已选 0 项', { exact: true }).waitFor();
    await page.getByRole('checkbox', { name: '全选本页可删除项', exact: true }).check();
    const countBefore = await page.evaluate(() => window.__tasks.length);
    await page
      .locator('.task-selection')
      .getByRole('button', { name: /删除所选/ })
      .click();
    assert.equal(await page.getByRole('dialog').count(), 0);
    await page.waitForFunction((before) => window.__tasks.length < before, countBefore);
    await page.getByRole('button', { name: '完成管理', exact: true }).click();
    // Raw files stay protected until a separate direct cleanup operation.
    await search.fill('文件采集结束');
    await page.locator('[data-task-id="recording"]').waitFor();
    await page.locator('.task-list[aria-busy="false"]').waitFor();
    await page
      .locator('[data-task-id="recording"]')
      .getByRole('button', { name: '处理采集', exact: true })
      .click();
    await page.getByRole('button', { name: '清理原始采集', exact: true }).click();
    await page
      .locator('[data-task-id="recording"]')
      .getByRole('button', { name: '删除任务记录', exact: true })
      .waitFor({ state: 'visible' });
    await page.waitForFunction(
      () =>
        !document.querySelector('[data-task-id="recording"] [aria-label="删除任务记录"]').disabled,
    );
    assert.equal(await page.getByRole('dialog').count(), 0);
    // State changes and storage failures preserve records and support scoped retry.
    await search.fill('连接 Pixel');
    await page.locator('.task-list[aria-busy="false"]').waitFor();
    await page.evaluate(() => {
      window.__deleteRace = 'fail';
    });
    await failed.getByRole('button', { name: '删除任务记录', exact: true }).click();
    await failed.getByText('运行中', { exact: true }).waitFor();
    assert.equal(
      await failed.getByRole('button', { name: '删除任务记录', exact: true }).isDisabled(),
      true,
    );
    await page.evaluate(() => {
      window.__deleteRace = '';
      window.__tasks.find((t) => t.id === 'fail').state = 'failed';
      window.__deleteFailures = ['fail'];
    });
    await search.fill('');
    await search.fill('连接 Pixel');
    await page.waitForFunction(
      () => !document.querySelector('[data-task-id="fail"] [aria-label="删除任务记录"]').disabled,
    );
    await failed.getByRole('button', { name: '删除任务记录', exact: true }).click();
    await page
      .locator('.task-delete-feedback')
      .getByText('已删除 0 条任务记录，保留 1 条', { exact: true })
      .waitFor();
    await page.getByText('查看未删除项', { exact: true }).click();
    await page.getByText('Fixture storage locked', { exact: false }).waitFor();
    await page.evaluate(() => {
      window.__deleteFailures = [];
    });
    await page.getByRole('button', { name: '重试未删除项', exact: true }).click();
    await failed.waitFor({ state: 'detached' });
    assert.equal(await page.getByRole('dialog').count(), 0);
    // Search-scoped cleanup reaches every page while protecting active tasks.
    await search.fill('历史任务');
    await page.waitForFunction(() =>
      [...document.querySelectorAll('.task-toolbar button')].some(
        (b) => b.textContent.includes('清理历史（') && !b.disabled,
      ),
    );
    const historyBefore = await page.evaluate(
      () => window.__tasks.filter((t) => t.id.startsWith('history-')).length,
    );
    assert(historyBefore > 30);
    await page.getByRole('button', { name: /^清理历史（/ }).click();
    await page.getByRole('heading', { name: '没有匹配的任务', exact: true }).waitFor();
    assert.equal(
      await page.evaluate(() => window.__tasks.filter((t) => t.id.startsWith('history-')).length),
      0,
    );
    assert.equal(await page.evaluate(() => window.__tasks.some((t) => t.id === 'queue')), true);
    assert.deepEqual(errors, []);
    console.log(
      `Task center: ${collapsedHeight}px desktop rows; filters, actions, artifact cleanup, preview return via close/Escape/backdrop, scroll/focus preservation and narrow layout passed.`,
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
