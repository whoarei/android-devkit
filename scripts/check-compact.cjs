const fs = require('fs');
const assert = require('assert/strict');
const { chromium } = require('playwright');
(async () => {
  const b = await chromium.connectOverCDP('http://127.0.0.1:9224');
  const page = b.contexts()[0].pages()[0];
  const cdp = await page.context().newCDPSession(page);
  try {
    await page.locator('nav').getByRole('button', { name: '设备工作台', exact: true }).click();
    const widths = [];
    for (const width of [960, 640, 480]) {
      await cdp.send('Emulation.setDeviceMetricsOverride', {
        width,
        height: 800,
        deviceScaleFactor: 1,
        mobile: false,
      });
      await page.waitForTimeout(200);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
      assert(!overflow, `overflow at ${width}px`);
      await page.screenshot({ path: `.qa/app/compact-${width}.png` });
      widths.push({ width, overflow });
    }
    await page.getByRole('button', { name: '新建模拟器', exact: true }).click();
    await page.getByRole('button', { name: /下一步/ }).click();
    await page.getByRole('button', { name: /下一步/ }).click();
    assert(
      !(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)),
      'compact wizard overflow',
    );
    await page.screenshot({ path: '.qa/app/compact-wizard.png' });
    await page.getByRole('button', { name: '关闭面板', exact: true }).click();
    fs.writeFileSync(
      '.qa/app/compact-report.json',
      JSON.stringify({ widths, wizardOverflow: false }, null, 2),
    );
    console.log(JSON.stringify({ widths, wizardOverflow: false }));
  } finally {
    await cdp.send('Emulation.clearDeviceMetricsOverride');
    await b.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
