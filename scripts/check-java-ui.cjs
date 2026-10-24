// IPC fixtures only: no SDK, Java, environment variables or user settings are changed.
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
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.addInitScript(() => {
      const sdkPackage = {
        id: 'platform-tools',
        name: 'Android SDK Platform-Tools',
        version: '37.0.0',
        installed: false,
        installedVersion: '',
        category: 'tool',
        api: '',
        abi: '',
        tag: '',
        size: 1024,
        dependencies: [],
        dependencyMinVersions: {},
        obsolete: false,
        channel: 0,
        sourceUrl: '',
      };
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
          sdkDownload: { kind: 'google', name: '', baseUrl: '', verifyOfficial: true },
        },
        environment: { adb: 'fixture', warnings: [] },
        devices: [],
        avds: [],
        tasks: [],
        packages: [sdkPackage],
        server: 'fixture',
        serverEpoch: 1,
        freeBytes: 20 * 1024 ** 3,
      };
      window.__calls = [];
      window.__javaFailure = false;
      window.__holdCheck = false;
      window.__javaCandidate = false;
      window.__java = (path) => ({
        path: path || '',
        source: 'configured',
        status: path && !path.includes('old') ? 'ready' : 'missing',
        version: path ? '21.0.1' : '',
        vendor: 'fixture',
        architecture: 'amd64',
        message: path ? 'Java 与 SDK 工具兼容性检查通过' : '未找到可用的 Java，请配置运行环境',
        toolsReady: true,
        compatibilityChecked: !!path,
      });
      const license = { id: 'Temurin', text: 'Fixture Java license', hash: 'java-license' };
      window.__finishJava = (failed) => {
        const task = window.__fixture.tasks[0];
        task.state = failed ? 'failed' : 'succeeded';
        task.message = failed ? 'Java 下载连接超时' : 'Java 已就绪';
        task.progress = failed ? 20 : 100;
        if (!failed)
          window.__fixture.settings.javaPath = 'C:\\Fixture\\runtime\\java21\\bin\\java.exe';
      };
      window.isTauri = true;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: () => 1,
        unregisterCallback() {},
        invoke: async (command, args = {}) => {
          window.__calls.push({ command, args: structuredClone(args) });
          if (command === 'workspace_snapshot') return structuredClone(window.__fixture);
          if (command === 'window_state') return { maximized: false };
          if (command === 'java_check') {
            if (window.__holdCheck)
              await new Promise((resolve) => {
                window.__releaseCheck = resolve;
              });
            return window.__java(
              args.path === undefined ? window.__fixture.settings.javaPath : args.path,
            );
          }
          if (command === 'java_environment')
            return {
              current: window.__java(window.__fixture.settings.javaPath),
              candidates: window.__javaCandidate
                ? [{ ...window.__java('C:\\Studio\\jbr\\bin\\java.exe'), source: 'studio' }]
                : [],
              privatePath: 'C:\\Fixture\\runtime\\java21\\bin\\java.exe',
              download: {
                ...sdkPackage,
                size: 48999141,
                url: 'https://github.com/adoptium/fixture',
              },
              license,
              busy: window.__fixture.tasks.some((task) => task.state === 'running'),
              preparation: window.__fixture.tasks.find((task) => task.state === 'running') || null,
            };
          if (command === 'java_apply') {
            if (args.path.includes('old')) throw { message: 'Java 不兼容' };
            window.__fixture.settings.javaPath = args.path;
            return window.__java(args.path);
          }
          if (command === 'file_choose') return 'C:\\Java\\bin\\java.exe';
          if (command === 'plan_create') {
            if (args.operation.kind === 'sdk_install' && !window.__fixture.settings.javaPath)
              throw { code: 'JAVA_CONFIGURATION_REQUIRED', message: '配置 Java' };
            return {
              id: args.operation.kind,
              hash: 'fixture',
              confirmationToken: 'fixture',
              title: args.operation.kind === 'java_prepare' ? '准备 Java 运行时' : 'SDK 安装计划',
              operation: args.operation,
              effects: ['Fixture plan'],
              warnings: [],
              licenses: args.operation.kind === 'java_prepare' ? [license] : [],
            };
          }
          if (command === 'task_submit') {
            if (args.planId === 'java_prepare') {
              const task = {
                id: 'java-task',
                title: '准备 Java 运行时',
                state: 'running',
                phase: 'downloading',
                message: '正在下载 Java',
                progress: 20,
                cancellable: true,
                operation: { kind: 'java_prepare', args: {}, target: null },
                createdAt: 1,
                updatedAt: 1,
                log: [],
              };
              window.__fixture.tasks = [task];
              return structuredClone(task);
            }
            return { id: 'sdk-task' };
          }
          if (command === 'task_cancel') {
            window.__fixture.tasks[0].state = 'cancelled';
            window.__fixture.tasks[0].message = '已取消';
            return;
          }
          if (command === 'task_list')
            return {
              tasks: structuredClone(window.__fixture.tasks),
              total: window.__fixture.tasks.length,
              counts: {},
              offset: 0,
              pageSize: 30,
            };
          if (command === 'settings_save') {
            window.__fixture.settings = structuredClone(args.settings);
            return;
          }
          if (command === 'sdk_sites_status' || command === 'artifacts_list') return [];
          if (command.startsWith('plugin:event|')) return 1;
          return null;
        },
      };
    });
    const url = process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327';
    const navigate = (name) => page.getByRole('button', { name, exact: true }).first().click();
    const selectComponent = async () => {
      await navigate('SDK 管理');
      await page.getByRole('button', { name: 'SDK Tools', exact: true }).click();
      await page.getByLabel('platform-tools 变更', { exact: true }).selectOption('install');
      await page.getByRole('button', { name: '确认安装 1 项', exact: true }).click();
      await page.getByRole('dialog', { name: '配置 Java 运行环境' }).waitFor();
    };
    await page.goto(url);
    await selectComponent();
    let modal = page.getByRole('dialog', { name: '配置 Java 运行环境' });
    assert.equal(await modal.getByRole('button', { name: '同意并准备 Java' }).isDisabled(), true);
    await modal.getByRole('checkbox', { name: '我已查看并接受 Temurin 运行时许可' }).check();
    await modal.getByRole('button', { name: '同意并准备 Java' }).click();
    await modal.getByRole('button', { name: '关闭并保留任务' }).waitFor();
    assert.equal(
      await page.evaluate(
        () => window.__calls.filter((item) => item.command === 'task_submit').length,
      ),
      1,
    );
    await modal.getByRole('button', { name: '关闭并保留任务' }).click();
    await navigate('设置');
    await navigate('SDK 管理');
    assert.equal(
      await page.getByLabel('platform-tools 变更', { exact: true }).inputValue(),
      'install',
    );
    await page.getByRole('button', { name: '配置 Java', exact: true }).click();
    await modal.getByRole('button', { name: '关闭并保留任务' }).waitFor();
    await page.evaluate(() => window.__finishJava(false));
    await modal.getByRole('button', { name: '返回安装计划' }).click();
    await page.getByRole('dialog', { name: 'SDK 安装计划', exact: true }).waitFor();
    const resumed = await page.evaluate(
      () => window.__calls.filter((item) => item.command === 'plan_create').at(-1).args.operation,
    );
    assert.equal(resumed.args.packages, 'platform-tools');
    assert.equal(
      await page.evaluate(
        () => window.__calls.filter((item) => item.command === 'task_submit').length,
      ),
      1,
    );
    await page.getByRole('button', { name: '返回修改', exact: true }).click();
    assert.equal(
      await page.getByLabel('platform-tools 变更', { exact: true }).inputValue(),
      'install',
    );
    console.log(
      'Private Java, license gate, background reconnect, retained selection and explicit SDK plan: OK',
    );

    await page.reload();
    await selectComponent();
    await modal.getByRole('radio', { name: /使用已有 Java/ }).check();
    await modal.getByRole('button', { name: '选择文件' }).click();
    await modal.getByRole('button', { name: '检测可用性' }).click();
    await modal.getByRole('button', { name: '使用此 Java' }).waitFor({ state: 'visible' });
    assert.equal(await modal.getByRole('button', { name: '使用此 Java' }).isEnabled(), true);
    await modal
      .getByRole('textbox', { name: 'Java 可执行文件', exact: true })
      .fill('C:\\old\\java.exe');
    assert.equal(await modal.getByRole('button', { name: '使用此 Java' }).isDisabled(), true);
    await modal.getByRole('button', { name: '检测可用性' }).click();
    assert.equal(await modal.getByRole('button', { name: '使用此 Java' }).isDisabled(), true);
    await modal.getByRole('button', { name: '选择文件' }).click();
    await modal.getByRole('button', { name: '检测可用性' }).click();
    await modal.getByRole('button', { name: '使用此 Java' }).click();
    await page.getByRole('dialog', { name: 'SDK 安装计划', exact: true }).waitFor();
    assert.equal(
      await page.evaluate(
        () => window.__calls.filter((item) => item.command === 'task_submit').length,
      ),
      0,
    );
    console.log('Existing Java validation, stale result invalidation and resume: OK');

    await page.reload();
    await selectComponent();
    await modal.getByRole('checkbox', { name: '我已查看并接受 Temurin 运行时许可' }).check();
    await modal.getByRole('button', { name: '同意并准备 Java' }).click();
    await modal.getByRole('button', { name: '关闭并保留任务' }).waitFor();
    await page.evaluate(() => window.__finishJava(true));
    await modal.getByRole('button', { name: '检查代理设置' }).click();
    await navigate('SDK 管理');
    assert.equal(
      await page.getByLabel('platform-tools 变更', { exact: true }).inputValue(),
      'install',
    );
    await page.getByRole('button', { name: '配置 Java', exact: true }).click();
    await modal.getByRole('radio', { name: /使用已有 Java/ }).check();
    await modal.getByRole('button', { name: '选择文件' }).click();
    await modal.getByRole('button', { name: '检测可用性' }).click();
    await modal.getByRole('button', { name: '使用此 Java' }).click();
    await page.getByRole('dialog', { name: 'SDK 安装计划', exact: true }).waitFor();
    assert.equal(
      await page.evaluate(
        () =>
          window.__calls.filter(
            (item) => item.command === 'task_submit' && item.args.planId !== 'java_prepare',
          ).length,
      ),
      0,
    );
    console.log('Download failure, proxy navigation and local recovery preserve SDK choices: OK');
    await page.reload();
    await selectComponent();
    fs.mkdirSync(path.resolve(__dirname, '../.qa/app'), { recursive: true });
    await modal.getByRole('radio', { name: /一键准备应用私有 Java/ }).waitFor();
    await page.screenshot({ path: path.resolve(__dirname, '../.qa/app/java-environment.png') });
    await modal.getByRole('checkbox', { name: '我已查看并接受 Temurin 运行时许可' }).check();
    await modal.getByRole('button', { name: '同意并准备 Java' }).click();
    await modal.getByRole('button', { name: '取消准备' }).click();
    await modal.getByRole('button', { name: '重试下载' }).waitFor();
    await modal.getByRole('button', { name: '稍后处理' }).click();
    assert.equal(
      await page.getByLabel('platform-tools 变更', { exact: true }).inputValue(),
      'install',
    );
    await page.reload();
    await page.evaluate(() => {
      window.__javaCandidate = true;
    });
    await selectComponent();
    await modal.getByRole('button', { name: '使用此 Java' }).waitFor();
    assert.equal(await modal.getByRole('radio', { name: /使用已有 Java/ }).isChecked(), true);
    assert.equal(await modal.getByRole('button', { name: '使用此 Java' }).isEnabled(), true);
    await modal.getByRole('button', { name: '关闭面板', exact: true }).focus();
    await page.keyboard.press('Shift+Tab');
    assert.equal(await modal.evaluate((element) => element.contains(document.activeElement)), true);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await page.getByRole('button', { name: 'Configure Java', exact: true }).click();
    const english = page.getByRole('dialog', { name: 'Configure Java runtime', exact: true });
    await english.getByRole('radio', { name: /Use existing Java/ }).waitFor();
    await english.getByRole('button', { name: 'Choose file', exact: true }).waitFor();
    await page.setViewportSize({ width: 680, height: 900 });
    await page.evaluate(() => (document.documentElement.dataset.theme = 'dark'));
    const overflow = await english.evaluate(
      (element) => element.scrollWidth > element.clientWidth + 1,
    );
    assert.equal(overflow, false);
    await page.screenshot({
      path: path.resolve(__dirname, '../.qa/app/java-environment-dark-compact.png'),
    });
    console.log(
      'Cancellation, Studio detection, keyboard focus, English and compact dark layout: OK',
    );
    assert.deepEqual(errors, []);
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
