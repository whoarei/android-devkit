import { useEffect, useState } from 'react';
import { Globe2, ShieldCheck } from 'lucide-react';
import { call, errorText } from './api';
import { t } from './i18n';
import { Button, Field, Modal } from './ui';
import type { SdkDownloadSource, SdkSourceCheck, Settings } from './bindings';
import './sdk-download.css';

export const defaultSdkDownload = (): SdkDownloadSource => ({
  kind: 'google',
  name: '',
  url: '',
  verifyOfficial: true,
});
export const sdkSourceName = (source?: SdkDownloadSource) =>
  source?.kind === 'tencent'
    ? t('腾讯云镜像')
    : source?.kind === 'custom'
      ? source.name
      : t('Google 官方');
const sourceBase = (source: SdkDownloadSource) =>
  source.kind === 'google'
    ? 'https://dl.google.com/android/repository/'
    : source.kind === 'tencent'
      ? 'https://mirrors.cloud.tencent.com/AndroidSDK/'
      : source.url;
const paths = [
  ['SDK 组件', 'repository2-3.xml'],
  ['AOSP 系统镜像', 'sys-img/android/sys-img2-3.xml'],
  ['Google APIs 系统镜像', 'sys-img/google_apis/sys-img2-3.xml'],
  ['Google Play 系统镜像', 'sys-img/google_apis_playstore/sys-img2-3.xml'],
];

