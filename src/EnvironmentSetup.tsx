import { useEffect, useRef, useState } from 'react';
import { t } from './i18n';
import { call, errorText, native } from './api';
import { Button, Field, Modal } from './ui';
import { bytes } from './DeviceUtilities';
import type { Environment, Settings, Workspace } from './types';

export function EnvironmentSetup({
  data,
  close,
  done,
}: {
  data: Workspace;
  close: () => void;
  done: (purpose: string) => Promise<void>;
}) {
  const [candidates, setCandidates] = useState<Environment[]>([]);
  const [path, setPath] = useState(data.settings.sdkRoot);
  const [purpose, setPurpose] = useState('reuse');
  const [error, setError] = useState('');
  const [scanning, setScanning] = useState(true);
  const [saving, setSaving] = useState(false);
  const alive = useRef(true);
  const scan = async () => {
    setScanning(true);
    setError('');
    try {
      const found = await call<Environment[]>('environment_scan');
      if (alive.current) setCandidates(found);
    } catch (error) {
      if (alive.current) setError(errorText(error));
    } finally {
      if (alive.current) setScanning(false);
    }
  };
  useEffect(() => {
    alive.current = true;
    void scan();
    return () => {
      alive.current = false;
    };
  }, []);
  return (
    <Modal title={t('选择或创建 SDK 环境')} onClose={close} wide dismissible={!saving} trapFocus>
      <p>
        {t(
          '只扫描配置目录、Android 环境变量和当前用户标准目录；选择后才应用，不合并或搬迁现有 SDK。',
        )}
      </p>
      <Button disabled={scanning || saving || !native} onClick={() => void scan()}>
        {scanning ? t('正在扫描…') : t('重新发现环境')}
      </Button>
      <div className="environment-candidates" aria-busy={scanning}>
        {candidates.map((candidate) => (
          <label key={candidate.path} className="environment-candidate">
            <input
              type="radio"
              name="sdk-candidate"
              checked={path.toLowerCase() === candidate.path.toLowerCase()}
              disabled={saving}
              onChange={() => setPath(candidate.path)}
            />
            <span>
              <strong>{candidate.path}</strong>
              <small>
                {t(candidate.source)} · {t(candidate.writable ? '可写' : '不可写')} ·{' '}
                {t(
                  candidate.adb || candidate.emulator || candidate.commandTools
                    ? '检测到 SDK 工具'
                    : '缺少 SDK 工具',
                )}{' '}
                · {t('可用空间')} {bytes(candidate.freeBytes || 0)}
              </small>
              <small>
                {[candidate.adb, candidate.emulator, candidate.commandTools]
                  .filter(Boolean)
                  .join(' · ')}
              </small>
              {candidate.warnings.map((warning) => (
                <small key={warning}>{t(warning)}</small>
              ))}
            </span>
          </label>
        ))}
        {!scanning && !candidates.length && (
          <p>{t('未发现已有 SDK，可以选择目录并准备新环境。')}</p>
        )}
      </div>
      <Field label={t('SDK 目录')}>
        <div className="input-action">
          <input value={path} disabled={saving} onChange={(event) => setPath(event.target.value)} />
          <Button
            disabled={saving}
            onClick={() => {
              void call<string | null>('file_choose', { kind: 'sdk' })
                .then((value) => {
                  if (value) setPath(value);
                })
                .catch((error) => setError(errorText(error)));
            }}
          >
            {t('选择目录')}
          </Button>
        </div>
      </Field>
      <Field label={t('使用目的')}>
        <select
          value={purpose}
          disabled={saving}
          onChange={(event) => setPurpose(event.target.value)}
        >
          <option value="reuse">{t('复用已有环境')}</option>
          <option value="usb">{t('只连接真机')}</option>
          <option value="emulator">{t('使用模拟器')}</option>
          <option value="build">{t('准备外部构建环境')}</option>
        </select>
      </Field>
      <p className="hint">
        {purpose === 'reuse'
          ? t('保留现有组件和 AVD，不重新下载。缺少的能力可稍后补齐。')
          : purpose === 'build'
            ? t(
                '应用后进入 SDK 管理，按项目需要选择 SDK Platform 和 Build Tools；项目构建仍需完整 JDK。',
              )
            : t('应用后刷新目录并展示最小安装计划，接受许可后才安装。系统镜像在创建模拟器时选择。')}
      </p>
      {error && (
        <p className="error-banner" role="alert">
          {t(error)}
        </p>
      )}
      <div className="modal-actions">
        <Button disabled={saving} onClick={close}>
          {t('稍后配置')}
        </Button>
        <Button
          primary
          disabled={!path.trim() || scanning || saving}
          onClick={() => {
            if (saving) return;
            setSaving(true);
            setError('');
            void (async () => {
              const latest = await call<Workspace>('workspace_snapshot');
              const settings: Settings = { ...latest.settings, sdkRoot: path.trim() };
              await call('settings_save', { settings });
              await done(purpose);
            })()
              .catch((error) => {
                if (alive.current) setError(errorText(error));
              })
              .finally(() => {
                if (alive.current) setSaving(false);
              });
          }}
        >
          {saving ? t('正在应用…') : t('应用所选环境')}
        </Button>
      </div>
    </Modal>
  );
}
