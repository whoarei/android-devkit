const { chromium } = require('playwright');
(async () => {
  const b = await chromium.connectOverCDP('http://127.0.0.1:9224');
  try {
    const p = b.contexts()[0].pages()[0];
    await p
      .evaluate(async (discard) => {
        const s = await window.__TAURI_INTERNALS__.invoke('close_status');
        return window.__TAURI_INTERNALS__.invoke('close_resolve', {
          mode: 'exit',
          discard,
          discardIds: s.unsaved.map((a) => a.id),
        });
      }, process.argv.includes('--discard'))
      .catch(() => {});
  } finally {
    await b.close().catch(() => {});
  }
})().catch(() => process.exit(1));
