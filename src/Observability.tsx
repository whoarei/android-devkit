import { t as tr, getLocale } from './i18n';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { FileDown, FileText, Image, Trash2 } from 'lucide-react';
import { Modal } from './ui';
import { call, deviceKey, errorText, native, size } from './api';
const artifactSize = (bytes: number) =>
  bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 * 1024
      ? `${(bytes / 1024).toFixed(1)} KiB`
      : size(bytes);
import type {
  Artifact,
  CloseStatus,
  Device,
  ExportPreview,
  LogFilter,
  LogSnapshot,
} from './bindings';
type Guard = <T>(fn: () => Promise<T>) => Promise<T | undefined>;
export type ExportRequest = Record<string, unknown>;
const emptyFilter: LogFilter = {
  level: 'V',
  tag: '',
  keyword: '',
  package: '',
  pids: [],
  after: 0,
  before: 0,
};
function Btn({
  children,
  onClick,
  disabled = false,
  primary = false,
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
}) {
  return (
    <button className={`button ${primary ? 'primary' : ''}`} onClick={onClick} disabled={disabled}>
      {children}
    </button>
  );
}
function Dialog({
  title,
  children,
  close,
}: {
  title: string;
  children: ReactNode;
  close: () => void;
}) {
  useEffect(() => {
    const fn = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    };
    document.addEventListener('keydown', fn);
    return () => document.removeEventListener('keydown', fn);
  }, [close]);
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section className="modal wide" role="dialog" aria-modal="true" aria-label={tr(title)}>
        <div className="modal-heading">
          <h2>{tr(title)}</h2>
          <button className="icon-button" aria-label={tr('关闭面板')} onClick={close}>
            ×
          </button>
        </div>
        {children}
      </section>
    </div>
  );
}
export function ExportDialog({
  value,
  setValue,
  guard,
  close,
}: {
  value: ExportPreview;
  setValue: (v: ExportPreview) => void;
  guard: Guard;
  close: () => void;
}) {
  const [saved, setSaved] = useState('');
  return (
    <Dialog title={tr('导出内容预览')} close={close}>
      <p>
        {tr(value.artifact.title)} · {value.rows.toLocaleString(getLocale())}
        {tr(' 行 \u00B7')} {artifactSize(value.artifact.bytes)} ·{' '}
        {value.redacted ? tr('已默认脱敏') : tr('原始内容，未脱敏')}
        {value.truncated ? tr(' \u00B7 导出已截断') : ''}
      </p>
      <p className="hint">{tr(value.warning)}</p>
      <pre className="export-preview">{value.text || tr('所选范围没有日志。')}</pre>
      <div className="modal-actions">
        <span>
          {tr('当前第 ')}
          {value.rows ? value.offset + 1 : 0}—{Math.min(value.offset + 300, value.rows)}
          {tr(' 行')}
        </span>
        <Btn
          disabled={value.offset === 0}
          onClick={() =>
            void guard(async () =>
              setValue(
                await call('export_page', {
                  artifactId: value.artifact.id,
                  offset: Math.max(0, value.offset - 300),
                }),
              ),
            )
          }
        >
          {tr('上一页')}
        </Btn>
        <Btn
          disabled={value.nextOffset === null}
          onClick={() =>
            void guard(async () =>
              setValue(
                await call('export_page', {
                  artifactId: value.artifact.id,
                  offset: value.nextOffset,
                }),
              ),
            )
          }
        >
          {tr('下一页')}
        </Btn>
        <Btn
          primary
          onClick={() =>
            void guard(async () => {
              const p = await call<string | null>('export_save', { artifactId: value.artifact.id });
              if (p) setSaved(p);
            })
          }
        >
          {tr('保存到本地…')}
        </Btn>
      </div>
      {saved && (
        <p role="status">
          {tr('已保存：')}
          {saved}
        </p>
      )}
    </Dialog>
  );
}
export function LogPanel({
  target,
  guard,
  prepare,
  onExport,
}: {
  target: Device;
  guard: Guard;
  prepare: (kind: string, args: Record<string, string>) => Promise<void>;
  onExport: (request: ExportRequest) => void;
}) {
  const [filter, setFilter] = useState<LogFilter>({ ...emptyFilter });
  const [pid, setPid] = useState('');
  const [snapshot, setSnapshot] = useState<LogSnapshot | null>(null);
  const [paused, setPaused] = useState(false);
  const [view, setView] = useState<LogSnapshot | null>(null);
  const [missed, setMissed] = useState(0);
  const [raw, setRaw] = useState(false);
  const [record, setRecord] = useState(false);
  const [quota, setQuota] = useState(64);
  const [minutes, setMinutes] = useState(60);
  const pausedRef = useRef(false);
  const currentRef = useRef(filter);
  const latestRef = useRef<LogSnapshot | null>(null);
  const effective = { ...filter, pids: pid.split(/[ ,]+/).filter(Boolean).map(Number) };
  currentRef.current = effective;
  pausedRef.current = paused;
  const read = async (action: string) => {
    const next = await call<LogSnapshot>('log_read', {
      target,
      action,
      filter: currentRef.current,
    });
    setSnapshot(next);
    latestRef.current = next;
    if (!pausedRef.current) setView(next);
    return next;
  };
  useEffect(() => {
    setView(null);
    setSnapshot(null);
    setFilter({ ...emptyFilter });
    const timer = setInterval(() => void guard(() => read('read')), 1000);
    return () => {
      clearInterval(timer);
      void call('log_read', { target, action: 'stop', filter: emptyFilter }).catch(() => {});
    };
  }, [deviceKey(target)]);
  const set = (key: keyof LogFilter, value: string) => setFilter((v) => ({ ...v, [key]: value }));
  const exportFilter = {
    ...effective,
    after: filter.after,
    before: (paused ? view?.latest : snapshot?.latest) ?? 0,
  };
  return (
    <section className="log-panel" id="logs">
      <div className="log-toolbar">
        <h3>Logcat · {target.model}</h3>
        <div>
          <Btn onClick={() => void guard(() => read(snapshot?.active ? 'stop' : 'start'))}>
            {snapshot?.active ? tr('结束查看') : tr('开始查看')}
          </Btn>
          <Btn
            onClick={() => {
              if (paused) {
                const next = latestRef.current;
                if (next) {
                  setMissed(Math.max(0, next.oldest - (view?.latest ?? 0) - 1));
                  setView(next);
                }
              }
              setPaused((v) => !v);
            }}
          >
            {paused ? tr('恢复视图') : tr('暂停视图')}
          </Btn>
          <Btn
            onClick={() => {
              setFilter((v) => ({ ...v, after: snapshot?.latest ?? 0 }));
              setView((v) => (v ? { ...v, rows: [] } : null));
            }}
          >
            {tr('清空视图')}
          </Btn>
        </div>
      </div>
      <div className="log-filters">
        <label>
          {tr('最低级别')}
          <select
            aria-label={tr('日志最低级别')}
            value={filter.level}
            onChange={(e) => set('level', e.target.value)}
          >
            {['V', 'D', 'I', 'W', 'E', 'F'].map((v) => (
              <option key={v}>{v}</option>
            ))}
          </select>
        </label>
        <label>
          Tag
          <input
            aria-label={tr('日志 Tag')}
            value={filter.tag}
            onChange={(e) => set('tag', e.target.value)}
          />
        </label>
        <label>
          PID
          <input
            aria-label={tr('日志 PID')}
            placeholder={tr('多个用逗号分隔')}
            value={pid}
            onChange={(e) => setPid(e.target.value)}
          />
        </label>
        <label>
          {tr('应用包名')}
          <input
            aria-label={tr('日志应用包名')}
            placeholder={tr('跟踪主进程与子进程')}
            value={filter.package}
            onChange={(e) => set('package', e.target.value)}
          />
        </label>
        <label>
          {tr('关键词')}
          <input
            aria-label={tr('过滤日志关键词')}
            value={filter.keyword}
            onChange={(e) => set('keyword', e.target.value)}
          />
        </label>
      </div>
      <p className="hint">
        {(snapshot?.status ? tr(snapshot.status) : undefined) ?? tr('开始查看后显示设备日志')} ·{' '}
        {(snapshot?.processStatus ? tr(snapshot.processStatus) : undefined) ?? tr('全部进程')}
        {snapshot?.pids.length ? ` · PID ${snapshot.pids.join(', ')}` : ''}
        {paused ? tr(' \u00B7 视图暂停，后台缓冲继续更新') : ''}
        {missed ? tr(' \u00B7 暂停期间至少遗漏 {0} 行', missed) : ''}
      </p>
      <pre className="log-lines">
        {view?.rows.map((r) => r.raw).join('\n') || tr('当前过滤范围没有日志。')}
      </pre>
      <small>
        {tr('缓冲 ')}
        {snapshot?.buffered.toLocaleString(getLocale()) ?? 0}
        {tr(' / 10,000 行 \u00B7 淘汰 ')}
        {snapshot?.dropped ?? 0} {tr('行 \u00B7 仅渲染末 500 行；清空不会清除设备日志')}
      </small>
      <div className="task-actions">
        <label>
          <input type="checkbox" checked={raw} onChange={(e) => setRaw(e.target.checked)} />{' '}
          {tr('导出原始内容（含敏感信息）')}
        </label>
        <Btn
          onClick={() => onExport({ kind: 'view', target, filter: exportFilter, redacted: !raw })}
        >
          {tr('预览并导出当前视图')}
        </Btn>
        <Btn onClick={() => setRecord((v) => !v)}>{tr('文件采集…')}</Btn>
      </div>
      {record && (
        <div className="record-options">
          <label>
            {tr('总配额 MiB')}
            <input
              aria-label={tr('采集总配额')}
              type="number"
              min={8}
              max={4096}
              value={quota}
              onChange={(e) => setQuota(Number(e.target.value))}
            />
          </label>
          <label>
            {tr('时限 分钟')}
            <input
              aria-label={tr('采集时限')}
              type="number"
              min={1}
              max={1440}
              value={minutes}
              onChange={(e) => setMinutes(Number(e.target.value))}
            />
          </label>
          <Btn
            primary
            onClick={() =>
              void prepare('log_record', {
                filter: JSON.stringify({ ...effective, after: 0 }),
                quotaMiB: String(quota),
                seconds: String(minutes * 60),
              })
            }
          >
            {tr('查看采集计划')}
          </Btn>
          <p className="hint">
            {tr('按当前过滤条件采集；每段最多 100 MiB。切页不停止采集，在任务中心停止并导出。')}
          </p>
        </div>
      )}
    </section>
  );
}
export function RecordingExport({
  taskId,
  onExport,
  guard,
}: {
  taskId: string;
  onExport: (r: ExportRequest) => void;
  guard: Guard;
}) {
  const [segments, setSegments] = useState<
    {
      name: string;
      bytes: number;
    }[]
  >([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [raw, setRaw] = useState(false);
  const [loaded, setLoaded] = useState(false);
  return (
    <div>
      <Btn
        onClick={() =>
          void guard(async () => {
            const m = await call<{
              files: {
                name: string;
                bytes: number;
              }[];
            }>('recording_segments', { taskId });
            setSegments(m.files);
            setSelected(m.files.map((_, i) => i));
            setLoaded(true);
          })
        }
      >
        {tr('选择采集段导出')}
      </Btn>
      {loaded && (
        <div className="segment-picker">
          {segments.map((s, i) => (
            <label key={s.name}>
              <input
                type="checkbox"
                checked={selected.includes(i)}
                onChange={(e) =>
                  setSelected((v) => (e.target.checked ? [...v, i] : v.filter((x) => x !== i)))
                }
              />
              {s.name} · {artifactSize(s.bytes)}
            </label>
          ))}
          <label>
            <input type="checkbox" checked={raw} onChange={(e) => setRaw(e.target.checked)} />
            {tr('导出原始内容')}
          </label>
          <Btn
            disabled={!selected.length}
            onClick={() =>
              onExport({ kind: 'recording', taskId, segments: selected, redacted: !raw })
            }
          >
            {tr('预览所选采集段')}
          </Btn>
        </div>
      )}
    </div>
  );
}
export function ArtifactsPanel({
  guard,
  onExport,
  onScreenshot,
  previewOpen,
}: {
  guard: Guard;
  onExport: (p: ExportPreview) => void;
  onScreenshot: (src: string) => void;
  previewOpen: boolean;
}) {
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const previewTrigger = useRef<HTMLElement | null>(null);
  const wasPreviewing = useRef(false);
  const query = useQuery({
    queryKey: ['artifacts'],
    queryFn: () => call<Artifact[]>('artifacts_list'),
    enabled: native,
    refetchInterval: 3000,
  });
  useEffect(() => {
    const returning = wasPreviewing.current && !previewOpen;
    wasPreviewing.current = previewOpen;
    if (!open || previewOpen) return;
    let cancelled = false;
    void query.refetch().then(() => {
      if (returning && !cancelled)
        requestAnimationFrame(() => {
          if (cancelled) return;
          const target = previewTrigger.current?.isConnected
            ? previewTrigger.current
            : document.querySelector<HTMLElement>('.artifact-modal .icon-button');
          target?.focus({ preventScroll: true });
        });
    });
    return () => {
      cancelled = true;
    };
  }, [open, previewOpen, query.refetch]);
  const pending = query.data?.filter((a) => !a.acknowledged) ?? [];
  const run = (work: () => Promise<void>) =>
    void guard(async () => {
      if (busy) return;
      setBusy(true);
      setError('');
      try {
        await work();
      } catch (e) {
        setError(errorText(e));
        throw e;
      } finally {
        await Promise.all([
          client.invalidateQueries({ queryKey: ['artifacts'] }),
          client.invalidateQueries({ queryKey: ['task-list'] }),
          client.invalidateQueries({ queryKey: ['workspace'] }),
          client.invalidateQueries({ queryKey: ['close-status'] }),
        ]);
        setBusy(false);
      }
    });
  const discard = (ids: string[]) => run(() => call('artifacts_discard', { artifactIds: ids }));
  const preview = (a: Artifact) =>
    run(async () => {
      previewTrigger.current =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
      if (a.kind === 'screenshot') {
        onScreenshot(await call('artifact_copy', { taskId: a.taskId, preview: true }));
      } else {
        onExport(await call('export_page', { artifactId: a.id, offset: 0 }));
      }
    });
  return (
    <>
      <button
        className="artifact-inbox icon-button"
        aria-label={tr('待保存文件：{0} 项', pending.length)}
        title={tr('待保存文件')}
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={!native}
        onClick={() => {
          setOpen(true);
          setError('');
        }}
      >
        <FileDown size={18} aria-hidden="true" />
        {pending.length > 0 && (
          <span className="artifact-count" aria-hidden="true">
            {pending.length > 99 ? '99+' : pending.length}
          </span>
        )}
      </button>
      {open && (
        <Modal
          title={tr('待保存文件')}
          onClose={() => setOpen(false)}
          wide
          className="artifact-modal"
          hidden={previewOpen}
        >
          <div className="artifact-summary">
            <span>
              {tr(
                '{0} 项 · {1}',
                pending.length,
                artifactSize(pending.reduce((sum, a) => sum + a.bytes, 0)),
              )}
            </span>
            <button
              className="text-button danger"
              disabled={busy || !pending.length}
              onClick={() => discard(pending.map((a) => a.id))}
            >
              <Trash2 size={13} />
              {tr('全部忽略并清理')}
            </button>
          </div>
          <p className="hint">
            {tr('忽略会删除对应的本地缓存；已另存文件、原始采集段和任务记录保留。')}
          </p>
          {(error || query.error) && (
            <p className="error-banner" role="alert">
              {tr(error || errorText(query.error))}
            </p>
          )}
          <div className="artifact-items" aria-busy={busy || query.isFetching}>
            {pending.map((a) => (
              <article className="artifact-item" key={a.id} data-artifact-id={a.id}>
                <div className="artifact-symbol">
                  {a.kind === 'screenshot' ? <Image size={18} /> : <FileText size={18} />}
                </div>
                <div className="artifact-info">
                  <strong>{tr(a.title)}</strong>
                  <small>
                    <time dateTime={new Date(a.createdAt * 1000).toISOString()}>
                      {new Date(a.createdAt * 1000).toLocaleString(getLocale(), { hour12: false })}
                    </time>{' '}
                    · {artifactSize(a.bytes)}
                  </small>
                </div>
                <div className="artifact-actions">
                  <Btn disabled={busy} onClick={() => preview(a)}>
                    {tr('预览')}
                  </Btn>
                  <Btn
                    disabled={busy}
                    onClick={() =>
                      run(async () => {
                        await call('export_save', { artifactId: a.id });
                      })
                    }
                  >
                    {tr('保存…')}
                  </Btn>
                  <button
                    className="text-button danger"
                    disabled={busy}
                    onClick={() => discard([a.id])}
                  >
                    {tr('忽略并清理')}
                  </button>
                </div>
              </article>
            ))}
            {!pending.length && !query.isPending && !query.error && (
              <div className="artifact-empty">
                <FileDown size={24} />
                <p>{tr('没有待保存的文件')}</p>
              </div>
            )}
          </div>
        </Modal>
      )}
    </>
  );
}
export function CloseDialog({ guard, close }: { guard: Guard; close: () => void }) {
  const [discard, setDiscard] = useState(false);
  const [stop, setStop] = useState(false);
  const query = useQuery({
    queryKey: ['close-status'],
    queryFn: () => call<CloseStatus>('close_status'),
    refetchInterval: 1000,
  });
  const s = query.data;
  const resolve = (mode: string) =>
    void guard(async () => {
      await call('close_resolve', {
        mode,
        discard,
        stopRecordings: stop,
        discardIds: s?.unsaved.map((a) => a.id) ?? [],
      });
      if (mode === 'return' || mode === 'hide') close();
      await query.refetch();
    });
  return (
    <Dialog title={tr('退出工作台')} close={() => resolve('return')}>
      <p>{tr('退出后，模拟器和共享 ADB 继续运行。')}</p>
      {s?.tasks.map((t) => (
        <p key={t.id}>
          {tr(t.title)} · {t.phase} · {tr(t.message)}
        </p>
      ))}
      {s?.tasks.some((t) => t.operation.kind === 'log_record') && (
        <label className="check-row">
          <input type="checkbox" checked={stop} onChange={(e) => setStop(e.target.checked)} />
          {tr('停止文件采集并封口后退出')}
        </label>
      )}
      {s?.unsaved.length ? (
        <>
          <h3>
            {tr('尚未另存 ')}
            {s.unsaved.length}
            {tr(' 个产物')}
          </h3>
          {s.unsaved.map((a) => (
            <div className="task-actions" key={a.id}>
              <span>
                {tr(a.title)} · {artifactSize(a.bytes)}
              </span>
              <Btn
                onClick={() =>
                  void guard(async () => {
                    await call('export_save', { artifactId: a.id });
                    await query.refetch();
                  })
                }
              >
                {tr('保存…')}
              </Btn>
            </div>
          ))}
          <label className="check-row">
            <input
              type="checkbox"
              checked={discard}
              onChange={(e) => setDiscard(e.target.checked)}
            />
            {tr('忽略上述未保存文件并清理对应本地缓存')}
          </label>
        </>
      ) : null}
      {s?.draining && (
        <p role="status">{tr('正在等待任务收尾；若产生新的未保存产物，将再次提示。')}</p>
      )}
      <div className="modal-actions">
        <Btn onClick={() => resolve('return')}>{tr('返回')}</Btn>
        {s?.trayEnabled && <Btn onClick={() => resolve('hide')}>{tr('隐藏到托盘')}</Btn>}
        {s?.tasks.length ? (
          <>
            <Btn disabled={s.draining} onClick={() => resolve('wait')}>
              {tr('完成任务后退出')}
            </Btn>
            <Btn disabled={s.draining} onClick={() => resolve('cancel')}>
              {tr('取消任务并退出')}
            </Btn>
          </>
        ) : (
          <Btn primary onClick={() => resolve('exit')}>
            {tr('退出')}
          </Btn>
        )}
      </div>
    </Dialog>
  );
}
