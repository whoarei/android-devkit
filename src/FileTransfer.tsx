import { useEffect, useRef, useState } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  File,
  Folder,
  FolderOpen,
  Link2,
  RefreshCw,
} from 'lucide-react';
import { call, errorText, size, submitTask, taskActive } from './api';
import type { Device, FileDirectory, FileEntry, Plan, Target, Task, TransferSet } from './bindings';
import { t } from './i18n';
import { Button, Field, Modal } from './ui';

const androidRoot = '/sdcard';
type Side = 'local' | 'android';
type Direction = 'upload' | 'download';
type ConflictPolicy = 'fail' | 'rename' | 'replace';

export function FileTransfer({
  target,
  tasks,
  prepare,
  planOpen,
  onSubmitted,
  onClose,
}: {
  target: Device;
  tasks: Task[];
  prepare: (kind: string, args: Record<string, string>, target: Target) => Promise<void>;
  planOpen: boolean;
  onSubmitted: (task: Task) => void;
  onClose: () => void;
}) {
  const [locations, setLocations] = useState<Record<Side, FileDirectory | null>>({
    local: null,
    android: null,
  });
  const [activity, setActivity] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [transfer, setTransfer] = useState<TransferSet | null>(null);
  const [conflictPolicy, setConflictPolicy] = useState<ConflictPolicy>('fail');
  const [rename, setRename] = useState('');
  const mounted = useRef(false);
  const working = useRef(false);
  const stagedId = useRef('');
  const submission = useRef<{ plan: Plan; requestId: string } | null>(null);
  const review = useRef<HTMLDivElement>(null);
  const ready = target.state === 'ready';
  const busy = activity !== null;
  const locked = busy || !!transfer;
  const { local, android } = locations;
  const androidFolder = android && android.path !== androidRoot ? android.path : '';
  const run = async (work: () => Promise<void>, status = '') => {
    if (working.current) return;
    working.current = true;
    setActivity(status);
    setError('');
    try {
      await work();
    } catch (e) {
      setError(errorText(e));
    } finally {
      working.current = false;
      setActivity(null);
    }
  };
  const browse = async (side: Side, path: string | null) => {
    let directory: FileDirectory;
    if (side === 'local') {
      directory = await call<FileDirectory>('local_files', { path });
    } else {
      const remote = path ?? androidRoot;
      directory = {
        path: remote,
        parent: remote === androidRoot ? null : remote.slice(0, remote.lastIndexOf('/')),
        entries: await call<FileEntry[]>('device_files', { target, path: remote }),
      };
    }
    setLocations((current) => ({ ...current, [side]: directory }));
  };
  const readBoth = async (left: Promise<void>, right: Promise<void>) => {
    const results = await Promise.allSettled([left, right]);
    const failed = results.find((result) => result.status === 'rejected');
    if (failed) throw failed.reason;
  };
  const clearTransfer = () => {
    stagedId.current = '';
    submission.current = null;
    setTransfer(null);
  };
  const releaseTransfer = (id: string) => call('phone_selection_release', { kind: 'transfer', id });
  const discard = async () => {
    if (stagedId.current) await releaseTransfer(stagedId.current);
    clearTransfer();
  };
  useEffect(() => {
    mounted.current = true;
    void run(() =>
      readBoth(
        browse('local', null),
        browse('android', `${androidRoot}/Download`).catch(() => browse('android', null)),
      ),
    );
    return () => {
      mounted.current = false;
      if (stagedId.current) void releaseTransfer(stagedId.current).catch(() => {});
    };
  }, []);

  const fileTasks = tasks.filter(
    (task) =>
      ['file_upload', 'file_download'].includes(task.operation.kind) &&
      task.operation.target?.serial === target.serial,
  );
  const activeTasks = fileTasks.filter((task) => taskActive(task.state));
  const interrupted = fileTasks.filter(
    (task) =>
      ['needs_review', 'cancelled', 'failed'].includes(task.state) &&
      task.operation.args.backupsResolved !== 'true',
  );
  const transferSubmitted =
    !!transfer && fileTasks.some((task) => task.operation.args.transferId === transfer.id);
  const settledTasks = fileTasks.filter((task) => !taskActive(task.state));
  const settledIds = settledTasks.map((task) => task.id);
  const settledKey = settledIds.slice().sort().join(',');
  const initialSettledIds = useRef(new Set(settledIds));
  const latestResult = settledTasks.find(
    (task) => !initialSettledIds.current.has(task.id) && !interrupted.includes(task),
  );
  const lastSettledKey = useRef(settledKey);
  useEffect(() => {
    if (transferSubmitted) clearTransfer();
  }, [transferSubmitted]);
  useEffect(() => {
    if (lastSettledKey.current === settledKey || planOpen || busy || !ready) return;
    lastSettledKey.current = settledKey;
    void run(() =>
      readBoth(browse('local', local?.path ?? null), browse('android', android?.path ?? null)),
    );
  }, [settledKey, planOpen, busy, ready]);
  useEffect(() => {
    if (transfer && !busy && !planOpen) review.current?.scrollIntoView({ block: 'nearest' });
  }, [transfer, busy, planOpen]);

  const submitTransfer = async (selection: TransferSet, policy: ConflictPolicy) => {
    const kind = `file_${selection.direction}`;
    const existing = selection.conflicts[0];
    const args = {
      transferId: selection.id,
      conflicts: policy,
      ...(policy === 'rename' && existing
        ? { renames: JSON.stringify({ [existing]: rename }) }
        : {}),
    };
    if (existing) {
      await prepare(kind, args, target);
      return;
    }
    const attempt = submission.current ?? {
      plan: await call<Plan>('plan_create', { operation: { kind, args, target } }),
      requestId: crypto.randomUUID(),
    };
    if (!mounted.current) return;
    submission.current = attempt;
    try {
      const task = await submitTask(attempt.plan, attempt.requestId);
      clearTransfer();
      onSubmitted(task);
    } catch (e) {
      if (
        e &&
        typeof e === 'object' &&
        'code' in e &&
        ['PLAN_CHANGED', 'PLAN_EXPIRED'].includes(String(e.code))
      ) {
        submission.current = null;
      }
      throw e;
    }
  };
  const startTransfer = async (direction: Direction, path: string) => {
    if (!ready) return;
    const directory = direction === 'upload' ? androidFolder : local?.path;
    if (!directory)
      throw new Error(direction === 'upload' ? '请先打开一个共享存储目录' : '请选择有效的本地目录');
    const selection = await call<TransferSet>('transfer_prepare', {
      direction,
      paths: [path],
      target,
      directory,
    });
    if (!mounted.current) {
      await releaseTransfer(selection.id);
      return;
    }
    stagedId.current = selection.id;
    setTransfer(selection);
    const existing = selection.conflicts[0];
    if (!existing) {
      await submitTransfer(selection, 'fail');
      return;
    }
    setConflictPolicy('fail');
    const isDirectory = selection.items.some(
      (item) => item.relative === existing && item.directory,
    );
    const dot = isDirectory ? -1 : existing.lastIndexOf('.');
    setRename(
      dot > 0 ? `${existing.slice(0, dot)} (copy)${existing.slice(dot)}` : `${existing} (copy)`,
    );
  };
  const queueTransfer = (direction: Direction, path: string) =>
    void run(() => startTransfer(direction, path), '正在准备传输…');
  useEffect(() => {
    if (planOpen) return;
    document.body.dataset.phoneDrop = 'upload';
    const drop = (event: Event) => {
      if (locked) return;
      const paths = (event as CustomEvent<string[]>).detail;
      if (paths.length > 1) setError('每次只能传输一个文件或目录。');
      else if (paths[0]) queueTransfer('upload', paths[0]);
    };
    window.addEventListener('devkit-phone-drop', drop);
    return () => {
      delete document.body.dataset.phoneDrop;
      window.removeEventListener('devkit-phone-drop', drop);
    };
  }, [androidFolder, planOpen, locked, ready]);

  const uploadDisabled = locked || !ready || !androidFolder;
  const downloadDisabled = locked || !ready || !local || !androidFolder;
  const conflict = transfer?.conflicts[0];
  const feedback = activity || (!activeTasks.length && latestResult?.message) || '';
  return (
    <Modal
      title="文件传输"
      onClose={() =>
        void run(async () => {
          await discard();
          onClose();
        })
      }
      wide
      hidden={planOpen}
      dismissible={!busy}
      className="file-transfer-modal"
      trapFocus
    >
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
      <div className="transfer-panes">
        <FilePane
          title="本地"
          direction="upload"
          directory={local}
          disabled={locked}
          transferDisabled={uploadDisabled}
          loading={busy && !local}
          onBrowse={(path) => void run(() => browse('local', path))}
          onRefresh={() => void run(() => browse('local', local?.path ?? null))}
          onChoose={() =>
            void run(async () => {
              const path = await call<string | null>('file_choose', { kind: 'folder' });
              if (path) await browse('local', path);
            })
          }
          onTransfer={(path) => queueTransfer('upload', path)}
        />
        <FilePane
          title="Android"
          direction="download"
          device={target.model ? `${target.model} · ${target.serial}` : target.serial}
          directory={android}
          disabled={locked || !ready}
          transferDisabled={downloadDisabled}
          loading={busy && !android}
          onBrowse={(path) => void run(() => browse('android', path))}
          onRefresh={() => void run(() => browse('android', android?.path ?? null))}
          onTransfer={(path) => queueTransfer('download', path)}
        />
      </div>
      <p className="transfer-feedback" role="status">
        {t(feedback)}
      </p>
      {transfer && (!busy || conflict) && (
        <div className="phone-review transfer-review" data-transfer-id={transfer.id} ref={review}>
          <strong>{t(conflict ? '同名文件或目录已存在' : '传输已准备')}</strong>
          {conflict && (
            <>
              <p className="phone-path">{conflict}</p>
              <fieldset disabled={busy}>
                <legend className="sr-only">{t('同名文件或目录已存在')}</legend>
                {(
                  [
                    ['rename', '重命名'],
                    ['replace', '替换同名目标'],
                  ] as const
                ).map(([policy, label]) => (
                  <label className="check-row" key={policy}>
                    <input
                      type="radio"
                      name="fileConflict"
                      checked={conflictPolicy === policy}
                      onChange={() => setConflictPolicy(policy)}
                    />
                    {t(label)}
                  </label>
                ))}
                {conflictPolicy === 'rename' && (
                  <Field label={t('新名称：{0}', conflict)}>
                    <input value={rename} onChange={(event) => setRename(event.target.value)} />
                  </Field>
                )}
              </fieldset>
            </>
          )}
          <div className="transfer-review-actions">
            <Button disabled={busy} onClick={() => void run(discard)}>
              {t('取消')}
            </Button>
            <Button
              primary
              disabled={
                busy ||
                !ready ||
                (!!conflict &&
                  (conflictPolicy === 'fail' || (conflictPolicy === 'rename' && !rename.trim())))
              }
              onClick={() =>
                void run(
                  () => submitTransfer(transfer, conflict ? conflictPolicy : 'fail'),
                  '正在准备传输…',
                )
              }
            >
              {t(conflict ? '确认传输计划' : '重试传输')}
            </Button>
          </div>
        </div>
      )}
      {activeTasks.map((task) => (
        <div className="transfer-status" role="status" key={task.id}>
          <div>
            <strong>{t(task.title)}</strong>
            <p>{t(task.message)}</p>
          </div>
          <Button
            disabled={busy || task.cancelRequested || !task.cancellable}
            onClick={() =>
              void run(async () => {
                await call('task_cancel', { taskId: task.id });
              })
            }
          >
            {t('取消传输')}
          </Button>
          {task.progress !== null && (
            <progress max={100} value={task.progress} aria-label={t('传输进度')} />
          )}
        </div>
      ))}
      {interrupted.map((task) => (
        <div className="phone-review transfer-review" key={task.id}>
          <p>{t(task.message)}</p>
          <div className="transfer-review-actions">
            {task.state === 'needs_review' && task.operation.args.conflicts === 'replace' && (
              <Button
                disabled={locked || !ready}
                onClick={() =>
                  void run(() => prepare('file_restore', { sourceTaskId: task.id }, target))
                }
              >
                {t('恢复被替换的旧目标')}
              </Button>
            )}
            <Button
              disabled={locked || !ready}
              onClick={() =>
                void run(() => prepare('file_cleanup', { sourceTaskId: task.id }, target))
              }
            >
              {t('清理原传输暂存')}
            </Button>
          </div>
        </div>
      ))}
    </Modal>
  );
}

