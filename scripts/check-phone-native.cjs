// Opt-in native WebView validation. Requires a QA build with a distinct application identifier.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { createHash } = require('node:crypto');
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
(async () => {
  const metadata = JSON.parse(
    fs.readFileSync('.qa/phase2-ui/native-process.json', 'utf8').replace(/^\uFEFF/, ''),
  );
  assert(path.resolve(metadata.dataDirectory).startsWith(path.resolve('.qa/phase2-ui') + path.sep));
  let ready = false;
  for (let i = 0; i < 120; i++) {
    try {
      ready = (
        await fetch(`http://127.0.0.1:${metadata.debugPort}/json/version`, {
          signal: AbortSignal.timeout(1000),
        })
      ).ok;
    } catch {}
    if (ready) break;
    await sleep(250);
  }
  assert(ready, 'QA WebView debugging endpoint');
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${metadata.debugPort}`);
  try {
    let page;
    for (let attempt = 0; attempt < 120; attempt++) {
      page = browser
        .contexts()
        .flatMap((context) => context.pages())
        .find((p) => /tauri|localhost/.test(p.url()));
      if (page) break;
      await sleep(250);
    }
    assert(page, 'QA WebView');
    await page.waitForFunction(() => !!window.__TAURI_INTERNALS__);
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    const invoke = (command, args = {}) =>
      page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), {
        command,
        args,
      });
    const initial = await invoke('workspace_snapshot');
    assert.equal(initial.setupRequired, true);
    assert.deepEqual(initial.tasks, []);
    const avdRoot = path.join(metadata.dataDirectory, 'avd');
    fs.mkdirSync(avdRoot, { recursive: true });
    await invoke('settings_save', {
      settings: { ...initial.settings, sdkRoot: 'E:\\Android\\sdk', avdRoots: [avdRoot] },
    });
    await invoke('java_apply', {
      path: 'C:\\Program Files\\Android\\Android Studio\\jbr\\bin\\java.exe',
    });
    const firstRun = page.getByRole('button', { name: '稍后配置', exact: true });
    if (await firstRun.isVisible()) await firstRun.click();
    const snapshot = await invoke('workspace_snapshot');
    const device = snapshot.devices.find(
      (d) =>
        d.serial === metadata.emulatorSerial &&
        d.avdName === metadata.avdName &&
        d.state === 'ready',
    );
    assert(device, 'Exact disposable QA emulator');
    const target = {
      serial: device.serial,
      transportId: device.transportId,
      serverEpoch: device.serverEpoch,
    };
    const execute = async (kind, args) => {
      const plan = await invoke('plan_create', { operation: { kind, args, target } });
      const submit = {
        planId: plan.id,
        planHash: plan.hash,
        requestId: require('node:crypto').randomUUID(),
        confirmationToken: plan.confirmationToken,
        acceptedLicenses: [],
      };
      const task = await invoke('task_submit', submit);
      assert.equal(
        (await invoke('task_submit', submit)).id,
        task.id,
        'Repeated requestId returns the original task',
      );
      const deadline = Date.now() + 60000;
      while (Date.now() < deadline) {
        const current = (await invoke('workspace_snapshot')).tasks.find((t) => t.id === task.id);
        if (!['queued', 'running', 'waiting_confirmation'].includes(current.state)) {
          assert.equal(current.state, 'succeeded', `${current.message}\n${current.log.join('\n')}`);
          return current;
        }
        await sleep(250);
      }
      throw Error(`Native ${kind} timeout`);
    };
    assert.equal(await invoke('apk_set_tools'), '');
    const apk = await invoke('apk_set_prepare', {
      paths: ['base.apk', 'config.zh.apk'].map((name) =>
        path.resolve('.qa/runtime/fixture/splits', name),
      ),
    });
    await execute('apk_set_install', { setId: apk.id });
    await execute('app_launch', { package: apk.package });
    const video = await execute('screen_record', { seconds: '3', resolution: '720p' });
    const data = await invoke('video_preview', { artifactId: video.id });
    assert(data.startsWith('data:video/mp4;base64,'));
    // The real artifact inbox opens a video element under the application's production CSP.
    await page.getByRole('button', { name: '任务中心', exact: true }).click();
    await page.getByRole('button', { name: /待保存文件：/ }).click();
    const inbox = page.getByRole('dialog', { name: '待保存文件', exact: true });
    await inbox.getByRole('button', { name: '预览', exact: true }).click();
    const preview = page.getByRole('dialog', { name: '录屏预览', exact: true });
    await preview.locator('video').evaluate(
      (video) =>
        new Promise((resolve, reject) => {
          if (video.readyState >= 1) return resolve();
          video.onloadedmetadata = resolve;
          video.onerror = () => reject(Error('Native video decode failed'));
        }),
    );
    const media = await preview.locator('video').evaluate(async (video) => {
      video.muted = true;
      await video.play();
      await new Promise((resolve) => video.requestVideoFrameCallback(resolve));
      return { width: video.videoWidth, height: video.videoHeight, duration: video.duration };
    });
    assert(media.width > 0 && media.height > 0 && media.duration >= 2);
    await page.screenshot({ path: '.qa/phase2-ui/native-video.png' });
    await preview.getByRole('button', { name: '关闭面板' }).click();
    await inbox.getByRole('button', { name: '关闭面板' }).click();
    const source = path.join(
      metadata.dataDirectory,
      "中文 native'$()目录-" + path.basename(path.dirname(metadata.dataDirectory)),
    );
    fs.mkdirSync(source, { recursive: true });
    fs.writeFileSync(path.join(source, '.hidden'), 'native roundtrip');
    const transfer = await invoke('transfer_prepare', {
      direction: 'upload',
      paths: [source],
      target,
      directory: '/sdcard/Download',
    });
    await execute('file_upload', { transferId: transfer.id });
    const destination = path.join(metadata.dataDirectory, 'download');
    fs.mkdirSync(destination);
    const download = await invoke('transfer_prepare', {
      direction: 'download',
      paths: transfer.roots.map((r) => `/sdcard/Download/${r}`),
      target,
      directory: destination,
    });
    await execute('file_download', { transferId: download.id });
    const hash = (file) => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
    assert.equal(
      hash(path.join(source, '.hidden')),
      hash(path.join(destination, path.basename(source), '.hidden')),
    );
    const rename = await invoke('transfer_prepare', {
      direction: 'upload',
      paths: [source],
      target,
      directory: '/sdcard/Download',
    });
    await execute('file_upload', {
      transferId: rename.id,
      conflicts: 'rename',
      renames: JSON.stringify({ [path.basename(source)]: path.basename(source) + ' copy' }),
    });
    fs.writeFileSync(path.join(source, '.hidden'), 'explicitly replaced native data');
    const replace = await invoke('transfer_prepare', {
      direction: 'upload',
      paths: [source],
      target,
      directory: '/sdcard/Download',
    });
    await execute('file_upload', { transferId: replace.id, conflicts: 'replace' });
    const replacementDestination = path.join(metadata.dataDirectory, 'replacement-download');
    fs.mkdirSync(replacementDestination);
    const replacementDownload = await invoke('transfer_prepare', {
      direction: 'download',
      paths: [`/sdcard/Download/${path.basename(source)}`],
      target,
      directory: replacementDestination,
    });
    await execute('file_download', { transferId: replacementDownload.id });
    assert.equal(
      hash(path.join(source, '.hidden')),
      hash(path.join(replacementDestination, path.basename(source), '.hidden')),
    );
    const cancellationSource = path.join(
      metadata.dataDirectory,
      'cancel-native-' + path.basename(path.dirname(metadata.dataDirectory)),
    );
    fs.mkdirSync(cancellationSource);
    for (let i = 0; i < 64; i++)
      fs.writeFileSync(path.join(cancellationSource, `${i}.bin`), Buffer.alloc(65536, i));
    const cancelSet = await invoke('transfer_prepare', {
      direction: 'upload',
      paths: [cancellationSource],
      target,
      directory: '/sdcard/Download',
    });
    const cancelPlan = await invoke('plan_create', {
      operation: { kind: 'file_upload', args: { transferId: cancelSet.id }, target },
    });
    const cancelTask = await invoke('task_submit', {
      planId: cancelPlan.id,
      planHash: cancelPlan.hash,
      requestId: require('node:crypto').randomUUID(),
      confirmationToken: cancelPlan.confirmationToken,
      acceptedLicenses: [],
    });
    let cancelled;
    for (let i = 0; i < 100; i++) {
      const task = (await invoke('workspace_snapshot')).tasks.find((t) => t.id === cancelTask.id);
      if (task.phase === 'transferring') {
        await sleep(150);
        await invoke('task_cancel', { taskId: task.id });
        break;
      }
      await sleep(50);
    }
    for (let i = 0; i < 100; i++) {
      const task = (await invoke('workspace_snapshot')).tasks.find((t) => t.id === cancelTask.id);
      if (!['queued', 'running'].includes(task.state)) {
        cancelled = task;
        break;
      }
      await sleep(50);
    }
    assert.equal(cancelled?.state, 'cancelled');
    assert(
      (await invoke('device_files', { target, path: '/sdcard/Download' })).every(
        (e) => e.name !== path.basename(cancellationSource),
      ),
    );
    await execute('file_cleanup', { sourceTaskId: cancelTask.id });
    const artifacts = await invoke('artifacts_list');
    await invoke('artifacts_discard', { artifactIds: artifacts.map((a) => a.id) });
    assert.deepEqual(await invoke('artifacts_list'), []);
    assert.deepEqual(errors, []);
    fs.writeFileSync(
      '.qa/phase2-ui/native-report.json',
      JSON.stringify(
        {
          isolatedProfile: true,
          distinctApplicationId: 'dev.android-devkit.qa.phase2',
          target,
          nativeSplitInstallation: true,
          requestIdIdempotency: true,
          nativeRecordingAndVideoPreview: media,
          nativeFileRoundtripSha256: true,
          renameAndReplacement: true,
          cancellationBeforePublish: true,
          stagingCleanup: true,
          videoCacheDiscard: true,
          errors,
        },
        null,
        2,
      ),
    );
    await invoke('close_resolve', {
      mode: 'exit',
      discard: false,
      stopRecordings: false,
      discardIds: [],
    }).catch(() => {});
    console.log(
      'Native Phase 2: actual IPC/permissions, split installation, MP4 decode/playback under CSP, shared-storage transfer with Unicode/.hidden, SHA-256 roundtrip and video cache cleanup passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
