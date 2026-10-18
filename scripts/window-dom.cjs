const fs = require('fs');
const { chromium } = require('playwright');
(async () => {
  const browser = await chromium.connectOverCDP('http://127.0.0.1:9224');
  try {
    const page = browser.contexts()[0].pages()[0];
    const value = await page.evaluate(() => {
      const r = document
        .querySelector('.window-buttons button:nth-child(2)')
        .getBoundingClientRect();
      return {
        rect: { x: r.x, y: r.y, width: r.width, height: r.height },
        scale: devicePixelRatio,
      };
    });
    fs.writeFileSync('.qa/app/window-dom.json', JSON.stringify(value));
    const before = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('window_state'));
    await page.evaluate(() =>
      window.__TAURI_INTERNALS__.invoke('window_action', { action: 'maximize' }),
    );
    const after = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('window_state'));
    if (before.maximized === after.maximized) throw new Error('maximize state did not change');
    await page.evaluate(() =>
      window.__TAURI_INTERNALS__.invoke('window_action', { action: 'maximize' }),
    );
    console.log(JSON.stringify({ maximizeToggle: true, scale: value.scale }));
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
