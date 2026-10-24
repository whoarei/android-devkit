import { useEffect, useState } from 'react';
import { Folder, Layers, Video, Upload, Download, RefreshCw, ArrowUp } from 'lucide-react';
import { call, errorText, size, taskActive } from './api';
import { t } from './i18n';
import { Button, Field, Modal } from './ui';
import type { ApkSet, Device, Target, Task, RemoteEntry, TransferSet } from './bindings';
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
}: {
  target: Device;
  tasks: Task[];
  prepare: Prepare;
  planOpen: boolean;
}) {
  const [mode, setMode] = useState<'apk' | 'record' | 'files' | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [apk, setApk] = useState<ApkSet | null>(null);
  const [toolPackage, setToolPackage] = useState('');
  const checkTools = async (refreshCatalog = false) => {
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
  const [seconds, setSeconds] = useState(30);
  const [resolution, setResolution] = useState('original');
  const [path, setPath] = useState('/sdcard');
  const [entries, setEntries] = useState<RemoteEntry[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [transfer, setTransfer] = useState<TransferSet | null>(null);
  const [conflictMode, setConflictMode] = useState('fail');
  const [renames, setRenames] = useState<Record<string, string>>({});
  const reviewTransfer = (value: TransferSet) => {
    setTransfer(value);
    setConflictMode('fail');
    setRenames(
      Object.fromEntries(
        value.conflicts.map((root) => {
          const directory = value.items.find((m) => m.relative === root)?.directory;
          const dot = directory ? -1 : root.lastIndexOf('.');
          return [
            root,
            dot > 0 ? `${root.slice(0, dot)} (copy)${root.slice(dot)}` : `${root} (copy)`,
          ];
        }),
      ),
    );
  };
  const ready = target.state === 'ready';
  const run = async (work: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      await work();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const browse = async (next = path) => {
    const list = await call<RemoteEntry[]>('device_files', { target, path: next });
    setEntries(list);
    setSelected([]);
    setPath(next);
    setTransfer(null);
  };
  const prepareUpload = async (paths: string[]) => {
    if (!paths.length) return;
    if (path === '/sdcard') throw new Error(t('请先打开一个共享存储目录'));
    reviewTransfer(
      await call<TransferSet>('transfer_prepare', {
        direction: 'upload',
        paths,
        target,
        directory: path,
      }),
    );
  };
  useEffect(() => {
    if (mode !== 'files' || planOpen) return;
    document.body.dataset.phoneDrop = 'upload';
    const drop = (event: Event) => {
      void run(() => prepareUpload((event as CustomEvent<string[]>).detail));
    };
    window.addEventListener('devkit-phone-drop', drop);
    return () => {
      delete document.body.dataset.phoneDrop;
      window.removeEventListener('devkit-phone-drop', drop);
    };
  }, [mode, path, target, planOpen, busy]);
  const open = (next: 'apk' | 'record' | 'files') => {
    setMode(next);
    setError('');
    setTransfer(null);
    if (next === 'files') void run(() => browse('/sdcard'));
    if (next === 'apk') void run(() => checkTools());
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
      task.state === 'needs_review' &&
      !task.artifact &&
      task.operation.target?.serial === target.serial,
  );
  const interruptedTransfers = tasks.filter(
    (task) =>
      ['file_upload', 'file_download'].includes(task.operation.kind) &&
      ['needs_review', 'cancelled', 'failed'].includes(task.state) &&
      task.operation.target?.serial === target.serial,
  );
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
      {mode && (
        <Modal
          title={t(mode === 'apk' ? '分包 APK 安装' : mode === 'record' ? '录屏' : '文件传输')}
          onClose={() => {
            if (!busy) setMode(null);
          }}
          wide
          hidden={planOpen}
          dismissible={!busy}
          className="phone-modal"
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
                        if (paths.length) setApk(await call<ApkSet>('apk_set_prepare', { paths }));
                      })
                    }
                  >
                    {t(kind === 'apk_files' ? '选择多个 APK…' : '选择 APK 目录…')}
                  </Button>
                ))}
                {toolPackage && (
                  <Button
                    disabled={busy}
                    onClick={() => void prepare('sdk_install', { packages: toolPackage })}
                  >
                    {t('准备 Build-Tools 依赖计划')}
                  </Button>
                )}
                <Button disabled={busy} onClick={() => void prepare('java_prepare')}>
                  {t('准备 Java 运行时')}
                </Button>
                <Button disabled={busy} onClick={() => void run(() => checkTools(true))}>
                  {t('检查校验工具，缺目录时刷新')}
                </Button>
              </div>
              {busy && <p role="status">{t('正在暂存并校验文件，请稍候…')}</p>}
              {apk && (
                <div className="phone-review" data-apk-set={apk.id}>
                  <strong>
                    {apk.package} · {apk.version} · versionCode={apk.versionCode}
                  </strong>
                  <p className="hint">
                    {t('签名证书 SHA-256')}：{apk.signers.join(', ')}
                  </p>
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
                    onClick={() => void prepare('apk_set_install', { setId: apk.id }, target)}
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
                    onChange={(e) => setResolution(e.target.value)}
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
                    {t('停止并保存录屏')}
                  </Button>
                </div>
              ) : (
                <Button
                  primary
                  disabled={
                    busy || !ready || !Number.isInteger(seconds) || seconds < 1 || seconds > 180
                  }
                  onClick={() =>
                    void prepare('screen_record', { seconds: String(seconds), resolution }, target)
                  }
                >
                  {t('确认录屏计划')}
                </Button>
              )}
              {recoverable.map((task) => (
                <div className="phone-review" key={task.id}>
                  <p>{t(task.message)}</p>
                  <Button
                    disabled={busy || !ready}
                    onClick={() =>
                      void prepare('screen_recover', { sourceTaskId: task.id }, target)
                    }
                  >
                    {t('取回原录屏')}
                  </Button>
                </div>
              ))}
            </>
          )}
          {mode === 'files' && (
            <>
              {interruptedTransfers.map((task) => (
                <div className="phone-review" key={task.id}>
                  <p>{t(task.message)}</p>
                  {task.state === 'needs_review' &&
                    task.operation.args.conflicts === 'replace' &&
                    task.operation.args.backupsResolved !== 'true' && (
                      <Button
                        disabled={busy || !ready}
                        onClick={() =>
                          void run(() => prepare('file_restore', { sourceTaskId: task.id }, target))
                        }
                      >
                        {t('恢复被替换的旧目标')}
                      </Button>
                    )}
                  <Button
                    disabled={busy || !ready}
                    onClick={() =>
                      void run(() => prepare('file_cleanup', { sourceTaskId: task.id }, target))
                    }
                  >
                    {t('清理原传输暂存')}
                  </Button>
                </div>
              ))}
              <div className="phone-toolbar">
                <Button
                  disabled={busy || path === '/sdcard' || !ready}
                  onClick={() => void run(() => browse(path.slice(0, path.lastIndexOf('/'))))}
                >
                  <ArrowUp size={14} />
                  {t('上级目录')}
                </Button>
                <Button disabled={busy || !ready} onClick={() => void run(() => browse())}>
                  <RefreshCw size={14} />
                  {t('刷新')}
                </Button>
                <span className="phone-path">{path}</span>
              </div>
              <div className="phone-toolbar">
                {['upload_files', 'upload_directory'].map((kind) => (
                  <Button
                    key={kind}
                    disabled={busy || !ready || path === '/sdcard'}
                    onClick={() =>
                      void run(async () =>
                        prepareUpload(await call<string[]>('phone_choose', { kind })),
                      )
                    }
                  >
                    <Upload size={14} />
                    {t(kind === 'upload_files' ? '上传文件…' : '上传目录…')}
                  </Button>
                ))}
                <Button
                  disabled={busy || !ready || !selected.length}
                  onClick={() =>
                    void run(async () => {
                      const [directory] = await call<string[]>('phone_choose', {
                        kind: 'download_directory',
                      });
                      if (directory)
                        reviewTransfer(
                          await call<TransferSet>('transfer_prepare', {
                            direction: 'download',
                            paths: selected,
                            target,
                            directory,
                          }),
                        );
                    })
                  }
                >
                  <Download size={14} />
                  {t('下载所选…')}
                </Button>
              </div>
              <p className="hint">
                {t(
                  '打开共享目录后可拖入文件或目录上传。仅访问共享存储；不跟随链接，同名目标需明确选择保留、重命名或替换。单次最多 10000 个条目、4 GiB。',
                )}
              </p>
              <div className="phone-file-list" aria-busy={busy}>
                <table>
                  <thead>
                    <tr>
                      <th>{t('选择')}</th>
                      <th>{t('名称')}</th>
                      <th>{t('类型')}</th>
                      <th>{t('大小')}</th>
                      <th>{t('修改时间')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {entries.map((entry) => (
                      <tr key={entry.path}>
                        <td>
                          <input
                            type="checkbox"
                            aria-label={t('选择：{0}', entry.name)}
                            disabled={busy || entry.link || path === '/sdcard'}
                            checked={selected.includes(entry.path)}
                            onChange={(e) =>
                              setSelected((previous) =>
                                e.target.checked
                                  ? [...previous, entry.path]
                                  : previous.filter((p) => p !== entry.path),
                              )
                            }
                          />
                        </td>
                        <td>
                          {entry.directory && !entry.link ? (
                            <button
                              className="text-button"
                              disabled={busy}
                              onClick={() => void run(() => browse(entry.path))}
                            >
                              <Folder size={14} />
                              {entry.name}
                            </button>
                          ) : (
                            entry.name
                          )}
                        </td>
                        <td>
                          {t(entry.link ? '链接或不支持的名称' : entry.directory ? '目录' : '文件')}
                        </td>
                        <td>
                          {entry.directory
                            ? '—'
                            : entry.bytes < 1024
                              ? `${entry.bytes} B`
                              : size(entry.bytes)}
                        </td>
                        <td>
                          {entry.modified ? new Date(entry.modified * 1000).toLocaleString() : '—'}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {!entries.length && !busy && <p className="hint">{t('目录为空')}</p>}
              </div>
              {busy && <p role="status">{t('正在读取并准备文件清单，请稍候…')}</p>}
              {transfer && (
                <div className="phone-review" data-transfer-id={transfer.id}>
                  <strong>
                    {t(
                      '已准备 {0} 项 · {1} 字节',
                      transfer.roots.length,
                      transfer.items.reduce((sum, item) => sum + item.bytes, 0),
                    )}
                  </strong>
                  <p className="phone-path">{transfer.directory}</p>
                  <ul>
                    {transfer.roots.map((root) => (
                      <li key={root}>
                        {root}
                        {transfer.conflicts.includes(root) ? ` · ${t('目标已存在')}` : ''}
                      </li>
                    ))}
                  </ul>
                  {!!transfer.conflicts.length && (
                    <>
                      <label className="check-row">
                        <input
                          type="radio"
                          name="fileConflict"
                          checked={conflictMode === 'skip'}
                          onChange={() => setConflictMode('skip')}
                        />
                        {t('跳过全部同名冲突项，保留现有文件')}
                      </label>
                      <label className="check-row">
                        <input
                          type="radio"
                          name="fileConflict"
                          checked={conflictMode === 'rename'}
                          onChange={() => setConflictMode('rename')}
                        />
                        {t('为冲突项指定新名称')}
                      </label>
                      {conflictMode === 'rename' &&
                        transfer.conflicts.map((root) => (
                          <Field key={root} label={t('新名称：{0}', root)}>
                            <input
                              value={renames[root] ?? ''}
                              onChange={(e) =>
                                setRenames((previous) => ({ ...previous, [root]: e.target.value }))
                              }
                            />
                          </Field>
                        ))}
                      <label className="check-row">
                        <input
                          type="radio"
                          name="fileConflict"
                          checked={conflictMode === 'replace'}
                          onChange={() => setConflictMode('replace')}
                        />
                        {t('替换已核对的同名目标及其全部内容')}
                      </label>
                      {conflictMode === 'replace' && (
                        <p className="warning-text">
                          {t(
                            '替换前复核内容摘要；不合并目录。全部发布成功后清理旧内容，中断时保留原件备份，可从原任务恢复。',
                          )}
                        </p>
                      )}
                    </>
                  )}
                  <Button
                    primary
                    disabled={
                      busy ||
                      !ready ||
                      (!!transfer.conflicts.length && conflictMode === 'fail') ||
                      (conflictMode === 'skip' &&
                        transfer.roots.every((r) => transfer.conflicts.includes(r))) ||
                      (conflictMode === 'rename' &&
                        transfer.conflicts.some((r) => !renames[r]?.trim()))
                    }
                    onClick={() =>
                      void run(() =>
                        prepare(
                          `file_${transfer.direction}`,
                          {
                            transferId: transfer.id,
                            conflicts: conflictMode,
                            ...(conflictMode === 'rename'
                              ? { renames: JSON.stringify(renames) }
                              : {}),
                          },
                          target,
                        ),
                      )
                    }
                  >
                    {t('确认传输计划')}
                  </Button>
                </div>
              )}
            </>
          )}
        </Modal>
      )}
    </>
  );
}
