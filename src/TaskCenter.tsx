import { t, getLocale } from './i18n';
import { usePreference } from './preferences';
import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Activity,
  ChevronDown,
  ChevronRight,
  Copy,
  FileDown,
  FileText,
  Image,
  Play,
  RefreshCw,
  Search,
  Trash2,
  X,
} from 'lucide-react';
import { call, errorText, native, taskActive } from './api';
import { ArtifactsPanel, RecordingExport, type ExportRequest } from './Observability';
import type {
  ExportPreview,
  Plan,
  Target,
  Task,
  TaskPage,
  TaskDeleteRequest,
  TaskDeleteResult,
} from './bindings';
type Guard = <T>(work: () => Promise<T>) => Promise<T | undefined>;
const filters = [
  ['all', '全部'],
  ['active', '进行中'],
  ['succeeded', '已完成'],
  ['failed', '失败'],
  ['waiting_confirmation', '待确认'],
  ['needs_review', '待检查'],
  ['cancelled', '已取消'],
] as const;
const taskState: Record<string, string> = {
  queued: '排队',
  running: '运行中',
  waiting_confirmation: '待确认',
  succeeded: '已完成',
  failed: '失败',
  needs_review: '待检查',
  cancelled: '已取消',
};
const targetName = (task: Task) =>
  task.operation.target?.serial ||
  task.operation.args.address ||
  task.operation.args.avdId?.split(/[\\/]/).at(-1) ||
  t('本机环境');
