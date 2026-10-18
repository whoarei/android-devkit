// Uses only a disposable AVD under .qa/runtime; existing AVDs are never started or edited.
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');
(async () => {
  const b = await chromium.connectOverCDP('http://127.0.0.1:9224');
  const p = b.contexts()[0].pages()[0];
  const errors = [];
  p.on('pageerror', (e) => errors.push(e.message));
  const invoke = (c, a = {}) =>
    p.evaluate(({ c, a }) => window.__TAURI_INTERNALS__.invoke(c, a), { c, a });
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const initial = (await invoke('workspace_snapshot')).settings;
  let avd;
  let settings;
  let result;
  async function task(kind, args = {}, target = null) {
    const plan = await invoke('plan_create', { operation: { kind, args, target } });
    assert.equal(plan.licenses.length, 0, 'test must reuse installed image');
    const t = await invoke('task_submit', {
      planId: plan.id,
      planHash: plan.hash,
      requestId: crypto.randomUUID(),
      confirmationToken: plan.confirmationToken,
      acceptedLicenses: [],
    });
    return finish(t.id);
  }
  async function finish(id) {
    const deadline = Date.now() + 300000;
    while (Date.now() < deadline) {
      const snap = await invoke('workspace_snapshot');
      const t = snap.tasks.find((t) => t.id === id);
      if (!['queued', 'running', 'waiting_confirmation'].includes(t.state)) {
        assert.equal(t.state, 'succeeded', JSON.stringify(t));
        return t;
      }
      await wait(500);
    }
    throw Error('Task timeout ' + id);
  }
  try {
    settings = { ...initial, avdRoots: [...initial.avdRoots, path.resolve('.qa/runtime/ui-avd')] };
    await invoke('settings_save', { settings });
    const name = 'DevKit_UI_M4_' + Date.now();
    await task('avd_create', {
      name,
      displayName: name,
      profile: 'pixel_2',
      image: 'system-images;android-29;default;x86_64',
      memory: '2048',
      storage: '2',
      gpu: 'auto',
    });
    avd = (await invoke('workspace_snapshot')).avds.find((a) => a.name === name);
    await task('avd_start', { avdId: avd.id });
    const device = (await invoke('workspace_snapshot')).devices.find(
      (d) => d.avdName === name && d.state === 'ready',
    );
    assert(device);
    const target = {
      serial: device.serial,
      transportId: device.transportId,
      serverEpoch: device.serverEpoch,
    };
    const apk = await invoke('apk_prepare', {
      path: path.resolve('.qa/runtime/fixture/fixture.apk'),
    });
    await task('apk_install', { apkId: apk.id }, target);
    await task('app_launch', { package: apk.package }, target);
    await p.locator('nav').getByRole('button', { name: '设备工作台', exact: true }).click();
    await p.getByText(name, { exact: true }).first().click();
    await p.getByRole('button', { name: '开始查看', exact: true }).click();
    await p.getByRole('textbox', { name: '日志应用包名', exact: true }).fill(apk.package);
    await p.getByRole('textbox', { name: '日志 Tag', exact: true }).fill('DevKitFixture');
    await p.locator('.log-lines').filter({ hasText: 'heartbeat' }).waitFor({ timeout: 20000 });
    await p.getByRole('button', { name: '暂停视图', exact: true }).click();
    const paused = await p.locator('.log-lines').innerText();
    await wait(1500);
    assert.equal(await p.locator('.log-lines').innerText(), paused);
    await p.getByRole('button', { name: '恢复视图', exact: true }).click();
    await p.getByRole('button', { name: '清空视图', exact: true }).click();
    const buffer = await invoke('log_read', { target, action: 'read', filter: {} });
    assert(buffer.buffered > 0, 'clear only advances view cursor');
    await p.locator('.log-lines').filter({ hasText: 'heartbeat' }).waitFor();
    await p.locator('.log-panel').screenshot({ path: '.qa/app/m4-live-logs.png' });
    await p.getByRole('button', { name: '文件采集…', exact: true }).click();
    await p.getByLabel('采集总配额', { exact: true }).fill('8');
    await p.getByLabel('采集时限', { exact: true }).fill('1');
    await p.getByRole('button', { name: '查看采集计划', exact: true }).click();
    await p
      .getByRole('dialog', { name: '开始文件采集', exact: true })
      .getByRole('button', { name: '开始文件采集', exact: true })
      .click();
    let recording;
    for (let i = 0; i < 30; i++) {
      recording = (await invoke('workspace_snapshot')).tasks.find(
        (t) => t.operation.kind === 'log_record' && t.phase === 'recording',
      );
      if (recording) break;
      await wait(200);
    }
    assert(recording);
    let recordedBytes = 0;
    for (let i = 0; i < 40; i++) {
      const manifest = await invoke('recording_segments', { taskId: recording.id });
      recordedBytes = manifest.bytes;
      if (recordedBytes > 0) break;
      await wait(500);
    }
    assert(recordedBytes > 0, 'recording must contain real log bytes before stop');
    await p.locator('nav').getByRole('button', { name: '任务中心', exact: true }).click();
    const card = p.locator('.task-card').filter({ hasText: '开始文件采集' }).first();
    await card.getByRole('button', { name: '取消', exact: true }).click();
    await finish(recording.id);
    await card.getByRole('button', { name: '选择采集段导出', exact: true }).click();
    await card.getByRole('button', { name: '预览所选采集段', exact: true }).click();
    const exportDialog = p.getByRole('dialog', { name: '导出内容预览', exact: true });
    await exportDialog.waitFor();
    await exportDialog.locator('.export-preview').filter({ hasText: 'heartbeat' }).waitFor();
    assert(!(await exportDialog.innerText()).includes('fixture-secret'));
    await exportDialog.screenshot({ path: '.qa/app/m4-recording-export.png' });
    await exportDialog.getByRole('button', { name: '关闭面板', exact: true }).click();
    assert.deepEqual(errors, []);
    result = {
      liveDevice: target.serial,
      applicationFilter: true,
      pauseKeepsView: true,
      clearPreservesDeviceBuffer: true,
      recordingSurvivesPageChange: true,
      cancelSealsRecording: true,
      segmentExportRedacted: true,
      recordedBytes,
      errors,
    };
  } catch (error) {
    console.error('Live UI failure:', error);
    await p.screenshot({ path: '.qa/app/m4-live-failure.png' });
    throw error;
  } finally {
    if (avd) {
      let snapshot = await invoke('workspace_snapshot');
      const devices = snapshot.devices.filter((d) => d.avdName === avd.name).map((d) => d.serial);
      for (const task of snapshot.tasks.filter(
        (t) =>
          t.operation.kind === 'log_record' &&
          devices.includes(t.operation.target?.serial) &&
          ['queued', 'running', 'waiting_confirmation'].includes(t.state),
      )) {
        await invoke('task_cancel', { taskId: task.id });
        await finish(task.id);
      }
      snapshot = await invoke('workspace_snapshot');
      if (snapshot.devices.some((d) => d.avdName === avd.name))
        await task('avd_stop', { avdId: avd.id });
      await task('avd_delete', { avdId: avd.id });
      await p.locator('nav').getByRole('button', { name: '设备工作台', exact: true }).click();
      await p.locator('.detail-panel').waitFor({ state: 'detached' });
    }
    await invoke('settings_save', { settings: initial });
    await b.close();
  }
  result.staleSelectionCleared = true;
  result.testAvdDeleted = true;
  fs.writeFileSync('.qa/app/m4-live-report.json', JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