export function SdkDownloadSettings({
  settings,
  sdkActive,
  selectedCount,
  applied,
}: {
  settings: Settings;
  sdkActive: boolean;
  selectedCount: number;
  applied: () => Promise<unknown>;
}) {
  const saved = JSON.stringify(settings.sdkDownload ?? defaultSdkDownload());
  const [draft, setDraft] = useState<SdkDownloadSource>(JSON.parse(saved));
  const [check, setCheck] = useState<SdkSourceCheck | null>(null);
  const [checked, setChecked] = useState('');
  const [working, setWorking] = useState<'check' | 'apply' | ''>('');
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');
  const [confirm, setConfirm] = useState(false);
  useEffect(() => {
    setDraft(JSON.parse(saved));
    setCheck(null);
    setChecked('');
  }, [saved]);
  const fingerprint = JSON.stringify([draft, settings.proxy, settings.sdkChannel]);
  const changed = JSON.stringify(draft) !== saved;
  const valid =
    check &&
    checked === fingerprint &&
    !check.archiveError &&
    check.sites.length === 4 &&
    check.sites.every((site) => !site.error && site.packageCount > 0);
  const update = (values: Partial<SdkDownloadSource>) => {
    setDraft((value) => ({ ...value, ...values }));
    setCheck(null);
    setChecked('');
    setError('');
    setSuccess('');
  };
  const test = async () => {
    setWorking('check');
    setError('');
    setSuccess('');
    setCheck(null);
    const source = { ...draft, name: draft.name.trim(), url: draft.url.trim() };
    if (source.kind === 'custom') {
      try {
        const url = new URL(source.url);
        if (!url.pathname.endsWith('/')) url.pathname += '/';
        source.url = url.toString();
      } catch {
        /* Rust returns the validation error. */
      }
    }
    setDraft(source);
    try {
      setCheck(await call<SdkSourceCheck>('sdk_source_check', { source }));
      setChecked(JSON.stringify([source, settings.proxy, settings.sdkChannel]));
    } catch (failure) {
      setError(errorText(failure));
    } finally {
      setWorking('');
    }
  };
  const apply = async () => {
    if (!valid || !changed || sdkActive || working) return;
    setWorking('apply');
    setConfirm(false);
    setError('');
    setSuccess('');
    try {
      const warnings = await call<string[]>('sdk_source_apply', { source: draft });
      await applied();
      setCheck(null);
      setChecked('');
      setSuccess(warnings.length ? warnings.join('; ') : t('下载源已切换，目录已刷新'));
    } catch (failure) {
      setError(errorText(failure));
      setChecked('');
    } finally {
      setWorking('');
    }
  };
  const base = sourceBase(draft);
  return (
    <section className="sdk-download-settings" aria-label={t('SDK 下载源')}>
      <div className="sdk-download-heading">
        <div>
          <h3>{t('SDK 下载源')}</h3>
          <p className="hint">{t('仓库目录、SDK 工具和系统镜像从同一来源获取。')}</p>
        </div>
        {changed && <span className="sdk-source-tag">{t('待应用')}</span>}
      </div>
      <fieldset
        className="sdk-source-options"
        disabled={!!working}
        aria-label={t('选择 SDK 下载源')}
      >
        {(
          [
            ['google', 'Google 官方', 'Google 原始仓库'],
            ['tencent', '腾讯云镜像', '国内网络可选'],
            ['custom', '自定义源', 'HTTPS 仓库根地址'],
          ] as const
        ).map(([kind, name, hint]) => (
          <label
            key={kind}
            className={`sdk-source-option ${draft.kind === kind ? 'selected' : ''}`}
          >
            <input
              type="radio"
              name="sdk-download-source"
              checked={draft.kind === kind}
              onChange={() =>
                update({
                  kind,
                  ...(kind === 'custom' && !draft.name ? { name: t('自定义 SDK 镜像') } : {}),
                })
              }
            />
            <span>
              <strong>{t(name)}</strong>
              <small>{t(hint)}</small>
              <small className="sdk-source-current">
                {(settings.sdkDownload?.kind ?? 'google') === kind ? t('当前使用') : '\u00a0'}
              </small>
            </span>
          </label>
        ))}
      </fieldset>
      <div className="sdk-source-addresses">
        {draft.kind === 'custom' ? (
          <>
            <Field label={t('源名称')}>
              <input
                disabled={!!working}
                maxLength={60}
                value={draft.name}
                onChange={(e) => update({ name: e.target.value })}
              />
            </Field>
            <Field
              label={t('仓库根地址')}
              hint={t('保留 Android 官方目录结构；末尾的 / 会自动补齐。')}
            >
              <input
                disabled={!!working}
                value={draft.url}
                placeholder="https://sdk.example.com/android/repository/"
                onChange={(e) => update({ url: e.target.value })}
              />
            </Field>
          </>
        ) : (
          <>
            <span className="hint">{t('仓库根地址')}</span>
            <code>{base}</code>
          </>
        )}
        <details>
          <summary>{t('查看 4 类仓库地址')}</summary>
          <dl>
            {paths.map(([name, path]) => (
              <div key={path}>
                <dt>{t(name)}</dt>
                <dd>
                  <code>{base ? `${base.replace(/\/$/, '')}/${path}` : path}</code>
                </dd>
              </div>
            ))}
          </dl>
        </details>
      </div>
      <div className="sdk-source-verification">
        <ShieldCheck size={18} />
        <div>
          <label className="check-row">
            <input
              type="checkbox"
              disabled={!!working}
              checked={draft.verifyOfficial}
              onChange={(e) => update({ verifyOfficial: e.target.checked })}
            />
            {t('使用 Google 官方校验值验证安装包')}
          </label>
          <p className="hint">
            {draft.verifyOfficial
              ? t('默认开启。官方暂不可达时使用已保存的官方记录；缺少匹配记录的组件暂不可安装。')
              : t('官方校验已关闭。仍按所选下载源提供的校验值验证完整性，不标记为官方验证。')}
          </p>
        </div>
      </div>
      {check && (
        <div className={`sdk-source-result ${valid ? '' : 'warning-text'}`} role="status">
          <strong>{valid ? t('连接检查通过') : t('连接检查未通过，当前下载源保持不变')}</strong>
          <div className="sdk-source-checks">
            {check.sites.map((site) => (
              <div key={site.url}>
                <span>{t(site.name)}</span>
                <span>
                  {site.error ? t(site.error) : t('{0} 个组件 · 可访问', site.packageCount)}
                </span>
              </div>
            ))}
            <div>
              <span>{t('Windows 工具安装包')}</span>
              <span>{check.archiveError ? t(check.archiveError) : t('可访问')}</span>
            </div>
          </div>
        </div>
      )}
      {error && (
        <p className="error-banner" role="alert">
          {t(error)}
        </p>
      )}
      {success && (
        <p className="sdk-source-result" role="status">
          {success}
        </p>
      )}
      <div className="sdk-source-actions">
        <p className="hint">
          {sdkActive
            ? t('SDK 安装任务进行中，结束后可应用')
            : !changed
              ? t('当前配置已生效')
              : t('先检查连接，通过后应用。')}
        </p>
        <Button
          disabled={!changed || !!working}
          onClick={() => {
            setDraft(JSON.parse(saved));
            setCheck(null);
            setChecked('');
            setError('');
            setSuccess('');
          }}
        >
          {t('撤销更改')}
        </Button>
        <Button loading={working === 'check'} disabled={!!working} onClick={() => void test()}>
          {working === 'check' ? t('检查中…') : t('检查连接')}
        </Button>
        <Button
          primary
          loading={working === 'apply'}
          disabled={!valid || !changed || sdkActive || !!working}
          onClick={() => (selectedCount ? setConfirm(true) : void apply())}
        >
          {t('应用并刷新')}
        </Button>
      </div>
      {confirm && (
        <Modal title={t('应用新的下载源？')} onClose={() => setConfirm(false)}>
          <p>
            {t('刷新目录后将清空当前 {0} 项待安装选择，请按新目录重新选择组件。', selectedCount)}
          </p>
          <div className="dialog-actions">
            <Button onClick={() => setConfirm(false)}>{t('继续编辑')}</Button>
            <Button primary onClick={() => void apply()}>
              {t('应用并刷新目录')}
            </Button>
          </div>
        </Modal>
      )}
    </section>
  );
}

export function SdkSourceBar({ source, open }: { source?: SdkDownloadSource; open: () => void }) {
  return (
    <div className="sdk-source-bar">
      <Globe2 size={17} />
      <span className="muted">{t('下载源')}</span>
      <strong>{sdkSourceName(source)}</strong>
      <small>{t('索引与安装包同步切换')}</small>
      <button className="text-button" onClick={open}>
        {t('更换来源')}
      </button>
    </div>
  );
}
