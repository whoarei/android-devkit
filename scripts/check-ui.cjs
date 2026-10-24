const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');
const { chromium } = require('playwright');
(async () => {
  const output = path.resolve(__dirname, '../.qa/app');
  fs.mkdirSync(output, { recursive: true });
  for (let i = 0; i < 30; i++) {
    try {
      if ((await fetch('http://127.0.0.1:9224/json/version')).ok) break;
    } catch {}
    await new Promise((r) => setTimeout(r, 300));
  }
  const browser = await chromium.connectOverCDP('http://127.0.0.1:9224');
  let page;
  for (let i = 0; i < 30; i++) {
    page = browser
      .contexts()
      .flatMap((c) => c.pages())
      .find((p) => /tauri|localhost/.test(p.url()));
    if (page) break;
    await new Promise((r) => setTimeout(r, 300));
  }
  assert(page, 'desktop WebView target');
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.waitForSelector('h1');
  await page.getByText('SDK 已就位', { exact: true }).waitFor({ timeout: 30000 });
  await page.screenshot({ path: path.join(output, 'devices.png') });
  assert.equal(await page.locator('h1').innerText(), '设备工作台');
  await page.getByRole('button', { name: '新建模拟器', exact: true }).click();
  await page.getByRole('button', { name: /下一步/ }).click();
  await page.getByRole('button', { name: /下一步/ }).click();
  await page.getByLabel('内部名称', { exact: true }).fill('../unsafe');
  await page.getByRole('button', { name: '查看创建计划', exact: true }).click();
  await page.getByRole('alert').filter({ hasText: '内部名称限' }).waitFor();
  await page.screenshot({ path: path.join(output, 'create.png') });
  await page.getByRole('button', { name: '关闭面板', exact: true }).click();
  await page.getByRole('button', { name: '关闭错误', exact: true }).click();
  await page.getByRole('button', { name: '连接设备', exact: true }).click();
  await page.getByLabel('设备 IP 地址', { exact: true }).fill('not-an-ip');
  await page.getByLabel('配对端口', { exact: true }).fill('1234');
  await page.getByLabel('6 位配对码', { exact: true }).fill('123456');
  await page.getByRole('button', { name: '配对', exact: true }).click();
  await page.getByRole('alert').filter({ hasText: '请输入有效 IP' }).waitFor();
  await page.getByRole('button', { name: '关闭面板', exact: true }).click();
  await page.getByRole('button', { name: '关闭错误', exact: true }).click();
  await page.locator('nav').getByRole('button', { name: 'SDK 管理', exact: true }).click();
  await page.screenshot({ path: path.join(output, 'sdk.png') });
  const catalogResult = await page.evaluate(async () =>
    window.__TAURI_INTERNALS__.invoke('catalog_refresh'),
  );
  await page.getByRole('button', { name: '下载源与站点', exact: true }).click();
  await page.getByLabel('启用 Android SDK', { exact: true }).waitFor();
  assert.equal(await page.locator('.sdk-site').count(), 4);
  await page.screenshot({ path: path.join(output, 'sdk-sites-native.png') });
  await page.getByRole('button', { name: 'SDK Tools', exact: true }).click();
  await page.getByRole('textbox', { name: '搜索组件', exact: true }).fill('platform-tools');
  await page.getByLabel('platform-tools 变更', { exact: true }).waitFor();
  await page.getByRole('textbox', { name: '搜索组件', exact: true }).fill('');
  await page.getByRole('button', { name: 'SDK Platforms', exact: true }).click();
  await page.locator('.sdk-group-heading').first().waitFor();
  await page.screenshot({ path: path.join(output, 'sdk-platforms-native.png') });
  await page.getByRole('button', { name: '刷新', exact: true }).waitFor();
  await page.locator('nav').getByRole('button', { name: '环境诊断', exact: true }).click();
  await page.getByRole('heading', { name: '硬件加速', exact: true }).waitFor({ timeout: 30000 });
  await page.screenshot({ path: path.join(output, 'diagnostics.png') });
  await page.locator('nav').getByRole('button', { name: '设置', exact: true }).click();
  assert.equal(
    await page.getByLabel('当前 SDK 根目录', { exact: true }).inputValue(),
    'E:\\Android\\sdk',
  );
  await page.screenshot({ path: path.join(output, 'settings.png') });
  const snapshot = await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke('workspace_snapshot'),
  );
  assert.equal(snapshot.settings.sdkRoot, 'E:\\Android\\sdk');
  assert(snapshot.packages.some((p) => p.id === 'platform-tools' && p.installed));
  assert(snapshot.packages.some((p) => !p.installed));
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
  assert(!overflow);
  assert.deepEqual(errors, []);
  const report = {
    sdk: snapshot.settings.sdkRoot,
    avds: snapshot.avds.map((a) => a.name),
    packages: snapshot.packages.length,
    devices: snapshot.devices.length,
    catalogWarnings: catalogResult,
    invalidAvdRejected: true,
    invalidWirelessRejected: true,
    overflow,
    errors,
  };
  fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  await browser.close();
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
