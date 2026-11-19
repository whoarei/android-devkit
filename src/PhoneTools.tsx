import { useEffect, useRef, useState } from 'react';
import { usePreference, useNumberPreference } from './preferences';
import { Folder, Layers, Video, Download, RefreshCw } from 'lucide-react';
import { FileTransfer } from './FileTransfer';
import { call, errorText, size, taskActive } from './api';
import { t } from './i18n';
import { Button, Field, Modal } from './ui';
import type { ApkSet, Device, Target, Task, DeviceToolStatus } from './bindings';
import './phone-tools.css';

type Prepare = (
  kind: string,
  args?: Record<string, string>,
  target?: Target | null,
) => Promise<void>;
export function PhoneTools({
  target,
  tasks,
  prepare,
  planOpen,
  onTransferSubmitted,
}: {
  target: Device;
  tasks: Task[];
  prepare: Prepare;
  planOpen: boolean;
  onTransferSubmitted: (task: Task) => void;
}) {
  const [mode, setMode] = useState<'apk' | 'record' | 'files' | null>(null);
  const [busy, setBusy] = useState(false);
  const working = useRef(false);
  const [error, setError] = useState('');
  const [apk, setApk] = useState<ApkSet | null>(null);
  const [toolPackage, setToolPackage] = useState('');
  const [bundletoolReady, setBundletoolReady] = useState(false);
  const [containerPath, setContainerPath] = useState('');
  const importContainer = async (path: string) => {
    if (apk) await call('phone_selection_release', { kind: 'apk', id: apk.id });
    setApk(null);
    setContainerPath(path);
    setApk(await call<ApkSet>('apk_container_prepare', { path, target }));
  };
  const checkTools = async (refreshCatalog = false) => {
    const tools = await call<DeviceToolStatus[]>('device_tools');
    setBundletoolReady(tools?.some((tool) => tool.name === 'bundletool' && tool.ready) ?? false);
    try {
      setToolPackage(await call<string>('apk_set_tools'));
    } catch (e) {
      if (
        !refreshCatalog ||
        typeof e !== 'object' ||
        e === null ||
        !('code' in e) ||
        e.code !== 'CATALOG_REQUIRED'
      )
        throw e;
      await call('catalog_refresh');
      setToolPackage(await call<string>('apk_set_tools'));
    }
  };
  const [seconds, setSeconds] = useNumberPreference('recordingSeconds', 1, 180);
  const [resolution, setResolution] = usePreference('recordingResolution');
  const ready = target.state === 'ready';
  const run = async (work: () => Promise<void>) => {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    setError('');
    try {
      await work();
    } catch (e) {
      setError(errorText(e));
    } finally {
      working.current = false;
      setBusy(false);
    }
  };
  const open = (next: 'apk' | 'record' | 'files') => {
    setMode(next);
    setError('');
  };
  const activeRecording = tasks.find(
    (task) =>
      task.operation.kind === 'screen_record' &&
      taskActive(task.state) &&
      task.operation.target?.serial === target.serial,
  );
  const recoverable = tasks.filter(
    (task) =>
      task.operation.kind === 'screen_record' &&
      !taskActive(task.state) &&
      ((task.state === 'needs_review' && !task.artifact) ||
        task.operation.args.deviceCleanupPending === 'true') &&
      task.operation.target?.serial === target.serial,
  );
  const preparedDependencies = tasks
    .filter(
      (task) =>
        ['sdk_install', 'java_prepare', 'device_tool_prepare'].includes(task.operation.kind) &&
        !taskActive(task.state),
    )
    .map((task) => task.id)
    .sort()
    .join(',');
  useEffect(() => {
    if (mode === 'apk' && !planOpen) void run(() => checkTools());
  }, [mode, planOpen, preparedDependencies]);
  return (
    <>
      <div className="detail-actions phone-tools" aria-label={t('设备常用操作')}>
        <Button disabled={!ready} onClick={() => open('apk')}>
          <Layers size={15} />
          {t('分包 APK 安装')}
        </Button>
        <Button disabled={!ready} onClick={() => open('record')}>
          <Video size={15} />
          {t('录屏')}
        </Button>
        <Button disabled={!ready} onClick={() => open('files')}>
          <Folder size={15} />
          {t('文件传输')}
        </Button>
      </div>
      {mode === 'files' && (
        <FileTransfer
          target={target}
          tasks={tasks}
          prepare={prepare}
          planOpen={planOpen}
          onSubmitted={onTransferSubmitted}
          onClose={() => setMode(null)}
        />
      )}
      {(mode === 'apk' || mode === 'record') && (
        <Modal
          title={t(mode === 'apk' ? '分包 APK 安装' : '录屏')}
          onClose={() => {
            if (!busy)
              void run(async () => {
                if (apk) await call('phone_selection_release', { kind: 'apk', id: apk.id });
                setApk(null);
                setMode(null);
              });
          }}
          wide
          hidden={planOpen}
          dismissible={!busy}
          className="phone-modal"
          trapFocus
        >
          <p className="hint phone-target">
            {t('目标设备')}：{target.model} · {target.serial}
          </p>
          {!ready && (
            <p className="warning-text" role="status">
              {t('目标设备暂不可用，请等待原连接恢复或重新打开操作面板。')}
            </p>
          )}
          {error && (
            <p className="error-banner" role="alert">
              {t(error)}
            </p>
          )}
          {mode === 'apk' && (
            <>
              <p className="hint">
                {t(
                  '选择同一应用的 base 与全部所需 split APK。校验包名、版本码、签名和依赖后，在一次会话中安装并保留数据。',
                )}
              </p>
              <div className="phone-toolbar">
                {['apk_files', 'apk_directory'].map((kind) => (
                  <Button
                    key={kind}
                    disabled={busy || !ready || !!toolPackage}
                    onClick={() =>
                      void run(async () => {
                        const paths = await call<string[]>('phone_choose', { kind });
                        if (paths.length) {
                          if (apk)
                            await call('phone_selection_release', { kind: 'apk', id: apk.id });
                          setApk(null);
                          setContainerPath('');
                          setApk(await call<ApkSet>('apk_set_prepare', { paths }));
                        }
                      })
                    }
                  >
                    {t(kind === 'apk_files' ? '选择多个 APK…' : '选择 APK 目录…')}
                  </Button>
                ))}
                <Button
                  disabled={busy || !ready || !!toolPackage}
                  onClick={() =>
                    void run(async () => {
                      const paths = await call<string[]>('phone_choose', { kind: 'apk_container' });
                      if (paths[0]) await importContainer(paths[0]);
                    })
                  }
                >
                  {t('导入 APKS / XAPK / APKM…')}
                </Button>
                {containerPath && !apk && (
                  <Button
                    disabled={busy || !ready || !!toolPackage}
                    onClick={() => void run(() => importContainer(containerPath))}
                  >
                    {t('重新导入所选安装包')}
                  </Button>
                )}
                {!bundletoolReady && (
                  <Button
                    disabled={busy}
                    onClick={() =>
                      void run(() => prepare('device_tool_prepare', { tool: 'bundletool' }))
                    }
                  >
                    {t('准备 APKS 选包工具')}
                  </Button>
                )}
                {toolPackage && (
                  <Button
                    disabled={busy}
                    onClick={() =>
                      void run(() => prepare('sdk_install', { packages: toolPackage }))
                    }
                  >
                    {t('准备 Build-Tools 依赖计划')}
                  </Button>
                )}
                <Button disabled={busy} onClick={() => void run(() => prepare('java_prepare'))}>
                  {t('准备 Java 运行时')}
                </Button>
                <Button disabled={busy} onClick={() => void run(() => checkTools(true))}>
                  {t('检查校验工具，缺目录时刷新')}
                </Button>
              </div>
              <p className="hint">
                {t(
                  'APKS 按当前设备配置选包；XAPK / APKM 导入 APK 集合。含 OBB 扩展数据的包会明确拒绝，AAB 需先生成 APK 集合。',
                )}
              </p>
              {busy && <p role="status">{t('正在暂存并校验文件，请稍候…')}</p>}
              {apk && (
                <div className="phone-review" data-apk-set={apk.id}>
                  <strong>
                    {apk.package} · {apk.version} · versionCode={apk.versionCode}
                  </strong>
                  <p className="hint">
                    {t('签名证书 SHA-256')}：{apk.signers.join(', ')}
                  </p>
                  {apk.target && (
                    <p className="hint">{t('此集合已绑定当前设备连接；连接变化后请重新导入。')}</p>
                  )}
                  <ul>
                    {apk.members.map((member) => (
                      <li key={member.sha256}>
                        {member.fileName} · {member.split || 'base'} · {size(member.bytes)} · API{' '}
                        {member.minSdk}
                        {member.abis.length ? ` · ${member.abis.join(', ')}` : ''}
                      </li>
                    ))}
                  </ul>
                  <Button
                    primary
                    disabled={busy || !ready}
                    onClick={() =>
                      void run(() => prepare('apk_set_install', { setId: apk.id }, target))
                    }
                  >
                    {t('确认分包安装计划')}
                  </Button>
                </div>
              )}
            </>
          )}
          {mode === 'record' && (
            <>
              <p className="hint">
                {t(
                  '静音 MP4，最长 180 秒。停止后自动取回至待保存文件，验证完成后清理设备临时文件。受保护画面可能黑屏，请保持屏幕方向。',
                )}
              </p>
              <div className="phone-options">
                <Field label={t('录屏时长（秒）')}>
                  <input
                    type="number"
                    min={1}
                    max={180}
                    value={seconds}
                    disabled={busy || !!activeRecording}
                    onChange={(e) => setSeconds(Number(e.target.value))}
                  />
                </Field>
                <Field label={t('分辨率')}>
                  <select
                    value={resolution}
                    disabled={busy || !!activeRecording}
                    onChange={(e) => setResolution(e.target.value as typeof resolution)}
                  >
                    <option value="original">{t('原始分辨率')}</option>
                    <option value="720p">{t('降低分辨率（最长边 1280）')}</option>
                  </select>
                </Field>
              </div>
              {activeRecording ? (
                <div className="phone-review" role="status">
                  <p>{t(activeRecording.message)}</p>
                  <Button
                    disabled={activeRecording.cancelRequested || !activeRecording.cancellable}
                    onClick={() =>
                      void run(async () => {
                        await call('task_cancel', { taskId: activeRecording.id });
                      })
                    }
                  >
                    {t(activeRecording.state === 'queued' ? '取消等待录屏' : '停止并保存录屏')}
                  </Button>
                </div>
              ) : (
                <Button
                  primary
                  disabled={
                    busy || !ready || !Number.isInteger(seconds) || seconds < 1 || seconds > 180
                  }
                  onClick={() =>
                    void run(() =>
                      prepare('screen_record', { seconds: String(seconds), resolution }, target),
                    )
                  }
                >
                  {t('确认录屏计划')}
                </Button>
              )}
              {recoverable.map((task) => (
                <div className="phone-review" key={task.id}>
                  <p>{t(task.message)}</p>
                  {!task.artifact && (
                    <Button
                      disabled={
                        busy ||
                        !ready ||
                        tasks.some(
                          (related) =>
                            taskActive(related.state) &&
                            related.operation.args.sourceTaskId === task.id,
                        )
                      }
                      onClick={() =>
                        void run(() => prepare('screen_recover', { sourceTaskId: task.id }, target))
                      }
                    >
                      {t('取回原录屏')}
                    </Button>
                  )}
                  <Button
                    danger
                    disabled={
                      busy ||
                      !ready ||
                      tasks.some(
                        (related) =>
                          taskActive(related.state) &&
                          related.operation.args.sourceTaskId === task.id,
                      )
                    }
                    onClick={() =>
                      void run(() => prepare('screen_cleanup', { sourceTaskId: task.id }, target))
                    }
                  >
                    {t('放弃并清理原录屏')}
                  </Button>
                </div>
              ))}
            </>
          )}
        </Modal>
      )}
    </>
  );
}
