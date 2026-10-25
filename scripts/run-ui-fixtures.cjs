// Each run owns its preview server and port; fixtures never access real devices.
const { spawn, execFileSync } = require('node:child_process');
const net = require('node:net');
const path = require('node:path');
const { stripVTControlCharacters } = require('node:util');
const root = path.resolve(__dirname, '..');
(async () => {
  const port = await new Promise((resolve, reject) => {
    const reservation = net.createServer();
    reservation.once('error', reject);
    reservation.listen(0, '127.0.0.1', () => {
      const port = reservation.address().port;
      reservation.close((error) => (error ? reject(error) : resolve(port)));
    });
  });
  const url = `http://127.0.0.1:${port}`;
  const server = spawn(
    process.execPath,
    [
      path.join(root, 'node_modules/vite/bin/vite.js'),
      'preview',
      '--host',
      '127.0.0.1',
      '--port',
      String(port),
      '--strictPort',
    ],
    { cwd: root, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true },
  );
  let startupError;
  let listening = false;
  let output = '';
  server.once('error', (error) => {
    startupError = error;
  });
  server.stdout.on('data', (chunk) => {
    output += chunk;
    listening ||= stripVTControlCharacters(output).includes(url);
  });
  server.stderr.on('data', (chunk) => {
    output += chunk;
  });
  try {
    let ready = false;
    for (let attempt = 0; attempt < 60; attempt++) {
      if (startupError) throw startupError;
      if (server.exitCode !== null)
        throw Error(`Preview server exited: ${server.exitCode}\n${output}`);
      if (listening) {
        try {
          ready = (await fetch(url, { signal: AbortSignal.timeout(1000) })).ok;
        } catch {}
      }
      if (ready) break;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    if (!ready) throw Error(`Production preview did not become ready\n${output}`);
    const requested = process.argv.slice(2);
    for (const script of requested.length
      ? requested
      : [
          'preferences',
          'network',
          'device',
          'terminal',
          'updater',
          'task-center',
          'sdk-avd',
          'sdk-mirror',
          'java',
          'p0',
          'phone',
        ]) {
      execFileSync(process.execPath, [path.join(__dirname, `check-${script}-ui.cjs`)], {
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
