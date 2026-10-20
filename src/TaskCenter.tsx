import { t, getLocale } from './i18n';
import { useEffect, useId, useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import {
  Activity,
  ChevronDown,
  ChevronRight,
  Copy,
  Play,
  RefreshCw,
  Search,
  X,
} from 'lucide-react';
import { call, errorText, native, taskActive } from './api';
import { ArtifactsPanel, RecordingExport, type ExportRequest } from './Observability';
import type { ExportPreview, Plan, Target, Task, TaskPage } from './bindings';
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
  const [state, setState] = useState('all');
  const [offset, setOffset] = useState(0);
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
    refetchInterval: 3000,
    placeholderData: keepPreviousData,
  });
  const page = query.data;
  useEffect(() => {
    if (page && !query.isPlaceholderData && page.offset !== offset) setOffset(page.offset);
  }, [page?.offset, query.isPlaceholderData, offset]);
  const counts = page?.counts ?? {};
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
            onChange={(event) => setSearch(event.target.value)}
          />
          {search && (
            <button
              aria-label={t('清空任务搜索')}
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
          disabled={query.isFetching || !native}
        >
          <RefreshCw size={13} className={query.isFetching ? 'spin' : ''} />
          {t('刷新')}
        </button>
        <ArtifactsPanel
          guard={guard}
          onExport={onExportPreview}
          onScreenshot={showPreview}
          previewOpen={previewOpen}
        />
      </div>
      <div className="task-filters" role="group" aria-label={t('任务状态筛选')}>
        {filters.map(([value, label]) => (
          <button
            key={value}
            aria-pressed={state === value}
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
                : t('任务会留在这里')}
          </h3>
          <p>
            {search || state !== 'all'
              ? t('试试其他关键词或状态筛选。')
              : t('安装、下载、创建和导出的结果均可回看。')}
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
            disabled={page.offset === 0 || query.isFetching}
            onClick={() => setOffset(Math.max(0, page.offset - page.pageSize))}
          >
            {t('上一页')}
          </button>
          <button
            className="button"
            disabled={page.offset + page.pageSize >= page.total || query.isFetching}
            onClick={() => setOffset(page.offset + page.pageSize)}
          >
            {t('下一页')}
          </button>
        </div>
      )}
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
}: {
  task: Task;
  guard: Guard;
  onExport: (request: ExportRequest) => void;
  act: (command: string, args: Record<string, unknown>) => Promise<void>;
  retry: () => void;
  preview: () => void;
  launch: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  const canLaunch =
    task.state === 'succeeded' &&
    ['apk_install', 'app_reinstall'].includes(task.operation.kind) &&
    task.operation.args.package;
  const isCapture = task.artifact && task.operation.kind === 'capture';
  const active = taskActive(task.state);
  return (
    <article
      className={`task-card ${expanded ? 'expanded' : ''}`}
      aria-label={t(task.title)}
      data-task-id={task.id}
    >
      <div className="task-row">
        <div className="task-symbol">
          <Activity size={16} />
        </div>
        <div className="task-overview">
          <div className="task-heading">
            <strong title={t(task.title)}>{t(task.title)}</strong>
            <span className={`badge ${task.state}`}>
              <i />
              {t(taskState[task.state] ?? task.state)}
            </span>
            <span className="task-target" title={targetName(task)}>
              {targetName(task)}
            </span>
          </div>
          <p className="task-message" title={t(task.message)}>
            {t(task.message) || t('暂无状态说明')}
          </p>
        </div>
        <time
          className="task-time"
          dateTime={new Date(task.createdAt * 1000).toISOString()}
          title={new Date(task.createdAt * 1000).toLocaleString(getLocale())}
        >
          {new Date(task.createdAt * 1000).toLocaleString(getLocale(), {
            month: '2-digit',
            day: '2-digit',
            hour: '2-digit',
            minute: '2-digit',
            hour12: false,
          })}
        </time>
        <div className="task-row-actions">
          {active && (
            <button
              className="button"
              disabled={task.cancelRequested}
              onClick={() => void act('task_cancel', { taskId: task.id })}
            >
              {task.cancelRequested
                ? t('正在取消')
                : task.cancellable
                  ? t('取消')
                  : t('此步骤完成后取消')}
            </button>
          )}
          {['failed', 'needs_review', 'cancelled'].includes(task.state) && (
            <button className="button" title={t('重新检查原任务后生成重试计划')} onClick={retry}>
              {t('重试')}
            </button>
          )}
          {canLaunch && (
            <button className="button" onClick={launch}>
              <Play size={12} />
              {t('启动应用')}
            </button>
          )}
          {isCapture && (
            <button className="button" onClick={preview}>
              {t('预览截图')}
            </button>
          )}
          <button
            className="task-expand text-button"
            aria-expanded={expanded}
            aria-controls={detailsId}
            onClick={() => setExpanded(!expanded)}
          >
            {expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
            {expanded ? t('收起详情') : t('展开详情')}
          </button>
        </div>
      </div>
      {task.progress !== null && active && (
        <progress aria-label={t('任务进度')} value={task.progress} max={100} />
      )}
      {expanded && (
        <section className="task-details" id={detailsId}>
          <dl>
            <dt>{t('任务 ID')}</dt>
            <dd>{task.id}</dd>
            <dt>{t('目标')}</dt>
            <dd>{targetName(task)}</dd>
            <dt>{t('创建时间')}</dt>
            <dd>{new Date(task.createdAt * 1000).toLocaleString(getLocale())}</dd>
            <dt>{t('当前阶段')}</dt>
            <dd>{task.phase}</dd>
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
          {task.operation.kind === 'log_record' && !active && task.artifact && (
            <RecordingExport taskId={task.id} guard={guard} onExport={onExport} />
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
