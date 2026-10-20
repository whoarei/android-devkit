// Serve the production frontend on a separate port; fixtures never access real devices.
const { spawn, execFileSync } = require('node:child_process');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const url = 'http://127.0.0.1:19328';
const server = spawn(
  process.execPath,
  [
    path.join(root, 'node_modules/vite/bin/vite.js'),
    'preview',
    '--host',
    '127.0.0.1',
    '--port',
    '19328',
    '--strictPort',
  ],
  { cwd: root, stdio: 'inherit', windowsHide: true },
);
let serverError;
server.on('error', (error) => {
  serverError = error;
});
(async () => {
  try {
    let ready = false;
    for (let attempt = 0; attempt < 60; attempt++) {
      if (serverError) throw serverError;
      if (server.exitCode !== null) throw Error(`Preview server exited: ${server.exitCode}`);
      try {
        ready = (await fetch(url, { signal: AbortSignal.timeout(1000) })).ok;
      } catch {}
      if (ready) break;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    if (!ready) throw Error('Production preview did not become ready');
    for (const script of [
      'check-network-ui.cjs',
      'check-device-ui.cjs',
      'check-updater-ui.cjs',
      'check-task-center-ui.cjs',
      'check-sdk-avd-ui.cjs',
    ]) {
      execFileSync(process.execPath, [path.join(__dirname, script)], {
        cwd: root,
        stdio: 'inherit',
        windowsHide: true,
        env: { ...process.env, ANDROID_DEVKIT_UI_URL: url },
      });
    }
  } finally {
    server.kill();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
