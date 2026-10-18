import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { call, deviceKey, native, size } from './api';
const artifactSize = (bytes: number) => (bytes < 1024 ? `${bytes} B` : size(bytes));
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
    <div className="modal-backdrop">
      <section className="modal wide" role="dialog" aria-modal="true" aria-label={title}>
        <div className="modal-heading">
          <h2>{title}</h2>
          <button className="icon-button" aria-label="关闭面板" onClick={close}>
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
    <Dialog title="导出内容预览" close={close}>
      <p>
        {value.artifact.title} · {value.rows.toLocaleString()} 行 ·{' '}
        {artifactSize(value.artifact.bytes)} · {value.redacted ? '已默认脱敏' : '原始内容，未脱敏'}
        {value.truncated ? ' · 导出已截断' : ''}
      </p>
      <p className="hint">{value.warning}</p>
      <pre className="export-preview">{value.text || '所选范围没有日志。'}</pre>
      <div className="modal-actions">
        <span>
          当前第 {value.rows ? value.offset + 1 : 0}—{Math.min(value.offset + 300, value.rows)} 行
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
          上一页
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
          下一页
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
          保存到本地…
        </Btn>
      </div>
      {saved && <p role="status">已保存：{saved}</p>}
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
            {snapshot?.active ? '结束查看' : '开始查看'}
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
            {paused ? '恢复视图' : '暂停视图'}
          </Btn>
          <Btn
            onClick={() => {
              setFilter((v) => ({ ...v, after: snapshot?.latest ?? 0 }));
              setView((v) => (v ? { ...v, rows: [] } : null));
            }}
          >
            清空视图
          </Btn>
        </div>
      </div>
      <div className="log-filters">
        <label>
          最低级别
          <select
            aria-label="日志最低级别"
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
            aria-label="日志 Tag"
            value={filter.tag}
            onChange={(e) => set('tag', e.target.value)}
          />
        </label>
        <label>
          PID
          <input
            aria-label="日志 PID"
            placeholder="多个用逗号分隔"
            value={pid}
            onChange={(e) => setPid(e.target.value)}
          />
        </label>
        <label>
          应用包名
          <input
            aria-label="日志应用包名"
            placeholder="跟踪主进程与子进程"
            value={filter.package}
            onChange={(e) => set('package', e.target.value)}
          />
        </label>
        <label>
          关键词
          <input
            aria-label="过滤日志关键词"
            value={filter.keyword}
            onChange={(e) => set('keyword', e.target.value)}
          />
        </label>
      </div>
      <p className="hint">
        {snapshot?.status ?? '开始查看后显示设备日志'} · {snapshot?.processStatus ?? '全部进程'}
        {snapshot?.pids.length ? ` · PID ${snapshot.pids.join(', ')}` : ''}
        {paused ? ' · 视图暂停，后台缓冲继续更新' : ''}
        {missed ? ` · 暂停期间至少遗漏 ${missed} 行` : ''}
      </p>
      <pre className="log-lines">
        {view?.rows.map((r) => r.raw).join('\n') || '当前过滤范围没有日志。'}
      </pre>
      <small>
        缓冲 {snapshot?.buffered.toLocaleString() ?? 0} / 10,000 行 · 淘汰 {snapshot?.dropped ?? 0}{' '}
        行 · 仅渲染末 500 行；清空不会清除设备日志
      </small>
      <div className="task-actions">
        <label>
          <input type="checkbox" checked={raw} onChange={(e) => setRaw(e.target.checked)} />{' '}
          导出原始内容（含敏感信息）
        </label>
        <Btn
          onClick={() => onExport({ kind: 'view', target, filter: exportFilter, redacted: !raw })}
        >
          预览并导出当前视图
        </Btn>
        <Btn onClick={() => setRecord((v) => !v)}>文件采集…</Btn>
      </div>
      {record && (
        <div className="record-options">
          <label>
            总配额 MiB
            <input
              aria-label="采集总配额"
              type="number"
              min={8}
              max={4096}
              value={quota}
              onChange={(e) => setQuota(Number(e.target.value))}
            />
          </label>
          <label>
            时限 分钟
            <input
              aria-label="采集时限"
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
            查看采集计划
          </Btn>
          <p className="hint">
            按当前过滤条件采集；每段最多 100 MiB。切页不停止采集，在任务中心停止并导出。
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
  const [segments, setSegments] = useState<{ name: string; bytes: number }[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [raw, setRaw] = useState(false);
  const [loaded, setLoaded] = useState(false);
  return (
    <div>
      <Btn
        onClick={() =>
          void guard(async () => {
            const m = await call<{ files: { name: string; bytes: number }[] }>(
              'recording_segments',
              { taskId },
            );
            setSegments(m.files);
            setSelected(m.files.map((_, i) => i));
            setLoaded(true);
          })
        }
      >
        选择采集段导出
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
            导出原始内容
          </label>
          <Btn
            disabled={!selected.length}
            onClick={() =>
              onExport({ kind: 'recording', taskId, segments: selected, redacted: !raw })
            }
          >
            预览所选采集段
          </Btn>
        </div>
      )}
    </div>
  );
}
export function ArtifactsPanel({
  guard,
  onExport,
}: {
  guard: Guard;
  onExport: (p: ExportPreview) => void;
}) {
  const query = useQuery({
    queryKey: ['artifacts'],
    queryFn: () => call<Artifact[]>('artifacts_list'),
    enabled: native,
    refetchInterval: 3000,
  });
  const pending = query.data?.filter((a) => !a.acknowledged) ?? [];
  return pending.length ? (
    <section className="artifact-list">
      <h3>未另存产物 · {pending.length}</h3>
      <p className="hint">退出前可保存，或在退出面板明确放弃未保存状态。</p>
      {pending.map((a) => (
        <div key={a.id}>
          <span>
            {a.title} · {artifactSize(a.bytes)}
          </span>
          {a.kind === 'export' && (
            <Btn
              onClick={() =>
                void guard(async () =>
                  onExport(await call('export_page', { artifactId: a.id, offset: 0 })),
                )
              }
            >
              预览
            </Btn>
          )}
          <Btn
            onClick={() =>
              void guard(async () => {
                await call('export_save', { artifactId: a.id });
                await query.refetch();
              })
            }
          >
            保存…
          </Btn>
        </div>
      ))}
    </section>
  ) : null;
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
    <Dialog title="退出工作台" close={() => resolve('return')}>
      <p>退出后，模拟器和共享 ADB 继续运行。</p>
      {s?.tasks.map((t) => (
        <p key={t.id}>
          {t.title} · {t.phase} · {t.message}
        </p>
      ))}
      {s?.tasks.some((t) => t.operation.kind === 'log_record') && (
        <label className="check-row">
          <input type="checkbox" checked={stop} onChange={(e) => setStop(e.target.checked)} />
          停止文件采集并封口后退出
        </label>
      )}
      {s?.unsaved.length ? (
        <>
          <h3>尚未另存 {s.unsaved.length} 个产物</h3>
          {s.unsaved.map((a) => (
            <div className="task-actions" key={a.id}>
              <span>
                {a.title} · {artifactSize(a.bytes)}
              </span>
              <Btn
                onClick={() =>
                  void guard(async () => {
                    await call('export_save', { artifactId: a.id });
                    await query.refetch();
                  })
                }
              >
                保存…
              </Btn>
            </div>
          ))}
          <label className="check-row">
            <input
              type="checkbox"
              checked={discard}
              onChange={(e) => setDiscard(e.target.checked)}
            />
            我明确放弃上述产物的未保存状态，仍保留本地缓存
          </label>
        </>
      ) : null}
      {s?.draining && <p role="status">正在等待任务收尾；若产生新的未保存产物，将再次提示。</p>}
      <div className="modal-actions">
        <Btn onClick={() => resolve('return')}>返回</Btn>
        {s?.trayEnabled && <Btn onClick={() => resolve('hide')}>隐藏到托盘</Btn>}
        {s?.tasks.length ? (
          <>
            <Btn disabled={s.draining} onClick={() => resolve('wait')}>
              完成任务后退出
            </Btn>
            <Btn disabled={s.draining} onClick={() => resolve('cancel')}>
              取消任务并退出
            </Btn>
          </>
        ) : (
          <Btn primary onClick={() => resolve('exit')}>
            退出
          </Btn>
        )}
      </div>
    </Dialog>
  );
}