function FilePane({
  title,
  direction,
  device,
  directory,
  disabled,
  transferDisabled,
  loading,
  onBrowse,
  onRefresh,
  onChoose,
  onTransfer,
}: {
  title: string;
  direction: Direction;
  device?: string;
  directory: FileDirectory | null;
  disabled: boolean;
  transferDisabled: boolean;
  loading: boolean;
  onBrowse: (path: string) => void;
  onRefresh: () => void;
  onChoose?: () => void;
  onTransfer: (path: string) => void;
}) {
  const parent = directory?.parent;
  const TransferIcon = direction === 'upload' ? ArrowRight : ArrowLeft;
  const transferLabel = direction === 'upload' ? '将 {0} 上传到 Android' : '将 {0} 下载到本地';
  return (
    <section className="transfer-pane" aria-label={t(title)} aria-description={device}>
      <header className="transfer-location">
        <strong title={device}>{t(title)}</strong>
        <span className="transfer-path" title={directory?.path}>
          {directory?.path ?? '—'}
        </span>
        {onChoose && (
          <button
            type="button"
            className="icon-button"
            aria-label={t('选择目录')}
            title={t('选择目录')}
            disabled={disabled}
            onClick={onChoose}
          >
            <FolderOpen size={16} />
          </button>
        )}
        <button
          type="button"
          className="icon-button"
          aria-label={t('上级目录')}
          title={t('上级目录')}
          disabled={disabled || !parent}
          onClick={() => parent && onBrowse(parent)}
        >
          <ArrowUp size={16} />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t('刷新')}
          title={t('刷新')}
          disabled={disabled}
          onClick={onRefresh}
        >
          <RefreshCw size={15} />
        </button>
      </header>
      <div className="transfer-entries" aria-busy={loading}>
        {directory?.entries.length ? (
          <ul>
            {directory.entries.map((entry) => {
              const Icon = entry.link ? Link2 : entry.directory ? Folder : File;
              const description = entry.link
                ? t('链接或不支持的名称')
                : entry.modified
                  ? new Date(entry.modified * 1000).toLocaleString()
                  : '';
              const actionLabel = t(transferLabel, entry.name);
              return (
                <li key={entry.path}>
                  {entry.directory && !entry.link ? (
                    <button
                      type="button"
                      className="transfer-entry-name"
                      title={entry.name}
                      disabled={disabled}
                      onClick={() => onBrowse(entry.path)}
                    >
                      <Icon size={15} />
                      <span>{entry.name}</span>
                    </button>
                  ) : (
                    <span
                      className="transfer-entry-name"
                      title={[entry.name, description].filter(Boolean).join('\n')}
                    >
                      <Icon size={15} />
                      <span>{entry.name}</span>
                    </span>
                  )}
                  {!entry.directory && !entry.link && (
                    <span className="transfer-entry-size">
                      {entry.bytes < 1024 ? `${entry.bytes} B` : size(entry.bytes)}
                    </span>
                  )}
                  <button
                    type="button"
                    className="icon-button transfer-entry-action"
                    aria-label={actionLabel}
                    title={actionLabel}
                    disabled={transferDisabled || entry.link}
                    onClick={() => onTransfer(entry.path)}
                  >
                    <TransferIcon size={15} aria-hidden="true" />
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          (directory || loading) && (
            <p className="hint">{t(loading ? '正在读取目录…' : '目录为空')}</p>
          )
        )}
      </div>
    </section>
  );
}
