// Production UI with isolated IPC doubles. No real keys, SDK changes, or devices.
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
    await require('./preferences-fixture.cjs')(page);
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.addInitScript(() => {
      window.isTauri = true;
      window.__calls = [];
      window.__tasks = [];
      window.__pendingArtifacts = [];
      window.__loseResponse = false;
      window.__failInspect = false;
      window.__missingJava = false;
      window.__existingOutput = false;
      const certificate = {
        subject: 'CN=Fixture',
        issuer: 'CN=Fixture',
        notBefore: 1600000000,
        notAfter: 2700000000,
        algorithm: 'RSA',
        bits: 3072,
        sha1: 'a'.repeat(40),
        sha256: 'b'.repeat(64),
        der: 'fixture-public-certificate',
      };
      window.__cert = certificate;
      window.__records = [
        {
          id: 'fixture-key',
          name: 'Fixture key',
          path: 'C:\\QA\\release.p12',
          role: 'upload',
          package: 'dev.fixture',
          notes: '',
          sha256: 'c'.repeat(64),
          format: 'PKCS12',
          checkedAt: 1700000000,
          status: 'ready',
          entries: [
            { alias: 'upload', privateKey: true, certificate },
            { alias: 'public-only', privateKey: false, certificate },
          ],
        },
      ];
      const settings = {
        sdkRoot: 'C:\\QA\\SDK',
        avdRoots: [],
        javaPath: 'C:\\QA\\java.exe',
        proxy: '',
        bootTimeoutSecs: 180,
        sdkSites: [],
        sdkChannel: 0,
        sdkDownload: { kind: 'google', name: '', url: '', verifyOfficial: true },
      };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      let callback = 0;
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback() {
          return ++callback;
        },
        unregisterCallback() {},
        invoke: async (command, args = {}) => {
          const recorded = structuredClone(args);
          if (recorded.secrets) recorded.secrets = { redacted: true };
          window.__calls.push({ command, args: recorded });
          if (command === 'workspace_snapshot')
            return {
              setupRequired: false,
              settings,
              environment: { path: settings.sdkRoot, adb: 'fixture', warnings: [] },
              freeBytes: 1e11,
              packages: [],
              avds: [],
              devices: [],
              tasks: structuredClone(window.__tasks),
              server: 'fixture',
              serverEpoch: 1,
            };
          if (command === 'window_state') return { maximized: false };
          if (command === 'terminal_list' || command === 'sdk_sites_status') return [];
          if (command === 'java_check')
            return {
              path: settings.javaPath,
              status: 'ready',
              version: '21',
              source: 'configured',
              architecture: 'amd64',
              vendor: 'fixture',
              message: 'ready',
              toolsReady: true,
              compatibilityChecked: true,
            };
          if (command === 'signing_environment')
            return {
              java: window.__missingJava ? '' : settings.javaPath,
              keytool: window.__missingJava ? '' : 'C:\\QA\\keytool.exe',
              apksigner: 'C:\\QA\\apksigner.jar',
              zipalign: 'C:\\QA\\zipalign.exe',
            };
          if (command === 'signing_records') return structuredClone(window.__records);
          if (command === 'artifacts_list') return structuredClone(window.__pendingArtifacts);
          if (command === 'task_list')
            return {
              tasks: structuredClone(window.__tasks),
              total: window.__tasks.length,
              counts: {},
              offset: 0,
              pageSize: 30,
              deletion: { blocked: {} },
            };
          if (command === 'close_status')
            return {
              tasks: [],
              unsaved: structuredClone(window.__pendingArtifacts),
              draining: false,
              trayEnabled: true,
            };
          if (command === 'signing_choose')
            return {
              apk: 'C:\\QA\\input.apk',
              certificate: 'C:\\QA\\certificate.pem',
              'output-apk': 'C:\\QA\\output-signed.apk',
              'output-p12': 'C:\\QA\\new.p12',
              'output-jks': 'C:\\QA\\new.jks',
              keystore: 'C:\\QA\\release.p12',
            }[args.kind];
          if (command === 'apk_signature_inspect') {
            if (window.__failInspect) throw { message: 'Fixture inspection failed' };
            return {
              path: args.path,
              sha256: 'd'.repeat(64),
              package: 'dev.fixture',
              version: '1.0',
              verified: true,
              unsigned: false,
              signers: [certificate],
              schemes: ['v1', 'v2'],
              errors: [],
              warnings: [],
              lineage: false,
              standalone: true,
            };
          }
          if (command === 'signing_certificate_inspect') return certificate;
          if (command === 'signing_prepare') {
            if (args.secrets.storePassword === 'wrong') throw { message: 'Wrong fixture password' };
            window.__plan = {
              id: 'signing-plan',
              hash: 'fixture-hash',
              title: args.request.mode === 'create' ? '新建 Keystore' : '签名 APK',
              operation: {
                kind: args.request.mode === 'create' ? 'keystore_create' : 'apk_sign',
                args: {
                  request: JSON.stringify(args.request),
                  package: 'dev.fixture',
                  version: '1.0',
                  inputBytes: '87178270',
                  keystoreName: 'Fixture key',
                  keystorePath: 'C:\\QA\\release.p12',
                  overwriteSource: String(args.request.overwriteSource),
                  overwriteOutput: String(window.__existingOutput && !args.request.overwriteSource),
                },
                target: null,
              },
              effects: ['Fixture plan', args.request.output],
              warnings: [],
              licenses: [],
              createdAt: 1,
              settingsHash: 'settings',
              confirmationToken: 'token',
            };
            return window.__plan;
          }
          if (command === 'signing_submit') {
            let task = window.__tasks.find((task) => task.requestId === args.requestId);
            if (!task) {
              task = {
                id: 'fixture-signing-task',
                requestId: args.requestId,
                title: window.__plan.title,
                operation: {
                  ...window.__plan.operation,
                  args: { savedOutput: 'C:\\QA\\output-signed.apk' },
                },
                state: 'succeeded',
                phase: 'complete',
                progress: 100,
                message: '签名结果已验证并保存',
                log: [],
                createdAt: 1,
                updatedAt: 1,
                cancelRequested: false,
                cancellable: false,
                artifact: '',
                licensePrompt: '',
                confirmedLicenses: [],
              };
              window.__tasks.push(task);
            }
            if (window.__loseResponse) {
              window.__loseResponse = false;
              throw { message: 'Response lost after submission' };
            }
            return structuredClone(task);
          }
          if (command === 'keystore_register') return args.record;
          if (command === 'signing_result_discard') {
            window.__pendingArtifacts = window.__pendingArtifacts.filter(
              (artifact) => artifact.id !== args.artifactId,
            );
            return null;
          }
          if (command === 'apk_prepare')
            return {
              id: 'fixture-install-apk',
              path: args.path,
              fileName: 'output-signed.apk',
              size: 4096,
              sha256: 'd'.repeat(64),
              package: 'dev.fixture',
              version: '1.0',
              minSdk: '23',
              abis: [],
              warning: '',
            };
          return null;
        },
      };
    });
    await page.goto(process.env.ANDROID_DEVKIT_UI_URL || 'http://127.0.0.1:19327');
    await page.getByRole('button', { name: '签名与 Keystore', exact: true }).click();
    const tool = page.locator('.signing-tools');
    const environment = tool.locator('.signing-environment');
    const environmentDetails = environment.locator('.signing-environment-details');
    await environment.getByLabel('工具环境：签名工具齐全', { exact: true }).waitFor();
    assert.equal(await tool.getByRole('button', { name: '配置 Java', exact: true }).count(), 0);
    await environment.locator('summary').hover();
    await environmentDetails.waitFor({ state: 'visible' });
    // Clicking a mouse-hovered icon must not toggle the newly opened panel shut.
    await environment.locator('summary').click();
    await environmentDetails.waitFor({ state: 'visible' });
    const summaryBounds = await environment.locator('summary').boundingBox();
    const detailsBounds = await environmentDetails.boundingBox();
    await page.mouse.move(
      summaryBounds.x + summaryBounds.width / 2,
      summaryBounds.y + summaryBounds.height + 4,
    );
    await page.waitForTimeout(60);
    await page.mouse.move(detailsBounds.x + detailsBounds.width - 16, detailsBounds.y + 12);
    await page.waitForTimeout(240);
    assert.equal(await environmentDetails.isVisible(), true);
    assert.equal(await environment.getByRole('button').count(), 0);
    await page.mouse.move(20, 45);
    await environmentDetails.waitFor({ state: 'hidden' });
    await page.evaluate(() => {
      window.__missingJava = true;
    });
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page.getByRole('button', { name: '签名与 Keystore', exact: true }).click();
    await environment.getByLabel('工具环境：缺少签名工具', { exact: true }).waitFor();
    await environment.locator('summary').hover();
    await environment.getByRole('button', { name: '前往设置', exact: true }).click();
    await page.locator('.settings-panel').waitFor({ state: 'visible' });
    assert.equal(
      await page.getByRole('dialog', { name: '配置 Java 运行环境', exact: true }).count(),
      0,
    );
    assert.equal(
      await page.evaluate(() => window.__calls.some((c) => c.command === 'java_environment')),
      false,
    );
    await page.evaluate(() => {
      window.__missingJava = false;
    });
    await page.getByRole('button', { name: '签名与 Keystore', exact: true }).click();
    await environment.getByLabel('工具环境：签名工具齐全', { exact: true }).waitFor();
    await environment.locator('summary').focus();
    await page.keyboard.press('Enter');
    await environmentDetails.waitFor({ state: 'visible' });
    await page.keyboard.press('Escape');
    await environmentDetails.waitFor({ state: 'hidden' });
    await tool.getByRole('button', { name: /Fixture key/ }).click();
    const aliases = tool.getByLabel('密钥 alias', { exact: true });
    assert.equal(await aliases.inputValue(), '');
    await aliases.selectOption('public-only');
    assert.equal(
      await tool.getByRole('button', { name: '使用此密钥签名', exact: true }).isDisabled(),
      true,
    );
    await aliases.selectOption('upload');
    assert.equal(
      await tool.getByRole('button', { name: '使用此密钥签名', exact: true }).isEnabled(),
      true,
    );
    await tool.getByRole('tab', { name: 'APK 校验', exact: true }).click();
    await tool.getByRole('button', { name: '选择 APK', exact: true }).click();
    fs.mkdirSync(path.resolve(__dirname, '../.qa/signing'), { recursive: true });
    await tool
      .locator('.signing-file-row')
      .screenshot({ path: path.resolve(__dirname, '../.qa/signing/apk-controls.png') });
    await tool.getByRole('button', { name: '检查签名', exact: true }).click();
    await tool.getByText('当前证书一致', { exact: false }).waitFor();
    await tool.getByRole('tab', { name: 'APK 签名', exact: true }).click();
    await tool.locator('.signing-apk-summary').getByText('已签名', { exact: true }).waitFor();
    assert.equal(await tool.locator('.signing-report').count(), 0);
    await page.screenshot({ path: path.resolve(__dirname, '../.qa/signing/compact-signing.png') });
    await tool.getByRole('button', { name: '查看签名详情', exact: true }).click();
    await tool.getByText('当前证书一致', { exact: false }).waitFor();
    assert.equal(
      await page.evaluate(
        () => window.__calls.filter((c) => c.command === 'apk_signature_inspect').length,
      ),
      1,
    );
    await page.evaluate(() => {
      window.__failInspect = true;
    });
    await tool.getByRole('button', { name: '检查签名', exact: true }).click();
    await tool.getByRole('alert').getByText('Fixture inspection failed').waitFor();
    assert.equal(await tool.getByText('签名完整性验证通过', { exact: true }).count(), 0);
    await page.evaluate(() => {
      window.__failInspect = false;
    });
    await tool.getByRole('tab', { name: 'APK 签名', exact: true }).click();
    const openSigning = () =>
      tool.getByRole('button', { name: '选择输出并核对签名计划', exact: true }).click();
    await openSigning();
    let passwords = page.getByRole('dialog', { name: '提供本次签名密码', exact: true });
    await passwords.getByLabel('容器密码', { exact: true }).fill('wrong');
    await passwords.getByRole('button', { name: '显示容器密码', exact: true }).click();
    assert.equal(
      await passwords.getByLabel('容器密码', { exact: true }).getAttribute('type'),
      'text',
    );
    assert.equal(await passwords.getByLabel('容器密码', { exact: true }).inputValue(), 'wrong');
    assert.equal(
      await passwords
        .getByLabel('私钥密码（留空使用容器密码）', { exact: true })
        .getAttribute('type'),
      'password',
    );
    await passwords.getByRole('button', { name: '隐藏容器密码', exact: true }).focus();
    await page.keyboard.press('Enter');
    assert.equal(
      await passwords.getByLabel('容器密码', { exact: true }).getAttribute('type'),
      'password',
    );
    await passwords
      .locator('.signing-password-input')
      .first()
      .screenshot({ path: path.resolve(__dirname, '../.qa/signing/password-eye.png') });
    await passwords.getByRole('button', { name: '显示容器密码', exact: true }).click();
    await passwords.getByRole('button', { name: '检查并继续', exact: true }).click();
    await passwords.getByRole('alert').getByText('Wrong fixture password').waitFor();
    await page.keyboard.press('Escape');
    await openSigning();
    passwords = page.getByRole('dialog', { name: '提供本次签名密码', exact: true });
    assert.equal(await passwords.getByLabel('容器密码', { exact: true }).inputValue(), '');
    assert.equal(
      await passwords.getByLabel('容器密码', { exact: true }).getAttribute('type'),
      'password',
    );
    const testPassword = 'Fixture-only-password-9382';
    await passwords.getByLabel('容器密码', { exact: true }).fill(testPassword);
    await passwords.getByRole('button', { name: '检查并继续', exact: true }).click();
    const plan = page.getByRole('dialog', { name: '核对签名计划', exact: true });
    await plan.waitFor();
    assert.equal(await page.locator('input[type=password]').count(), 0);
    await page.evaluate(() => {
      window.__loseResponse = true;
    });
    await plan.getByRole('button', { name: '确认签名', exact: true }).click();
    await plan.getByRole('alert').getByText('Response lost after submission').waitFor();
    await plan.getByRole('button', { name: '确认签名', exact: true }).click();
    await tool.getByText('任务已提交，可在本页或任务中心查看', { exact: true }).waitFor();
    const submissions = await page.evaluate(() =>
      window.__calls.filter((c) => c.command === 'signing_submit'),
    );
    assert.equal(submissions.length, 2);
    assert.equal(submissions[0].args.requestId, submissions[1].args.requestId);
    assert.equal(await page.evaluate(() => window.__tasks.length), 1);
    assert.equal(
      await page.evaluate((p) => JSON.stringify(localStorage).includes(p), testPassword),
      false,
    );
    await page.evaluate(() => {
      window.__existingOutput = true;
    });
    await openSigning();
    passwords = page.getByRole('dialog', { name: '提供本次签名密码', exact: true });
    await passwords.getByLabel('容器密码', { exact: true }).fill(testPassword);
    await passwords.getByRole('button', { name: '检查并继续', exact: true }).click();
    await plan.getByText('覆盖输出文件', { exact: true }).waitFor();
    await plan.getByRole('button', { name: '签名并覆盖', exact: true }).waitFor();
    await plan.getByRole('button', { name: '返回修改', exact: true }).click();
    await page.evaluate(() => {
      window.__existingOutput = false;
    });
    const outputPickCount = await page.evaluate(
      () =>
        window.__calls.filter((c) => c.command === 'signing_choose' && c.args.kind === 'output-apk')
          .length,
    );
    await tool.getByLabel('覆盖源文件', { exact: true }).check();
    await tool.getByRole('button', { name: '核对签名计划', exact: true }).click();
    passwords = page.getByRole('dialog', { name: '提供本次签名密码', exact: true });
    await passwords.getByLabel('容器密码', { exact: true }).fill(testPassword);
    await passwords.getByRole('button', { name: '检查并继续', exact: true }).click();
    await plan.getByRole('button', { name: '签名并覆盖', exact: true }).waitFor();
    assert.equal(
      await page.evaluate(
        () =>
          window.__calls.filter(
            (c) => c.command === 'signing_choose' && c.args.kind === 'output-apk',
          ).length,
      ),
      outputPickCount,
    );
    const overwriteRequest = await page.evaluate(
      () => window.__calls.filter((c) => c.command === 'signing_prepare').at(-1).args.request,
    );
    assert.equal(overwriteRequest.overwriteSource, true);
    assert.equal(overwriteRequest.output, overwriteRequest.input);
    assert.equal(await plan.locator('details').count(), 0);
    await page.screenshot({ path: path.resolve(__dirname, '../.qa/signing/review-overwrite.png') });
    await plan.getByRole('button', { name: '返回修改', exact: true }).click();
    await tool.getByRole('tab', { name: 'Keystore', exact: true }).click();
    await tool.getByRole('button', { name: '新建 Keystore', exact: true }).click();
    const create = page.getByRole('dialog', { name: '新建 Keystore', exact: true });
    await create.getByLabel('名称', { exact: true }).fill('New disposable key');
    await create.getByLabel('证书主体（DN）', { exact: true }).fill('CN=Disposable');
    await create.getByRole('button', { name: '选择新文件', exact: true }).click();
    await create.getByLabel('容器密码', { exact: true }).fill(testPassword);
    await create.getByLabel('确认容器密码', { exact: true }).fill('mismatch');
    assert.equal(
      await create.getByRole('button', { name: '核对生成计划', exact: true }).isDisabled(),
      true,
    );
    await create.getByLabel('确认容器密码', { exact: true }).fill(testPassword);
    await create.getByRole('button', { name: '核对生成计划', exact: true }).click();
    await plan.waitFor();
    await plan.getByRole('button', { name: '返回修改', exact: true }).click();
    await page.waitForFunction(() =>
      window.__calls.some((c) => c.command === 'signing_plan_release'),
    );
    await page.getByRole('button', { name: '任务中心', exact: true }).click();
    await page.evaluate(() => {
      window.__pendingArtifacts = [
        {
          id: 'pending-key',
          kind: 'keystore',
          title: '新建 Keystore（请备份）',
          taskId: 'key-task',
          bytes: 2048,
          sha256: 'e'.repeat(64),
          createdAt: 1,
          acknowledged: false,
        },
      ];
    });
    const inbox = page.getByRole('button', { name: /待保存文件：1 项/ });
    await inbox.waitFor();
    await inbox.click();
    const pending = page.getByRole('dialog', { name: '待保存文件', exact: true });
    assert.equal(
      await pending.getByRole('button', { name: '全部忽略并清理', exact: true }).isDisabled(),
      true,
    );
    assert.equal(
      await pending.getByRole('button', { name: '忽略并清理', exact: true }).isDisabled(),
      true,
    );
    assert.equal(
      await pending.getByRole('button', { name: '预览', exact: true }).isDisabled(),
      true,
    );
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: '签名与 Keystore', exact: true }).click();
    await page.evaluate(() => {
      window.__pendingArtifacts.push({
        id: 'saved-key-copy',
        kind: 'keystore',
        title: 'Saved disposable key',
        taskId: 'saved-key-task',
        bytes: 2048,
        sha256: 'f'.repeat(64),
        createdAt: 1,
        acknowledged: true,
      });
    });
    await tool.getByRole('button', { name: '清理此加密副本', exact: true }).click();
    const cleanup = page.getByRole('dialog', { name: '清理已另存密钥的缓存', exact: true });
    await cleanup
      .getByText('仅删除应用内的加密副本，已另存的 Keystore 和登记信息保持不变。', { exact: true })
      .waitFor();
    await cleanup.getByRole('button', { name: '清理此加密副本', exact: true }).click();
    await cleanup.waitFor({ state: 'hidden' });
    assert.equal(await page.evaluate(() => window.__records.length), 1);
    assert.equal(
      await page.evaluate(() => window.__pendingArtifacts.some((a) => a.id === 'saved-key-copy')),
      false,
    );
    assert.equal(
      await page.evaluate(() => window.__calls.some((c) => c.command === 'keystore_forget')),
      false,
    );
    await tool.getByRole('button', { name: '安装到设备', exact: true }).click();
    await page.waitForFunction(() => window.__calls.some((c) => c.command === 'apk_prepare'));
    assert.equal(
      await page.evaluate(() => window.__calls.find((c) => c.command === 'apk_prepare').args.path),
      'C:\\QA\\output-signed.apk',
    );
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: '签名与 Keystore', exact: true }).click();
    fs.mkdirSync(path.resolve(__dirname, '../.qa/signing'), { recursive: true });
    await page.screenshot({ path: path.resolve(__dirname, '../.qa/signing/keys.png') });
    await page.getByRole('button', { name: '切换界面语言', exact: true }).click();
    await tool.getByRole('tab', { name: 'APK signing', exact: true }).click();
    assert.equal(/[\u3400-\u9fff]/.test(await tool.innerText()), false);
    await page.setViewportSize({ width: 480, height: 900 });
    await page.getByRole('button', { name: 'Switch to dark theme', exact: true }).click();
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
      false,
    );
    await page.screenshot({
      path: path.resolve(__dirname, '../.qa/signing/english-dark-compact.png'),
    });
    await tool.getByRole('button', { name: 'Review signing plan', exact: true }).click();
    const englishPasswords = page.getByRole('dialog', {
      name: 'Provide passwords for this signing operation',
      exact: true,
    });
    await englishPasswords.getByLabel('Keystore password', { exact: true }).fill(testPassword);
    await englishPasswords.getByRole('button', { name: 'Check and continue', exact: true }).click();
    const englishPlan = page.getByRole('dialog', { name: 'Review signing plan', exact: true });
    await englishPlan.getByRole('button', { name: 'Sign and overwrite', exact: true }).waitFor();
    assert.equal(/[\u3400-\u9fff]/.test(await englishPlan.innerText()), false);
    await page.screenshot({
      path: path.resolve(__dirname, '../.qa/signing/review-english-compact.png'),
    });
    await page.keyboard.press('Escape');
    assert.deepEqual(errors, []);
    const report = {
      environmentSettingsHandoff: true,
      compactSigningReport: true,
      overwriteSourceNoPicker: true,
      existingOutputReplacement: true,
      structuredPlanReview: true,
      environmentHoverDismissal: true,
      explicitAliasSelection: true,
      certificateOnlyCannotSign: true,
      invalidatedInspection: true,
      passwordErrorInDialog: true,
      passwordVisibility: true,
      passwordsClearedOnClose: true,
      submissionIdempotency: true,
      creationConfirmation: true,
      pendingKeyProtection: true,
      savedKeyCacheCleanup: true,
      installHandoff: true,
      english: true,
      compactDarkLayout: true,
      pageErrors: errors,
    };
    fs.writeFileSync(
      path.resolve(__dirname, '../.qa/signing/ui-report.json'),
      JSON.stringify(report, null, 2),
    );
    console.log(JSON.stringify(report));
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