export function TaskCenter({
  guard,
  prepare,
  showPlan,
  showPreview,
  onExport,
  onExportPreview,
  previewOpen,
  refresh,
}: {
  guard: Guard;
  prepare: (kind: string, args?: Record<string, string>, target?: Target | null) => Promise<void>;
  showPlan: (plan: Plan) => void;
  showPreview: (src: string) => void;
  onExport: (request: ExportRequest) => void;
  onExportPreview: (preview: ExportPreview) => void;
  previewOpen: boolean;
  refresh: () => Promise<unknown>;
}) {
  const [search, setSearch] = useState('');
  const [keyword, setKeyword] = useState('');
  const [state, setState] = usePreference('taskState');
  const [offset, setOffset] = useState(0);
  const client = useQueryClient();
  const [managing, setManaging] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [deleting, setDeleting] = useState(false);
  const deletionBusy = useRef(false);
  const [result, setResult] = useState<TaskDeleteResult | null>(null);
  const [deleteError, setDeleteError] = useState('');
  const [selectionNotice, setSelectionNotice] = useState('');
  const [fileTask, setFileTask] = useState<string | null>(null);
  useEffect(() => {
    const timer = setTimeout(() => {
      setKeyword(search.trim());
      setOffset(0);
    }, 250);
    return () => clearTimeout(timer);
  }, [search]);
  const query = useQuery({
    queryKey: ['task-list', keyword, state, offset],
    queryFn: () => call<TaskPage>('task_list', { query: { keyword, state, offset } }),
    enabled: native,
    refetchInterval: deleting ? false : 3000,
    placeholderData: keepPreviousData,
  });
  const page = query.data;
  useEffect(() => {
    if (page && !query.isPlaceholderData && page.offset !== offset) setOffset(page.offset);
  }, [page?.offset, query.isPlaceholderData, offset]);
  const counts = page?.counts ?? {};
  const blocked = (task: Task) =>
    t(page?.deletion?.blocked[task.id] || '') ||
    (taskActive(task.state) ? t('任务仍在进行，请先取消并等待结束') : '') ||
    (task.operation.kind === 'log_record' && task.artifact
      ? t('原始采集仍在，请先处理采集文件')
      : '');
  const selectable = page?.tasks.filter((task) => !blocked(task)) ?? [];
  const scopeReady = !query.isPending && !query.isPlaceholderData && keyword === search.trim();
  const allSelected =
    selectable.length > 0 && selectable.every((task) => selected.includes(task.id));
  const scope = t(
    '关键词：{0} · 状态：{1} · 所有分页',
    keyword || t('无'),
    t(filters.find(([value]) => value === state)?.[1] ?? '全部'),
  );
  useEffect(() => {
    if (selected.length) setSelectionNotice(t('筛选或页码已变化，已清空选择'));
    setSelected([]);
  }, [search, state, offset]);
  const deleteRecords = async (request: TaskDeleteRequest) => {
    if (deletionBusy.current) return;
    deletionBusy.current = true;
    setDeleting(true);
    setDeleteError('');
    setResult(null);
    const origin = document.activeElement as HTMLElement | null;
    const originRow = origin?.closest<HTMLElement>('[data-task-id]');
    const neighbor = originRow?.nextElementSibling as HTMLElement | null;
    const previous = originRow?.previousElementSibling as HTMLElement | null;
    try {
      const response = await call<TaskDeleteResult>('task_delete', { request });
      setResult(response);
      setSelected((current) =>
        current.filter(
          (id) =>
            !request.taskIds.includes(id) || response.issues.some((issue) => issue.taskId === id),
        ),
      );
    } catch (error) {
      setDeleteError(t('删除请求未完成，请刷新列表核对结果') + ' · ' + t(errorText(error)));
    } finally {
      await Promise.allSettled([
        refresh(),
        client.invalidateQueries({ queryKey: ['artifacts'] }),
        client.invalidateQueries({ queryKey: ['close-status'] }),
      ]);
      deletionBusy.current = false;
      setDeleting(false);
      requestAnimationFrame(() => {
        const next =
          origin?.isConnected && !origin.hasAttribute('disabled')
            ? origin
            : [originRow, neighbor, previous]
                .filter((row) => row?.isConnected)
                .map((row) =>
                  row?.querySelector<HTMLElement>('.task-row-actions button:not(:disabled)'),
                )
                .find(Boolean);
        (next || document.querySelector<HTMLElement>('.task-manage'))?.focus({
          preventScroll: true,
        });
      });
    }
  };
  const count = (filter: string) =>
    filter === 'all'
      ? Object.values(counts).reduce<number>((sum, value) => sum + (value ?? 0), 0)
      : filter === 'active'
        ? (counts.running ?? 0) + (counts.queued ?? 0) + (counts.waiting_confirmation ?? 0)
        : (counts[filter] ?? 0);
  const clear = () => {
    setSearch('');
    setKeyword('');
    setState('all');
    setOffset(0);
  };
  const act = async (command: string, args: Record<string, unknown>) => {
    await guard(() => call(command, args));
    await refresh();
  };
  return (
    <div className="task-center">
      <div className="task-toolbar">
        <label className="task-search">
          <Search size={15} aria-hidden="true" />
          <input
            aria-label={t('搜索任务')}
            placeholder={t('搜索名称、设备、结果或任务 ID')}
            value={search}
            disabled={deleting}
            onChange={(event) => setSearch(event.target.value)}
          />
          {search && (
            <button
              aria-label={t('清空任务搜索')}
              disabled={deleting}
              onClick={() => {
                setSearch('');
                setKeyword('');
                setOffset(0);
              }}
            >
              <X size={14} />
            </button>
          )}
        </label>
        <span className="task-result-count" role="status">
          {query.isFetching ? t('正在更新…') : t('共 {0} 项任务', page?.total ?? 0)}
        </span>
        <button
          className="text-button"
          onClick={() => void query.refetch()}
          disabled={query.isFetching || !native || deleting}
        >
          <RefreshCw size={13} className={query.isFetching ? 'spin' : ''} />
          {t('刷新')}
        </button>
        <button
          className="button task-manage"
          aria-pressed={managing}
          disabled={!native || deleting}
          onClick={() => {
            setManaging(!managing);
            setSelected([]);
            setSelectionNotice('');
          }}
        >
          {t(managing ? '完成管理' : '管理任务')}
        </button>
        <button
          className="button"
          disabled={!native || deleting || managing || !scopeReady || !page?.deletion?.eligible}
          onClick={() => void deleteRecords({ taskIds: [], query: { keyword, state, offset: 0 } })}
        >
          {deleting ? t('正在删除…') : t('清理历史（{0}）', page?.deletion?.eligible ?? 0)}
        </button>
        <ArtifactsPanel
          guard={guard}
          onExport={onExportPreview}
          onScreenshot={showPreview}
          previewOpen={previewOpen}
          requestedTaskId={fileTask}
          onRequestHandled={() => setFileTask(null)}
        />
      </div>
      <div className="task-filters" role="group" aria-label={t('任务状态筛选')}>
        {filters.map(([value, label]) => (
          <button
            key={value}
            aria-pressed={state === value}
            disabled={deleting}
            className={state === value ? 'selected' : ''}
            onClick={() => {
              setState(value);
              setOffset(0);
            }}
          >
            {t(label)}
            <span>{count(value)}</span>
          </button>
        ))}
      </div>
      {page?.deletion && (
        <p className="task-cleanup-scope">
          {scope} · {t('可清理 {0} 条', page.deletion.eligible)}
          {' · '}
          {t(
            '保留：进行中 {0} 条、文件未处理 {1} 条、待检查 {2} 条、收尾或恢复中 {3} 条',
            page.deletion.active,
            page.deletion.files,
            page.deletion.review,
            page.deletion.busy,
          )}
        </p>
      )}
      {managing && (
        <div className="task-selection">
          <label>
            <input
              type="checkbox"
              aria-label={t('全选本页可删除项')}
              checked={allSelected}
              ref={(el) => {
                if (el)
                  el.indeterminate =
                    !allSelected && selectable.some((task) => selected.includes(task.id));
              }}
              disabled={deleting || !scopeReady || !selectable.length}
              onChange={(event) =>
                setSelected(event.target.checked ? selectable.map((task) => task.id) : [])
              }
            />
            {t('全选本页可删除项')}
          </label>
          <span>{t('已选 {0} 项', selected.length)}</span>
          <button
            className="button danger"
            disabled={deleting || !scopeReady || !selected.length}
            onClick={() => void deleteRecords({ taskIds: selected, query: null })}
          >
            <Trash2 size={13} />
            {t('删除所选（{0}）', selected.length)}
          </button>
        </div>
      )}
      {selectionNotice && (
        <p className="hint" role="status">
          {selectionNotice}
        </p>
      )}
      {deleting && (
        <p className="task-delete-feedback" role="status">
          {t('正在删除…')}
        </p>
      )}
      {deleteError && (
        <p className="error-banner" role="alert">
          {deleteError}
        </p>
      )}
      {result && (
        <div className="task-delete-feedback" role="status">
          <p>
            {t('已删除 {0} 条任务记录，保留 {1} 条', result.deleted, result.issues.length)}
            {result.alreadyDeleted > 0 && ' · ' + t('{0} 条此前已删除', result.alreadyDeleted)}
          </p>
          {result.issues.length > 0 && (
            <details>
              <summary>{t('查看未删除项')}</summary>
              <ul>
                {result.issues.map((issue) => (
                  <li key={issue.taskId}>
                    {t(issue.title) || issue.taskId} · {t(issue.reason)}
                  </li>
                ))}
              </ul>
              {result.issues.some((issue) => issue.retryable) && (
                <button
                  className="button"
                  disabled={deleting}
                  onClick={() =>
                    void deleteRecords({
                      taskIds: result.issues
                        .filter((issue) => issue.retryable)
                        .map((issue) => issue.taskId),
                      query: null,
                    })
                  }
                >
                  {t('重试未删除项')}
                </button>
              )}
            </details>
          )}
        </div>
      )}
      {query.error && (
        <p className="error-banner" role="alert">
          {t(errorText(query.error))}
        </p>
      )}
      {page?.tasks.length ? (
        <div className="task-list" aria-label={t('任务列表')} aria-busy={query.isFetching}>
          {page.tasks.map((task) => (
            <TaskRow
              key={task.id}
              task={task}
              blocked={blocked(task)}
              managing={managing}
              selected={selected.includes(task.id)}
              disabled={deleting || !scopeReady}
              onSelect={(checked) =>
                setSelected((current) =>
                  checked
                    ? [...current.filter((id) => id !== task.id), task.id]
                    : current.filter((id) => id !== task.id),
                )
              }
              onDelete={() => void deleteRecords({ taskIds: [task.id], query: null })}
              onFiles={() => setFileTask(task.id)}
              guard={guard}
              onExport={onExport}
              act={act}
              retry={() =>
                void guard(async () =>
                  showPlan(await call<Plan>('task_retry', { taskId: task.id })),
                )
              }
              launch={() =>
                void prepare(
                  'app_launch',
                  { package: task.operation.args.package ?? '' },
                  task.operation.target,
                )
              }
              preview={() =>
                void guard(async () =>
                  showPreview(
                    await call<string>('artifact_copy', { taskId: task.id, preview: true }),
                  ),
                )
              }
            />
          ))}
        </div>
      ) : (
        <div className="empty-state task-empty">
          <Activity size={28} />
          <h3>
            {query.isPending && native
              ? t('正在读取任务…')
              : search || state !== 'all'
                ? t('没有匹配的任务')
                : t('暂无任务记录')}
          </h3>
          <p>
            {search || state !== 'all'
              ? t('试试其他关键词或状态筛选。')
              : t('新的操作结果会继续记录在这里。')}
          </p>
          {(search || state !== 'all') && (
            <button className="text-button" onClick={clear}>
              {t('清除筛选')}
            </button>
          )}
        </div>
      )}
      {page && page.total > 0 && (
        <div className="task-pagination">
          <span>
            {page.offset + 1}–{Math.min(page.offset + page.tasks.length, page.total)} / {page.total}{' '}
            {t('项')}
          </span>
          <button
            className="button"
            disabled={page.offset === 0 || query.isFetching || deleting}
            onClick={() => setOffset(Math.max(0, page.offset - page.pageSize))}
          >
            {t('上一页')}
          </button>
          <button
            className="button"
            disabled={page.offset + page.pageSize >= page.total || query.isFetching || deleting}
            onClick={() => setOffset(page.offset + page.pageSize)}
          >
            {t('下一页')}
          </button>
        </div>
      )}
      <p className="hint">{t('删除直接执行，记录与诊断无法恢复；已另存文件和操作结果保留。')}</p>
    </div>
  );
}
function TaskRow({
  task,
  guard,
  onExport,
  act,
  retry,
  preview,
  launch,
  blocked,
  managing,
  selected,
  disabled,
  onSelect,
  onDelete,
  onFiles,
}: {
  task: Task;
  guard: Guard;
  onExport: (request: ExportRequest) => void;
  act: (command: string, args: Record<string, unknown>) => Promise<void>;
  retry: () => void;
  preview: () => void;
  launch: () => void;
  blocked: string;
  managing: boolean;
  selected: boolean;
  disabled: boolean;
  onSelect: (checked: boolean) => void;
  onDelete: () => void;
  onFiles: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [clearingRecording, setClearingRecording] = useState(false);
  const detailsId = useId();
  const canLaunch =
    task.state === 'succeeded' &&
    ['apk_install', 'apk_install_wait', 'app_reinstall', 'apk_set_install'].includes(
      task.operation.kind,
    ) &&
    task.operation.args.package;
  const isCapture = task.artifact && task.operation.kind === 'capture';
  const active = taskActive(task.state);
  const recording =
    task.operation.kind === 'log_record' &&
    !active &&
    (!!task.artifact || blocked.includes(t('原始采集仍在，请先处理采集文件')));
  const pendingFile = blocked.includes(t('有待保存文件')) || blocked.includes('待保存文件');
  const formattedTime = new Date(task.createdAt * 1000).toLocaleString(getLocale(), {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
  const message = `${targetName(task)} · ${formattedTime} · ${t(task.message) || t('暂无状态说明')}`;
  return (
    <article
      className={`task-card ${expanded ? 'expanded' : ''}`}
      aria-label={t(task.title)}
      data-task-id={task.id}
    >
      <div className="task-row">
        {managing ? (
          <input
            type="checkbox"
            className="task-select"
            checked={selected}
            disabled={disabled || !!blocked}
            aria-label={t('选择任务：{0}', t(task.title))}
            aria-describedby={`${detailsId}-condition`}
            onChange={(event) => onSelect(event.target.checked)}
          />
        ) : (
          <div className="task-symbol">
            <Activity size={16} />
          </div>
        )}
        <div className="task-overview">
          <div className="task-heading">
            <strong title={t(task.title)}>{t(task.title)}</strong>
            <span className={`badge ${task.state}`}>
              <i />
              {t(taskState[task.state] ?? task.state)}
            </span>
          </div>
          <p className="task-message" title={message}>
            <span>
              {targetName(task)} ·{' '}
              <time dateTime={new Date(task.createdAt * 1000).toISOString()}>{formattedTime}</time>{' '}
              ·{' '}
            </span>
            <span>{t(task.message) || t('暂无状态说明')}</span>
          </p>
        </div>
        <div className="task-row-actions">
          {active && (
            <TaskAction
              label={
                task.cancelRequested
                  ? t('正在取消')
                  : task.cancellable
                    ? t(
                        task.operation.kind === 'screen_record'
                          ? task.state === 'queued'
                            ? '取消等待录屏'
                            : '停止并保存录屏'
                          : '取消',
                      )
                    : t('此步骤完成后取消')
              }
              disabled={disabled || task.cancelRequested}
              onClick={() => void act('task_cancel', { taskId: task.id })}
            >
              <X size={14} />
            </TaskAction>
          )}
          {['failed', 'needs_review', 'cancelled'].includes(task.state) &&
            !['file_upload', 'file_download'].includes(task.operation.kind) && (
              <TaskAction
                label={t(
                  task.state === 'needs_review' &&
                    ['avd_start', 'avd_cold', 'avd_wait'].includes(task.operation.kind)
                    ? '继续等待系统就绪'
                    : task.state === 'needs_review' && task.operation.kind === 'screen_record'
                      ? '取回原录屏'
                      : '重试',
                )}
                disabled={disabled}
                onClick={retry}
              >
                <RefreshCw size={14} />
              </TaskAction>
            )}
          {canLaunch && (
            <TaskAction label={t('启动应用')} disabled={disabled} onClick={launch}>
              <Play size={14} />
            </TaskAction>
          )}
          {pendingFile && (
            <TaskAction label={t('处理文件')} disabled={disabled} onClick={onFiles}>
              <FileDown size={14} />
            </TaskAction>
          )}
          {isCapture && (
            <TaskAction label={t('预览截图')} disabled={disabled} onClick={preview}>
              <Image size={14} />
            </TaskAction>
          )}
          {recording && (
            <TaskAction label={t('处理采集')} disabled={disabled} onClick={() => setExpanded(true)}>
              <FileText size={14} />
            </TaskAction>
          )}
          <button
            className="task-expand task-icon-button"
            aria-label={expanded ? t('收起详情') : t('展开详情')}
            data-tooltip={expanded ? t('收起详情') : t('展开详情')}
            disabled={disabled}
            aria-expanded={expanded}
            aria-controls={detailsId}
            onClick={() => setExpanded(!expanded)}
          >
            {expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
          </button>
          <TaskAction
            label={t('删除任务记录')}
            tooltip={blocked || t('删除任务记录')}
            danger
            disabled={disabled || !!blocked}
            onClick={onDelete}
          >
            <Trash2 size={14} />
          </TaskAction>
        </div>
      </div>
      <span className="sr-only" id={`${detailsId}-condition`}>
        {t(blocked) || t('点击删除直接移除记录')}
      </span>
      {task.progress !== null && active && (
        <progress aria-label={t('任务进度')} value={task.progress} max={100} />
      )}
      {expanded && (
        <section className="task-details" id={detailsId}>
          <dl>
            <dt>{t('任务名称')}</dt>
            <dd>{t(task.title)}</dd>
            <dt>{t('任务 ID')}</dt>
            <dd>{task.id}</dd>
            <dt>{t('目标')}</dt>
            <dd>{targetName(task)}</dd>
            <dt>{t('创建时间')}</dt>
            <dd>{new Date(task.createdAt * 1000).toLocaleString(getLocale())}</dd>
            <dt>{t('当前阶段')}</dt>
            <dd>{task.phase}</dd>
            <dt>{t('删除条件')}</dt>
            <dd>{t(blocked) || t('点击删除直接移除记录')}</dd>
            {task.operation.args.sdkDownloadSource && (
              <>
                <dt>{t('下载源')}</dt>
                <dd>{task.operation.args.sdkDownloadSource}</dd>
                <dt>{t('仓库根地址')}</dt>
                <dd>{task.operation.args.sdkDownloadUrl}</dd>
                <dt>{t('官方校验')}</dt>
                <dd>
                  {task.operation.args.sdkOfficialVerification === 'true'
                    ? t('已开启')
                    : t('已关闭，按下载源记录验证完整性')}
                </dd>
              </>
            )}
          </dl>
          <p>{t(task.message)}</p>
          {isCapture && (
            <div className="task-actions">
              <button
                className="button"
                onClick={() => void act('artifact_save', { taskId: task.id })}
              >
                {t('保存截图')}
              </button>
              <button
                className="button"
                onClick={() => void act('artifact_copy', { taskId: task.id, preview: false })}
              >
                <Copy size={12} />
                {t('复制截图')}
              </button>
            </div>
          )}
          {recording && <RecordingExport taskId={task.id} guard={guard} onExport={onExport} />}
          {recording && (
            <div className="task-recording-cleanup">
              <p className="hint">
                {t('按需导出后可清理原始采集；清理后无法再次导出，已另存文件和任务记录保留。')}
              </p>
              <button
                className="button danger"
                disabled={disabled || clearingRecording}
                onClick={() => {
                  if (clearingRecording) return;
                  setClearingRecording(true);
                  void act('recording_discard', { taskId: task.id }).finally(() =>
                    setClearingRecording(false),
                  );
                }}
              >
                <Trash2 size={13} />
                {clearingRecording ? t('正在清理…') : t('清理原始采集')}
              </button>
            </div>
          )}
          <pre className="task-log">{task.log.join('\n') || t('暂无额外诊断输出')}</pre>
        </section>
      )}
      {task.licensePrompt && (
        <div className="license">
          <b>{t('官方工具正在等待许可确认')}</b>
          <pre>{task.licensePrompt}</pre>
          <button
            className="button"
            onClick={() => void act('license_respond', { taskId: task.id, accept: false })}
          >
            {t('拒绝')}
          </button>
          <button
            className="button primary"
            onClick={() => void act('license_respond', { taskId: task.id, accept: true })}
          >
            {t('接受当前许可')}
          </button>
        </div>
      )}
    </article>
  );
}
function TaskAction({
  label,
  tooltip,
  children,
  disabled,
  danger,
  onClick,
}: {
  label: string;
  tooltip?: string;
  children: ReactNode;
  disabled?: boolean;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      className={`task-icon-button${danger ? ' danger' : ''}`}
      aria-label={label}
      data-tooltip={tooltip || label}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
