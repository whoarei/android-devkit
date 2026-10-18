const { chromium } = require('playwright');
const path = require('path');
(async () => {
  const b = await chromium.connectOverCDP('http://127.0.0.1:9224');
  try {
    const p = b.contexts()[0].pages()[0];
    const invoke = (c, a = {}) =>
      p.evaluate(({ c, a }) => window.__TAURI_INTERNALS__.invoke(c, a), { c, a });
    const root = path.resolve('.qa/runtime/ui-avd');
    let snap = await invoke('workspace_snapshot');
    const own = snap.avds.filter(
      (a) =>
        path.resolve(a.dataPath).startsWith(root + path.sep) && a.name.startsWith('DevKit_UI_M4_'),
    );
    const names = own.map((a) => a.name);
    const serials = snap.devices.filter((d) => names.includes(d.avdName)).map((d) => d.serial);
    for (const t of snap.tasks.filter(
      (t) =>
        t.operation.kind === 'log_record' &&
        serials.includes(t.operation.target?.serial) &&
        ['queued', 'running', 'waiting_confirmation'].includes(t.state),
    )) {
      await invoke('task_cancel', { taskId: t.id });
      for (let i = 0; i < 100; i++) {
        snap = await invoke('workspace_snapshot');
        if (
          !['queued', 'running', 'waiting_confirmation'].includes(
            snap.tasks.find((x) => x.id === t.id).state,
          )
        )
          break;
        await new Promise((r) => setTimeout(r, 100));
      }
    }
    async function run(kind, a) {
      const plan = await invoke('plan_create', { operation: { kind, args: a, target: null } });
      const t = await invoke('task_submit', {
        planId: plan.id,
        planHash: plan.hash,
        requestId: crypto.randomUUID(),
        confirmationToken: plan.confirmationToken,
        acceptedLicenses: [],
      });
      for (let i = 0; i < 180; i++) {
        const s = await invoke('workspace_snapshot');
        const task = s.tasks.find((x) => x.id === t.id);
        if (!['queued', 'running', 'waiting_confirmation'].includes(task.state)) {
          if (task.state !== 'succeeded') throw Error(task.message);
          return;
        }
        await new Promise((r) => setTimeout(r, 500));
      }
      throw Error('cleanup timeout');
    }
    for (const a of own) {
      snap = await invoke('workspace_snapshot');
      if (snap.devices.some((d) => d.avdName === a.name)) await run('avd_stop', { avdId: a.id });
      await run('avd_delete', { avdId: a.id });
    }
    snap = await invoke('workspace_snapshot');
    await invoke('settings_save', {
      settings: {
        ...snap.settings,
        avdRoots: snap.settings.avdRoots.filter((r) => path.resolve(r) !== root),
      },
    });
    console.log(JSON.stringify({ deletedTestAvds: names }));
  } finally {
    await b.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
