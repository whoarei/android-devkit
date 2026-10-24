// Isolated browser fixtures; never invokes SDK tools or changes user AVDs.
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
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.addInitScript(() => {
      const pkg = (id, name, extra = {}) => ({
        id,
        name,
        version: '1.0.0',
        installed: false,
        installedVersion: '',
        category: id.startsWith('system-images;') ? 'image' : 'tool',
        api: id.match(/android-(\d+)/)?.[1] || '',
        abi: '',
        tag: '',
        size: 1024,
        dependencies: [],
        dependencyMinVersions: {},
        obsolete: false,
        channel: 0,
        sourceUrl: '',
        ...extra,
      });
      window.__fixture = {
        settings: {
          sdkRoot: 'E:\\SDK',
          avdRoots: ['E:\\AVD'],
          javaPath: '',
          proxy: '',
          bootTimeoutSecs: 180,
          trayEnabled: false,
          sdkSites: [],
          sdkChannel: 0,
        },
        environment: { adb: 'fixture', warnings: [] },
        devices: [],
        tasks: [],
        server: 'fixture',
        serverEpoch: 1,
        freeBytes: 20 * 1024 ** 3,
        packages: [
          pkg('platforms;android-35', 'Android SDK Platform 35', {
            installed: true,
            installedVersion: '1.0.0',
          }),
          pkg('sources;android-35', 'Sources for Android 35'),
          pkg('system-images;android-35;google_apis;x86_64', 'Google APIs x86_64 System Image', {
            abi: 'x86_64',
            installed: true,
            installedVersion: '1.0.0',
          }),
          pkg('system-images;android-35;default;x86_64', 'Intel x86_64 System Image'),
          pkg('system-images;android-35;default;arm64-v8a', 'ARM 64 v8a System Image'),
          pkg('system-images;android-35;google_apis;arm64-v8a', 'Google APIs ARM64 System Image'),
          pkg(
            'system-images;android-35;google_apis_playstore;x86_64',
            'Google Play x86_64 System Image',
          ),
          pkg(
            'system-images;android-35;google_apis_playstore;arm64-v8a',
            'Google Play ARM64 System Image',
          ),
          pkg(
            'system-images;android-35;google_apis_playstore_ps16k;arm64-v8a',
            '16 KB Google Play ARM64 System Image',
            { tag: 'page_size_16kb' },
          ),
          pkg('system-images;android-35;vendor_custom;x86_64', 'Vendor System Image'),
          // Deliberately unordered, including a single-digit API and a minor API version.
          pkg('system-images;android-29;default;x86_64', 'Android API 29 x86_64', { api: '' }),
          pkg('system-images;android-9;default;x86_64', 'Android API 9 x86_64'),
          pkg('system-images;android-36.1;default;x86_64', 'Android API 36.1 x86_64'),
          pkg('system-images;android-36;default;x86_64', 'Android API 36 x86_64'),
          pkg('platforms;android-29', 'Android SDK Platform 29'),
          pkg('build-tools;35.0.0', 'Build-Tools 35', { version: '35.0.0' }),
          pkg('build-tools;19.0.0', 'Build-Tools 19', { obsolete: true }),
          pkg('platform-tools', 'Platform-Tools', {
            installed: true,
            installedVersion: '36.0.0',
            version: '37.0.0',
          }),
          pkg('emulator', 'Android Emulator', {
            installed: true,
            installedVersion: '38.0.0',
            version: '37.0.0',
          }),
        ],
        avds: [
          {
            id: 'fixture-avd',
            name: 'Fixture',
            displayName: 'Fixture phone',
            api: '35',
            abi: 'x86_64',
            status: 'stopped',
            fingerprint: 'original',
            memory: '2048',
            disk: '10G',
            gpu: 'auto',
            owned: true,
            dataPath: 'E:\\AVD\\Fixture.avd',
            image: 'system-images;android-35;google_apis;x86_64',
            error: '',
            serial: '',
            config: {
              'hw.cpu.ncore': '2',
              'vm.heapSize': '256',
              'hw.lcd.width': '1080',
              'hw.lcd.height': '1920',
              'hw.lcd.density': '420',
              'hw.initialOrientation': 'portrait',
              'runtime.network.speed': 'full',
              'runtime.network.latency': 'none',
              'hw.camera.back': 'emulated',
              'skin.path': 'E:\\skin',
              'custom.preserved': 'value',
            },
          },
        ],
      };
      window.__operations = [];
      window.__settingsSaved = [];
      window.__rejectPlan = false;
      window.__holdCatalog = false;
      window.__catalogCalls = 0;
      window.__catalogError = false;
      window.__catalogWarnings = [];
      window.isTauri = true;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: () => 1,
        unregisterCallback() {},
        invoke: async (command, args) => {
          if (command === 'workspace_snapshot') return structuredClone(window.__fixture);
          if (command === 'window_state') return { maximized: false };
          if (command === 'artifacts_list') return [];
          if (command === 'task_list')
            return {
              tasks: [
                {
                  id: 'localized-task',
                  title: '启动模拟器',
                  state: 'succeeded',
                  phase: 'complete',
                  message: '模拟器系统已就绪',
                  operation: { kind: 'avd_start', args: {}, target: null },
                  createdAt: 1,
                  updatedAt: 1,
                  log: [],
                  progress: null,
                  artifact: '',
                  licensePrompt: '',
                  cancellable: false,
                },
              ],
              total: 1,
              counts: { succeeded: 1 },
              offset: 0,
              pageSize: 30,
            };
          if (command === 'sdk_sites_status')
            return [
              {
                name: 'Android SDK',
                url: 'https://dl.google.com/android/repository/repository2-3.xml',
                builtin: true,
                enabled: true,
                checkedAt: 1,
                packageCount: 100,
                error: '',
              },
              ...window.__fixture.settings.sdkSites.map((site) => ({
                ...site,
                builtin: false,
                checkedAt: 1,
                packageCount: 3,
                error: '',
              })),
            ];
          if (command === 'settings_save') {
            if (args.settings.sdkSites.some((site) => !site.url.startsWith('https://')))
              throw { code: 'INVALID_SDK_SITE', message: '请输入 HTTPS 仓库地址' };
            window.__fixture.settings = structuredClone(args.settings);
            window.__settingsSaved.push(args.settings);
            return;
          }
          if (command === 'catalog_refresh') {
            window.__catalogCalls++;
            if (window.__holdCatalog)
              await new Promise((resolve) => {
                window.__releaseCatalog = resolve;
              });
            if (window.__catalogError) throw { code: 'NETWORK', message: 'Fixture refresh failed' };
            return window.__catalogWarnings;
          }
          if (command === 'plan_create') {
            window.__operations.push(args.operation);
            if (window.__rejectPlan)
              throw { code: 'INVALID_AVD_SETTING', message: 'CPU 核心数范围为 1～16' };
            return {
              id: 'fixture',
              hash: 'fixture',
              confirmationToken: 'fixture',
              title: 'Fixture plan',
              operation: args.operation,
              effects: ['fixture'],
              warnings: [],
              licenses: [],
            };
          }
          if (command.startsWith('plugin:event|')) return 1;
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    await page.getByRole('button', { name: 'SDK 管理', exact: true }).click();
    const refreshButton = page.getByRole('button', { name: '刷新', exact: true });
    const holdRefresh = () =>
      page.evaluate(() => {
        window.__holdCatalog = true;
        window.__releaseCatalog = null;
      });
    const expectRefreshing = async () => {
      await refreshButton.locator('svg.spin').waitFor();
      assert.equal(await refreshButton.getAttribute('aria-busy'), 'true');
      assert.equal(await refreshButton.isDisabled(), true);
    };
    const releaseRefresh = async () => {
      await page.waitForFunction(() => typeof window.__releaseCatalog === 'function');
      await page.evaluate(() => {
        window.__holdCatalog = false;
        window.__releaseCatalog();
      });
      await page.waitForFunction(() => {
        const button = document.querySelector('.page-actions button[aria-busy="true"]');
        return !button;
      });
      assert.equal(await refreshButton.locator('.spin').count(), 0);
      assert.equal(await refreshButton.isDisabled(), false);
    };
    await holdRefresh();
    await refreshButton.click();
    await expectRefreshing();
    await refreshButton.evaluate((button) => button.click());
    assert.equal(await page.evaluate(() => window.__catalogCalls), 1);
    // The request continues when navigating away; revisiting shows the same animation.
    await page.getByRole('button', { name: '设备工作台', exact: true }).click();
    await page.getByRole('button', { name: 'SDK 管理', exact: true }).click();
    await expectRefreshing();
    await releaseRefresh();
    assert.equal(await page.getByText('更新站点与目录已刷新', { exact: true }).count(), 0);
    await page.evaluate(() => {
      window.__catalogError = true;
    });
    await holdRefresh();
    await refreshButton.click();
    await expectRefreshing();
    await releaseRefresh();
    await page.getByRole('alert').filter({ hasText: 'Fixture refresh failed' }).waitFor();
    await page.evaluate(() => {
      window.__catalogError = false;
      window.__catalogWarnings = ['Fixture source unavailable'];
    });
    await refreshButton.click();
    await page.getByRole('alert').filter({ hasText: 'Fixture source unavailable' }).waitFor();
    assert.equal(await refreshButton.locator('.spin').count(), 0);
    await page.evaluate(() => {
      window.__catalogWarnings = [];
    });
    await refreshButton.click();
    await page.waitForFunction(
      () => !document.querySelector('.page-actions button[aria-busy="true"]'),
    );
    const group = page.locator('.sdk-group').filter({ hasText: 'Android · API 35' });
    await group.getByRole('button', { name: 'Android · API 35', exact: true }).click();
    const api35Count = await page.evaluate(
      () => window.__fixture.packages.filter((p) => p.id.split(';')[1] === 'android-35').length,
    );
    assert.equal(await group.locator('.sdk-package').count(), 2);
    assert.equal(await group.locator('.sdk-image-family').count(), 4);
    const aosp = group
      .locator('.sdk-image-family')
      .filter({ has: page.getByRole('button', { name: 'AOSP', exact: true }) });
    await aosp.getByRole('button', { name: 'AOSP', exact: true }).click();
    assert.equal(await aosp.locator('.sdk-image-abi').count(), 2);
    await aosp.getByRole('button', { name: 'x86_64', exact: true }).click();
    await aosp
      .getByLabel('system-images;android-35;default;x86_64 变更', { exact: true })
      .waitFor();
    await group.getByRole('button', { name: 'Google APIs', exact: true }).click();
    await group.getByRole('button', { name: 'Google Play', exact: true }).click();
    fs.mkdirSync(path.resolve(__dirname, '../.qa/app'), { recursive: true });
    await page.setViewportSize({ width: 1280, height: 1100 });
    await page.screenshot({ path: path.resolve(__dirname, '../.qa/app/sdk-image-hierarchy.png') });
    await page.setViewportSize({ width: 1280, height: 800 });
    await group.getByRole('button', { name: 'Android · API 35', exact: true }).click();
    assert.equal(await group.locator('.sdk-package').count(), 0);
    await page.getByRole('button', { name: '展开全部', exact: true }).click();
    assert.equal(await group.locator('.sdk-package').count(), api35Count);
    await page.getByRole('button', { name: '收起全部', exact: true }).click();
    assert.equal(await page.locator('.sdk-package').count(), 0);
    const search = page.getByRole('textbox', { name: '搜索组件', exact: true });
    await search.fill('AOSP arm64-v8a');
    assert.equal(await page.locator('.sdk-package').count(), 1);
    await page
      .getByLabel('system-images;android-35;default;arm64-v8a 变更', { exact: true })
      .waitFor();
    await search.fill('Google Play arm64-v8a');
    assert.equal(await page.locator('.sdk-package').count(), 2);
    assert.equal(await page.locator('.sdk-image-family').count(), 1);
    const play = page.locator('.sdk-image-family');
    const playChange = page.getByLabel(
      'system-images;android-35;google_apis_playstore;arm64-v8a 变更',
      { exact: true },
    );
    await playChange.selectOption('install');
    await play.getByRole('button', { name: 'Google Play', exact: true }).click();
    assert.equal(await play.locator('.sdk-package').count(), 0);
    await play.getByRole('button', { name: 'Google Play', exact: true }).click();
    assert.equal(await playChange.inputValue(), 'install');
    await page
      .getByLabel('system-images;android-35;google_apis_playstore_ps16k;arm64-v8a 变更', {
        exact: true,
      })
      .waitFor();
    await playChange.selectOption('');
    await page.getByRole('button', { name: '收起全部', exact: true }).click();
    assert.equal(await page.locator('.sdk-package').count(), 0);
    await page.getByRole('button', { name: '展开全部', exact: true }).click();
    assert.equal(await page.locator('.sdk-package').count(), 2);
    await search.fill('vendor_custom');
    await page.getByRole('button', { name: '其他镜像 · vendor_custom', exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: 'AOSP', exact: true }).count(), 0);
    await page.getByRole('button', { name: '清空组件搜索' }).click();
    await page.getByLabel('组件状态筛选', { exact: true }).selectOption('installed');
    assert.equal(await page.locator('.sdk-package').count(), 2);
    await page
      .getByLabel('system-images;android-35;google_apis;x86_64 变更', { exact: true })
      .waitFor();
    await page.getByLabel('组件状态筛选', { exact: true }).selectOption('all');
    await page.getByRole('textbox', { name: '搜索组件', exact: true }).fill('Sources');
    await page.getByLabel('sources;android-35 变更', { exact: true }).selectOption('install');
    await page.getByRole('button', { name: '清空组件搜索' }).click();
    await page.getByRole('button', { name: 'SDK Tools', exact: true }).click();
    await page.getByRole('button', { name: '展开全部', exact: true }).click();
    assert.equal(await page.locator('.sdk-package').count(), 3);
    await page.getByLabel('隐藏过时组件（保留已安装项）').uncheck();
    assert.equal(await page.locator('.sdk-package').count(), 4);
    await page.getByLabel('组件状态筛选', { exact: true }).selectOption('updates');
    assert.equal(await page.locator('.sdk-package').count(), 1);
    await page.getByLabel('platform-tools 变更', { exact: true }).selectOption('install');
    await page.getByRole('button', { name: '确认安装 2 项', exact: true }).click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    assert.deepEqual((await page.evaluate(() => window.__operations.at(-1))).args, {
      packages: 'sources;android-35\nplatform-tools',
    });
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await page.getByRole('button', { name: '清空选择', exact: true }).click();
    await page.getByLabel('组件状态筛选', { exact: true }).selectOption('all');
    const emulator = page.locator('.sdk-package').filter({ hasText: 'Android Emulator' });
    assert.equal(await emulator.getByText('38.0.0', { exact: true }).count(), 1);
    assert.equal(await emulator.getByRole('option', { name: '更新', exact: true }).count(), 0);
    const output = path.resolve(__dirname, '../.qa/app');
    fs.mkdirSync(output, { recursive: true });
    await holdRefresh();
    await refreshButton.click();
    await expectRefreshing();
    await page.screenshot({ path: path.join(output, 'sdk-refreshing.png') });
    await releaseRefresh();
    await page.screenshot({ path: path.join(output, 'sdk-tools-tree.png') });
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
      false,
    );
    await page.screenshot({ path: path.join(output, 'sdk-tree-compact.png') });
    await page.setViewportSize({ width: 1280, height: 800 });
    await holdRefresh();
    await page.getByLabel('显示预览组件').click();
    await expectRefreshing();
    assert.equal(await page.getByLabel('显示预览组件').isDisabled(), true);
    await releaseRefresh();
    assert.equal(await page.getByLabel('显示预览组件').isChecked(), true);
    assert.equal(await page.evaluate(() => window.__fixture.settings.sdkChannel), 3);
    await page.getByRole('button', { name: '下载源与站点', exact: true }).click();
    assert.equal(await page.getByLabel('启用 Android SDK', { exact: true }).isDisabled(), true);
    await page.getByRole('button', { name: '添加站点', exact: true }).click();
    let dialog = page.getByRole('dialog', { name: '添加 SDK 更新站点' });
    await dialog.getByLabel('站点名称', { exact: true }).fill('Vendor fixture');
    await dialog.getByLabel('仓库 XML 地址', { exact: true }).fill('http://example.com/sdk.xml');
    await dialog.getByRole('button', { name: '保存并刷新', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: 'HTTPS' }).waitFor();
    assert.equal(await refreshButton.locator('.spin').count(), 0);
    await dialog.getByLabel('仓库 XML 地址', { exact: true }).fill('https://example.com/sdk.xml');
    await dialog.getByRole('button', { name: '保存并刷新', exact: true }).click();
    const vendor = page.locator('.sdk-site').filter({ hasText: 'Vendor fixture' });
    await vendor.waitFor();
    await page.waitForFunction(
      () => !document.querySelector('.page-actions button[aria-busy="true"]'),
    );
    await holdRefresh();
    await vendor.getByRole('checkbox').click();
    await expectRefreshing();
    await releaseRefresh();
    await page.getByText('已停用', { exact: true }).waitFor();
    assert.equal(await vendor.getByRole('checkbox').isChecked(), false);
    await vendor.getByRole('button', { name: '编辑', exact: true }).click();
    dialog = page.getByRole('dialog', { name: '编辑 SDK 更新站点' });
    await dialog.getByLabel('站点名称', { exact: true }).fill('Vendor renamed');
    await dialog.getByRole('button', { name: '保存并刷新', exact: true }).click();
    await page
      .locator('.sdk-site')
      .filter({ hasText: 'Vendor renamed' })
      .getByRole('button', { name: '删除', exact: true })
      .click();
    await page.waitForFunction(() => window.__fixture.settings.sdkSites.length === 0);

    await page.getByRole('button', { name: '设备工作台', exact: true }).click();
    await page.locator('.device-row').filter({ hasText: 'Fixture phone' }).click();
    await page.getByRole('button', { name: '编辑配置', exact: true }).click();
    dialog = page.getByRole('dialog', { name: '编辑模拟器配置' });
    assert.equal(await dialog.getByRole('button', { name: '查看保存计划' }).isDisabled(), true);
    await dialog.getByLabel('CPU 核心数', { exact: true }).fill('4');
    await dialog.getByText('摄像头与网络', { exact: true }).click();
    await dialog.getByLabel('后置摄像头', { exact: true }).fill('virtualscene');
    await dialog.getByLabel('网络速度', { exact: true }).fill('1024:2048');
    await dialog.getByLabel('网络延迟', { exact: true }).fill('20:100');
    await dialog.getByText('启动与存储', { exact: true }).click();
    assert.equal(await dialog.getByLabel('数据分区目标容量', { exact: true }).inputValue(), '10');
    await dialog.getByLabel('数据分区目标容量单位', { exact: true }).selectOption('MiB');
    assert.equal(
      await dialog.getByLabel('数据分区目标容量', { exact: true }).inputValue(),
      '10240',
    );
    await dialog.getByLabel('默认启动方式', { exact: true }).selectOption('cold');
    await dialog.getByLabel('扩展存储', { exact: true }).selectOption('size');
    await dialog.getByLabel('扩展存储容量', { exact: true }).fill('768');
    await dialog.getByText('设备皮肤与边框', { exact: true }).click();
    await dialog.getByLabel('设备皮肤', { exact: true }).selectOption('none');
    assert.equal(await dialog.getByLabel('显示设备边框', { exact: true }).isDisabled(), true);
    await page.screenshot({ path: path.join(output, 'avd-advanced-editor.png') });
    await dialog.getByRole('button', { name: '查看保存计划' }).click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.__operations.at(-1)), {
      kind: 'avd_edit',
      target: null,
      args: {
        avdId: 'fixture-avd',
        fingerprint: 'original',
        cpuCores: '4',
        cameraBack: 'virtualscene',
        networkSpeed: '1024:2048',
        networkLatency: '20:100',
        bootMode: 'cold',
        sdCardMode: 'size',
        sdCardMiB: '768',
        skinMode: 'none',
      },
    });
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await page.evaluate(() => {
      window.__rejectPlan = true;
    });
    await dialog.getByRole('button', { name: '查看保存计划' }).click();
    await dialog.getByRole('alert').filter({ hasText: '1～16' }).waitFor();
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(await dialog.evaluate((el) => el.scrollWidth > el.clientWidth), false);
    await page.screenshot({ path: path.join(output, 'avd-editor-compact.png') });
    await page.evaluate(() => {
      window.__fixture.avds[0].fingerprint = 'external-change';
    });
    // Refocus invalidates the workspace query, as when another tool changed config.ini.
    await page.evaluate(() => {
      window.dispatchEvent(new Event('blur'));
      window.dispatchEvent(new Event('focus'));
    });
    await dialog.getByRole('alert').filter({ hasText: '其他工具中更新' }).waitFor();
    assert.equal(await dialog.getByRole('button', { name: '查看保存计划' }).isDisabled(), true);
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.evaluate(() => {
      window.__rejectPlan = false;
    });
    await page.getByRole('button', { name: '新建模拟器', exact: true }).click();
    dialog = page.getByRole('dialog', { name: '新建模拟器' });
    await dialog.getByRole('button', { name: '平板', exact: false }).click();
    await dialog.getByRole('button', { name: '下一步', exact: true }).click();
    assert.deepEqual(await dialog.locator('.image-api-heading > b').allTextContents(), [
      'Android · API 36.1',
      'Android · API 36',
      'Android · API 35',
      'Android · API 29',
      'Android · API 9',
    ]);
    const installedImages = dialog.getByRole('region', { name: 'Android · API 35', exact: true });
    assert.equal(await installedImages.getByRole('button').getAttribute('aria-expanded'), 'true');
    assert.equal(await installedImages.getByRole('radio').count(), 4);
    assert.equal(await installedImages.locator('input:checked').count(), 1);
    assert.equal(await dialog.locator('input[name="image"]').count(), 8);
    assert.equal(await dialog.getByRole('radio').count(), 4);
    await installedImages.getByRole('button').click();
    assert.equal(await dialog.getByRole('radio').count(), 0);
    const newerImages = dialog.getByRole('region', { name: 'Android · API 36', exact: true });
    await newerImages.getByRole('button').focus();
    await page.keyboard.press('Enter');
    await newerImages.getByRole('radio').check();
    await dialog.getByRole('button', { name: '上一步', exact: true }).click();
    await dialog.getByRole('button', { name: '下一步', exact: true }).click();
    assert.equal(await newerImages.getByRole('radio').isChecked(), true);
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    dialog = page.getByRole('dialog', { name: 'Create emulator', exact: true });
    await dialog.getByText('Selected', { exact: true }).waitFor();
    await dialog.getByText('1 images', { exact: true }).first().waitFor();
    await page.getByRole('button', { name: 'Switch interface language', exact: true }).click();
    dialog = page.getByRole('dialog', { name: '新建模拟器', exact: true });
    await page.screenshot({ path: path.join(output, 'avd-image-api-groups.png') });
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(await dialog.evaluate((el) => el.scrollWidth > el.clientWidth), false);
    await page.screenshot({ path: path.join(output, 'avd-image-api-groups-compact.png') });
    await page.setViewportSize({ width: 1280, height: 800 });
    await dialog.getByRole('button', { name: '下一步', exact: true }).click();
    assert.equal(await dialog.getByLabel('屏幕宽度（px）', { exact: true }).inputValue(), '1536');
    await dialog.getByRole('button', { name: '查看创建计划' }).click();
    await page.getByRole('dialog', { name: 'Fixture plan' }).waitFor();
    const created = await page.evaluate(() => window.__operations.at(-1));
    assert.equal(created.kind, 'avd_create');
    assert.equal(created.args.image, 'system-images;android-36;default;x86_64');
    assert.equal(created.args.profile, 'Nexus 9');
    assert.equal(created.args.storageMiB, '8192');
    assert.equal(created.args.screenHeight, '2048');
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    await dialog.getByLabel('显示名称', { exact: true }).fill('自定义 name must stay');
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    dialog = page.getByRole('dialog', { name: 'Create emulator', exact: true });
    assert.equal(
      await dialog.getByLabel('Display name', { exact: true }).inputValue(),
      '自定义 name must stay',
    );
    await dialog.getByRole('button', { name: 'Review creation plan', exact: true }).waitFor();
    assert.equal(await page.locator('html').getAttribute('lang'), 'en-US');
    await dialog.getByRole('button', { name: 'Close panel', exact: true }).click();
    await page.getByRole('button', { name: 'SDK Manager', exact: true }).click();
    // SDK navigation now preserves the tab and selections while Java setup opens other pages.
    await page.getByRole('button', { name: 'SDK Platforms', exact: true }).click();
    await page.getByLabel('Show preview components', { exact: true }).waitFor();
    await page.getByRole('textbox', { name: 'Search components', exact: true }).fill('Sources');
    const selectedSource = page.getByLabel('sources;android-35 change', { exact: true });
    await selectedSource.selectOption('install');
    await page.getByRole('button', { name: 'Switch interface language', exact: true }).click();
    assert.equal(
      await page.getByLabel('sources;android-35 变更', { exact: true }).inputValue(),
      'install',
    );
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await page.getByRole('button', { name: 'Task Center', exact: true }).click();
    await page.getByRole('heading', { name: 'Task Center', exact: true }).waitFor();
    await page.getByText('Emulator system is ready', { exact: true }).waitFor();
    await page.getByText('Start emulator', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    assert.equal(
      await page.getByLabel('Interface language', { exact: true }).inputValue(),
      'en-US',
    );
    await page.reload();
    await page.getByRole('heading', { name: 'Device Workbench', exact: true }).waitFor();
    assert.equal(await page.locator('html').getAttribute('lang'), 'en-US');
    await page.getByRole('button', { name: 'Connect device', exact: true }).click();
    dialog = page.getByRole('dialog', { name: 'Connect device', exact: true });
    await dialog.getByLabel('Device IP address', { exact: true }).fill('192.0.2.10');
    await page.getByRole('button', { name: 'Switch interface language', exact: true }).click();
    assert.equal(await page.getByLabel('设备 IP 地址', { exact: true }).inputValue(), '192.0.2.10');
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(
      await page.getByRole('dialog').evaluate((el) => el.scrollWidth > el.clientWidth),
      false,
    );
    await page.screenshot({ path: path.join(output, 'language-english-compact.png') });
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.getByRole('button', { name: 'Close panel', exact: true }).click();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.screenshot({ path: path.join(output, 'language-settings-english.png') });
    await page.getByLabel('Interface language', { exact: true }).selectOption('zh-CN');
    await page.getByRole('heading', { name: '设置', exact: true }).waitFor();
    await page.getByLabel('界面主题', { exact: true }).selectOption('dark');
    assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
    const palette = await page.evaluate(() => {
      const root = getComputedStyle(document.documentElement);
      const panel = getComputedStyle(document.querySelector('.settings-panel section'));
      const button = getComputedStyle(document.querySelector('.button.primary'));
      return {
        page: root.backgroundColor,
        surface: panel.backgroundColor,
        text: root.color,
        buttonText: button.color,
        buttonFill: button.backgroundColor,
        scheme: root.colorScheme,
      };
    });
    assert.equal(palette.page, 'rgb(17, 24, 20)');
    assert.equal(palette.surface, 'rgb(26, 38, 30)');
    assert.equal(palette.scheme, 'dark');
    const contrast = (a, b) => {
      const light = (color) => {
        const rgb = color
          .match(/[\d.]+/g)
          .slice(0, 3)
          .map(Number)
          .map((v) => v / 255)
          .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
        return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
      };
      const [x, y] = [light(a), light(b)];
      return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
    };
    assert(contrast(palette.text, palette.surface) >= 4.5);
    assert(contrast(palette.buttonText, palette.buttonFill) >= 4.5);
    await page.getByLabel('当前 SDK 根目录', { exact: true }).fill('E:\\Unsaved-theme-input');
    await page.getByRole('button', { name: '切换为亮色主题', exact: true }).click();
    assert.equal(
      await page.getByLabel('当前 SDK 根目录', { exact: true }).inputValue(),
      'E:\\Unsaved-theme-input',
    );
    await page.getByRole('button', { name: '切换为暗色主题', exact: true }).click();
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    assert.equal(await page.getByLabel('Interface theme', { exact: true }).inputValue(), 'dark');
    await page.screenshot({ path: path.join(output, 'theme-dark-settings.png') });
    await page.getByRole('button', { name: 'SDK Manager', exact: true }).click();
    await page
      .getByRole('textbox', { name: 'Search components', exact: true })
      .fill('Google Play arm64');
    await page.screenshot({ path: path.join(output, 'theme-dark-sdk.png') });
    await page.getByRole('button', { name: 'Device Workbench', exact: true }).click();
    await page.locator('.device-row').filter({ hasText: 'Fixture phone' }).click();
    await page.getByRole('button', { name: 'Edit configuration', exact: true }).click();
    dialog = page.getByRole('dialog', { name: 'Edit emulator configuration', exact: true });
    await dialog.getByLabel('Display name', { exact: true }).fill('theme keeps 自定义 input');
    await page.getByRole('button', { name: 'Switch to light theme', exact: true }).click();
    assert.equal(
      await dialog.getByLabel('Display name', { exact: true }).inputValue(),
      'theme keeps 自定义 input',
    );
    await page.getByRole('button', { name: 'Switch to dark theme', exact: true }).click();
    await page.screenshot({ path: path.join(output, 'theme-dark-editor.png') });
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await page.reload();
    await page.getByRole('heading', { name: 'Device Workbench', exact: true }).waitFor();
    assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
    assert.equal(
      await page.evaluate(() => getComputedStyle(document.documentElement).colorScheme),
      'dark',
    );
    await page.setViewportSize({ width: 480, height: 640 });
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
      false,
    );
    await page.screenshot({ path: path.join(output, 'theme-dark-compact.png') });
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.getByRole('button', { name: 'Switch to light theme', exact: true }).click();
    assert.equal(await page.evaluate(() => localStorage.getItem('android-devkit.theme')), 'light');
    await page.getByRole('button', { name: 'Switch interface language', exact: true }).click();
    assert.deepEqual(errors, []);
    console.log(
      'SDK/AVD UI: refresh lifecycle, hierarchy, site CRUD, image API grouping and ordering, configuration, language/theme switching and persistence, input preservation, contrast and compact layout passed.',
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
