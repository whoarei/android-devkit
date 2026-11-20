import { t as tr, getLocale } from './i18n';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { FileDown, FileText, Image, Trash2 } from 'lucide-react';
import { Modal } from './ui';
import { call, errorText, native, size } from './api';
const artifactSize = (bytes: number) =>
  bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 * 1024
      ? `${(bytes / 1024).toFixed(1)} KiB`
      : size(bytes);
import type { Artifact, CloseStatus, ExportPreview } from './bindings';
type Guard = <T>(fn: () => Promise<T>) => Promise<T | undefined>;
export type ExportRequest = Record<string, unknown>;
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
  requestedTaskId,
  onRequestHandled,
}: {
  guard: Guard;
  onExport: (p: ExportPreview) => void;
  onScreenshot: (src: string) => void;
  previewOpen: boolean;
  requestedTaskId?: string | null;
  onRequestHandled?: () => void;
}) {
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [taskScope, setTaskScope] = useState<string | null>(null);
  useEffect(() => {
    if (requestedTaskId) {
      setTaskScope(requestedTaskId);
      setOpen(true);
      setError('');
      onRequestHandled?.();
    }
  }, [requestedTaskId, onRequestHandled]);
  const previewTrigger = useRef<HTMLElement | null>(null);
  const [video, setVideo] = useState('');
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
  const allPending = query.data?.filter((a) => !a.acknowledged) ?? [];
  const pending = allPending.filter((a) => !taskScope || a.taskId === taskScope);
  const clearable = pending.filter((a) => !['keystore', 'signed_apk'].includes(a.kind));
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
      if (a.kind === 'video') {
        setVideo(await call<string>('video_preview', { artifactId: a.id }));
      } else if (a.kind === 'screenshot') {
        onScreenshot(await call('artifact_copy', { taskId: a.taskId, preview: true }));
      } else {
        onExport(await call('export_page', { artifactId: a.id, offset: 0 }));
      }
    });
  return (
    <>
      <button
        className="artifact-inbox icon-button"
        aria-label={tr('待保存文件：{0} 项', allPending.length)}
        title={tr('待保存文件')}
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={!native}
        onClick={() => {
          setTaskScope(null);
          setOpen(true);
          setError('');
        }}
      >
        <FileDown size={18} aria-hidden="true" />
        {allPending.length > 0 && (
          <span className="artifact-count" aria-hidden="true">
            {allPending.length > 99 ? '99+' : allPending.length}
          </span>
        )}
      </button>
      {open && (
        <Modal
          title={tr('待保存文件')}
          onClose={() => setOpen(false)}
          wide
          className="artifact-modal"
          hidden={previewOpen || !!video}
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
              disabled={busy || !clearable.length}
              onClick={() => discard(clearable.map((a) => a.id))}
            >
              <Trash2 size={13} />
              {tr('全部忽略并清理')}
            </button>
          </div>
          <p className="hint">
            {taskScope && <>{tr('仅显示当前任务的文件。')} </>}
            {tr('忽略会删除对应的本地缓存；已另存文件、原始采集段和任务记录保留。')}
            {pending.some((a) => a.kind === 'keystore') && (
              <> {tr('新密钥受保护；请在签名工具中核验或明确放弃。')}</>
            )}
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
                  <Btn
                    disabled={busy || ['keystore', 'signed_apk', 'apk', 'apk_set'].includes(a.kind)}
                    onClick={() => preview(a)}
                  >
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
                    disabled={busy || a.kind === 'keystore'}
                    onClick={() =>
                      a.kind === 'signed_apk'
                        ? run(() =>
                            call('signing_result_discard', {
                              artifactId: a.id,
                              expectedSha256: a.sha256,
                            }),
                          )
                        : discard([a.id])
                    }
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
      {video && (
        <Modal title={tr('录屏预览')} onClose={() => setVideo('')} wide trapFocus>
          <video className="phone-video" controls src={video} />
          <p className="hint">{tr('确认画面后可返回待保存文件另存 MP4。')}</p>
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
  const protectedPending =
    s?.unsaved.some((a) => ['keystore', 'signed_apk'].includes(a.kind)) ?? false;
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
              disabled={protectedPending}
              onChange={(e) => setDiscard(e.target.checked)}
            />
            {tr('忽略上述未保存文件并清理对应本地缓存')}
          </label>
          {protectedPending && (
            <p className="hint">{tr('请先保存签名结果；新密钥只能在签名工具中逐项明确放弃。')}</p>
          )}
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
            <Btn disabled={s.draining || protectedPending} onClick={() => resolve('wait')}>
              {tr('完成任务后退出')}
            </Btn>
            <Btn disabled={s.draining || protectedPending} onClick={() => resolve('cancel')}>
              {tr('取消任务并退出')}
            </Btn>
          </>
        ) : (
          <Btn primary disabled={protectedPending} onClick={() => resolve('exit')}>
            {tr('退出')}
          </Btn>
        )}
      </div>
    </Dialog>
  );
}
