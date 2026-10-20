import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Download, ExternalLink, RefreshCw } from 'lucide-react';
import { version } from '../package.json';
import { call, errorText, native, size } from './api';
import { t } from './i18n';
import { Button, Modal } from './ui';
import type { AppUpdateInfo, AppUpdateProgress, CloseStatus } from './bindings';

const preferenceKey = 'android-devkit.auto-check-update';
type Phase =
  'idle' | 'checking' | 'uptodate' | 'available' | 'downloading' | 'verifying' | 'installing';

export function useSoftwareUpdate() {
  const [info, setInfo] = useState<AppUpdateInfo | null>(null);
  const [phase, setPhase] = useState<Phase>('idle');
  const [error, setError] = useState('');
  const [confirmation, setConfirmation] = useState<AppUpdateInfo | null>(null);
  const [progress, setProgress] = useState<AppUpdateProgress | null>(null);
  const [autoCheck, setAutoCheck] = useState(() => {
    try {
      return localStorage.getItem(preferenceKey) !== 'false';
    } catch {
      return true;
    }
  });
  const busy = useRef(false);
  const check = useCallback(async (silent = false) => {
    if (!native || busy.current) return;
    busy.current = true;
    setPhase('checking');
    setError('');
    setInfo(null);
    try {
      const result = await call<AppUpdateInfo | null>('check_app_update');
      setInfo(result);
      setPhase(result ? 'available' : 'uptodate');
    } catch (e) {
      setPhase('idle');
      if (!silent) setError(errorText(e));
    } finally {
      busy.current = false;
    }
  }, []);
  useEffect(() => {
    if (autoCheck) void check(true);
  }, [autoCheck, check]);
  const changeAutoCheck = (enabled: boolean) => {
    setAutoCheck(enabled);
    try {
      localStorage.setItem(preferenceKey, String(enabled));
    } catch {
      /* session preference */
    }
  };
  const openDownload = async () => {
    try {
      await call('open_app_download');
    } catch (e) {
      setError(errorText(e));
    }
  };
  const requestInstall = async () => {
    if (!info || busy.current) return;
    busy.current = true;
    setError('');
    try {
      const status = await call<CloseStatus>('close_status');
      if (status.draining || status.tasks.length)
        throw { message: '请先完成或停止所有任务和日志采集，再安装更新。' };
      if (status.unsaved.length) throw { message: '请先保存或清理待保存文件，再安装更新。' };
      setConfirmation(info);
    } catch (e) {
      setError(errorText(e));
    } finally {
      busy.current = false;
    }
  };
  const install = async () => {
    if (!confirmation || busy.current) return;
    busy.current = true;
    setError('');
    setProgress(null);
    setPhase('downloading');
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<AppUpdateProgress>('app-update-progress', ({ payload }) => {
        if (!['downloading', 'verifying', 'installing'].includes(payload.phase)) return;
        setPhase(payload.phase as Phase);
        setProgress(payload);
      });
      await call('install_app_update', { updateId: confirmation.updateId });
      throw { message: '更新安装未完成，请重试或前往下载页面。' };
    } catch (e) {
      setError(errorText(e));
      setPhase('available');
      setConfirmation(null);
    } finally {
      unlisten?.();
      busy.current = false;
    }
  };
  const updating = ['downloading', 'verifying', 'installing'].includes(phase);
  return {
    info,
    phase,
    error,
    confirmation,
    progress,
    autoCheck,
    check,
    changeAutoCheck,
    openDownload,
    requestInstall,
    install,
    updating,
    closeConfirmation: () => setConfirmation(null),
  };
}
type Updater = ReturnType<typeof useSoftwareUpdate>;

export function SoftwareUpdateSection({ updater }: { updater: Updater }) {
  const { info, phase, error } = updater;
  return (
    <section className="software-update" aria-label={t('软件更新')}>
      <h3>{t('软件更新')}</h3>
      <p className="hint">{t('当前版本：{0}', version)}</p>
      <label className="check-row">
        <input
          type="checkbox"
          checked={updater.autoCheck}
          disabled={updater.updating}
          onChange={(e) => updater.changeAutoCheck(e.target.checked)}
        />
        {t('启动时自动检查更新')}
      </label>
      <div className="update-actions">
        <Button
          disabled={!native || phase === 'checking' || updater.updating}
          onClick={() => void updater.check()}
        >
          <RefreshCw size={15} className={phase === 'checking' ? 'spin' : ''} />
          {phase === 'checking' ? t('正在检查更新…') : t('检查更新')}
        </Button>
        {info?.canAutoInstall && (
          <Button primary disabled={updater.updating} onClick={() => void updater.requestInstall()}>
            <Download size={15} />
            {t('下载并安装更新')}
          </Button>
        )}
        <Button disabled={!native || updater.updating} onClick={() => void updater.openDownload()}>
          <ExternalLink size={15} />
          {t('前往下载页面')}
        </Button>
      </div>
      {phase === 'uptodate' && <p role="status">{t('当前已是最新版本。')}</p>}
      {info && (
        <div className="update-details" role="status">
          <b>{t('发现新版本：{0}', info.latestVersion)}</b>
          {!info.canAutoInstall && <p>{t('当前为便携版或开发版，请下载新版本后手动替换。')}</p>}
          {info.notes && <pre className="update-notes">{info.notes}</pre>}
        </div>
      )}
      {error && (
        <p className="warning-text" role="alert">
          {t(error)}
        </p>
      )}
      {!native && <p className="hint">{t('请在桌面应用中检查软件更新。')}</p>}
    </section>
  );
}

export function SoftwareUpdateDialog({ updater }: { updater: Updater }) {
  if (!updater.confirmation) return null;
  const progress = updater.progress;
  const total = progress?.total && progress.total > 0 ? progress.total : undefined;
  const downloaded = Math.max(0, progress?.downloaded ?? 0);
  return (
    <Modal
      title={t('安装软件更新')}
      onClose={updater.closeConfirmation}
      dismissible={!updater.updating}
    >
      <p>{t('将从 {0} 更新到 {1}。', version, updater.confirmation.latestVersion)}</p>
      {!updater.updating ? (
        <>
          <p className="hint">
            {t('更新包通过签名校验后才会安装。安装时将关闭工作台，完成后重新打开。')}
          </p>
          <div className="modal-actions">
            <Button onClick={updater.closeConfirmation}>{t('取消')}</Button>
            <Button primary onClick={() => void updater.install()}>
              {t('确认更新并重启')}
            </Button>
          </div>
        </>
      ) : (
        <div className="update-progress" role="status" aria-live="polite">
          {updater.phase === 'downloading' ? (
            <>
              <progress
                aria-label={t('更新下载进度')}
                max={total}
                value={total ? Math.min(downloaded, total) : undefined}
              />
              <p>
                {total
                  ? t(
                      '已下载 {0} / {1}（{2}%）',
                      size(downloaded),
                      size(total),
                      Math.min(100, Math.floor((downloaded * 100) / total)),
                    )
                  : t('已下载 {0}', downloaded ? size(downloaded) : '0 B')}
              </p>
            </>
          ) : (
            <p>
              <RefreshCw size={16} className="spin" />
              {updater.phase === 'verifying'
                ? t('正在校验更新签名…')
                : t('正在安装更新，即将重新打开应用…')}
            </p>
          )}
        </div>
      )}
    </Modal>
  );
}
