import { useEffect, useRef, useState } from 'react';
import { Eraser, FileDown, Pause, Play, ScrollText, Square } from 'lucide-react';
import { call, deviceKey, deviceName, errorText } from './api';
import type { Device, LogFilter, LogSnapshot, Workspace } from './bindings';
import { getLocale, t } from './i18n';
import type { ExportRequest } from './Observability';
import { useNumberPreference } from './preferences';
import { Button } from './ui';
import './logs.css';

type PrepareRecording = (
  kind: string,
  args: Record<string, string>,
  target: Device,
) => Promise<void>;
type LogAction = 'read' | 'start' | 'stop';

const closingViews = new Map<string, Promise<unknown>>();
const emptyFilter: LogFilter = {
  level: 'V',
  tag: '',
  keyword: '',
  package: '',
  pids: [],
  after: 0,
  before: 0,
};

export function LogsPage({
  data,
  preferredTarget,
  targetKey,
  setTargetKey,
  openDevices,
  prepare,
  onExport,
}: {
  data?: Workspace;
  preferredTarget?: Device;
  targetKey: string;
  setTargetKey: (key: string) => void;
  openDevices: () => void;
  prepare: PrepareRecording;
  onExport: (request: ExportRequest) => void;
}) {
  const devices = data?.devices.filter((device) => device.state === 'ready') ?? [];
  const preferredKey = preferredTarget ? deviceKey(preferredTarget) : '';
  const initialTarget =
    devices.find((device) => deviceKey(device) === preferredKey) ??
    (devices.length === 1 ? devices[0] : undefined);
  const initialKey = initialTarget ? deviceKey(initialTarget) : '';
  useEffect(() => {
    if (!targetKey && initialKey) setTargetKey(initialKey);
  }, [targetKey, initialKey, setTargetKey]);

  const target = devices.find((device) => deviceKey(device) === (targetKey || initialKey));
  return (
    <div className="logs-page">
      <label className="logs-target">
        {t('目标设备')}
        <select
          aria-label={t('目标设备')}
          value={target ? deviceKey(target) : ''}
          onChange={(event) => setTargetKey(event.target.value)}
          disabled={!devices.length}
        >
          <option value="" disabled>
            {t(devices.length ? '请选择设备' : '暂无可用设备')}
          </option>
          {devices.map((device) => {
            const name = deviceName(device, data);
            const key = deviceKey(device);
            return (
              <option key={key} value={key}>
                {name === device.serial ? name : `${name} · ${device.serial}`}
              </option>
            );
          })}
        </select>
      </label>
      {target ? (
        <LogPanel key={deviceKey(target)} target={target} prepare={prepare} onExport={onExport} />
      ) : (
        <div className="logs-empty">
          <ScrollText size={28} aria-hidden="true" />
          <h2>{t(devices.length ? '请选择日志目标' : '暂无可用设备')}</h2>
          {targetKey && devices.length > 0 && (
            <p>{t('目标设备暂不可用，请重新选择或等待设备恢复。')}</p>
          )}
          {!devices.length && (
            <>
              <p>{t('连接并授权一台 Android 设备，或启动模拟器。')}</p>
              <Button onClick={openDevices}>{t('前往设备工作台')}</Button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

function LogPanel({
  target,
  prepare,
  onExport,
}: {
  target: Device;
  prepare: PrepareRecording;
  onExport: (request: ExportRequest) => void;
}) {
  const [filter, setFilter] = useState(emptyFilter);
  const [pid, setPid] = useState('');
  const [snapshot, setSnapshot] = useState<LogSnapshot | null>(null);
  const [paused, setPaused] = useState(false);
  const [view, setView] = useState<LogSnapshot | null>(null);
  const [missed, setMissed] = useState(0);
  const [error, setError] = useState('');
  const [changing, setChanging] = useState(false);
  const [raw, setRaw] = useState(false);
  const [record, setRecord] = useState(false);
  const [quota, setQuota] = useNumberPreference('logQuotaMib', 8, 4096);
  const [minutes, setMinutes] = useNumberPreference('logMinutes', 1, 1440);
  const key = deviceKey(target);
  const pausedRef = useRef(false);
  const currentFilter = useRef(filter);
  const session = useRef({
    mounted: false,
    started: false,
    pending: null as Promise<void> | null,
  });
  const effective = { ...filter, pids: pid.split(/[ ,]+/).filter(Boolean).map(Number) };
  currentFilter.current = effective;
  pausedRef.current = paused;

  const read = (action: LogAction) => {
    const state = session.current;
    if (action === 'read' && state.pending) return;
    if (action !== 'read') setChanging(true);
    const previous = state.pending ?? closingViews.get(key);
    const request = (async () => {
      await previous;
      if (!state.mounted) return;
      try {
        if (action === 'start') state.started = true;
        const next = await call<LogSnapshot>('log_read', {
          target,
          action,
          filter: currentFilter.current,
        });
        if (!state.mounted) return;
        setSnapshot(next);
        if (!pausedRef.current) setView(next);
        setError('');
      } catch (error) {
        if (state.mounted) setError(errorText(error));
      }
    })().finally(() => {
      if (state.pending === request) state.pending = null;
      if (state.mounted && action !== 'read') setChanging(false);
    });
    state.pending = request;
  };

  useEffect(() => {
    const state = session.current;
    state.mounted = true;
    read('read');
    const timer = window.setInterval(() => read('read'), 1000);
    return () => {
      state.mounted = false;
      window.clearInterval(timer);
      if (state.started) {
        // Finish an in-flight start before releasing this target, even on a quick tab switch.
        const release = Promise.resolve(state.pending)
          .then(() => call('log_read', { target, action: 'stop', filter: emptyFilter }))
          .catch(() => {})
          .finally(() => {
            if (closingViews.get(key) === release) closingViews.delete(key);
          });
        closingViews.set(key, release);
      }
    };
  }, []);

  const set = (key: 'level' | 'tag' | 'package' | 'keyword', value: string) =>
    setFilter((current) => ({ ...current, [key]: value }));
  const visibleSnapshot = paused ? view : snapshot;
  const rows = view?.rows.map((row) => row.raw).join('\n');

  return (
    <div className="log-panel">
      <div className="log-actions">
        <Button
          primary={!snapshot?.active}
          loading={changing}
          onClick={() => read(snapshot?.active ? 'stop' : 'start')}
        >
          {snapshot?.active ? <Square size={14} /> : <Play size={14} />}
          {t(snapshot?.active ? '结束查看' : '开始查看')}
        </Button>
        <Button
          disabled={!snapshot?.active && !paused}
          onClick={() => {
            if (paused && snapshot) {
              setMissed(Math.max(0, snapshot.oldest - (view?.latest ?? 0) - 1));
              setView(snapshot);
            } else {
              setMissed(0);
            }
            setPaused(!paused);
          }}
        >
          {paused ? <Play size={14} /> : <Pause size={14} />}
          {t(paused ? '恢复视图' : '暂停视图')}
        </Button>
        <Button
          title="清空当前视图，保留设备上的日志。"
          disabled={!snapshot?.buffered}
          onClick={() => {
            setFilter((current) => ({ ...current, after: snapshot?.latest ?? 0 }));
            setView((current) => (current ? { ...current, rows: [] } : null));
          }}
        >
          <Eraser size={14} />
          {t('清空视图')}
        </Button>
      </div>
      <div className="log-filters">
        <label>
          {t('最低级别')}
          <select
            aria-label={t('日志最低级别')}
            value={filter.level}
            onChange={(event) => set('level', event.target.value)}
          >
            {['V', 'D', 'I', 'W', 'E', 'F'].map((level) => (
              <option key={level}>{level}</option>
            ))}
          </select>
        </label>
        <label>
          Tag
          <input
            aria-label={t('日志 Tag')}
            value={filter.tag}
            onChange={(event) => set('tag', event.target.value)}
          />
        </label>
        <label>
          PID
          <input
            aria-label={t('日志 PID')}
            placeholder="1234, 5678"
            title={t('多个用逗号分隔')}
            value={pid}
            onChange={(event) => setPid(event.target.value)}
          />
        </label>
        <label>
          {t('应用包名')}
          <input
            aria-label={t('日志应用包名')}
            placeholder="com.example.app"
            title={t('跟踪主进程与子进程')}
            value={filter.package}
            onChange={(event) => set('package', event.target.value)}
          />
        </label>
        <label>
          {t('关键词')}
          <input
            aria-label={t('过滤日志关键词')}
            value={filter.keyword}
            onChange={(event) => set('keyword', event.target.value)}
          />
        </label>
      </div>
      {error && (
        <p className="error-banner" role="alert">
          {t(error)}
        </p>
      )}
      <div className="log-console">
        <div className="log-console-heading">
          <h2>Logcat</h2>
          <span className="log-status" role="status">
            <span className={`connection-dot ${snapshot?.active ? 'online' : ''}`} />
            {t(snapshot?.active ? '实时查看中' : snapshot?.status || '尚未开始采集')}
            {paused && t(' · 视图暂停，后台缓冲继续更新')}
            {missed > 0 && t(' · 暂停期间至少遗漏 {0} 行', missed)}
          </span>
        </div>
        <div className="log-output">
          {rows ? (
            <pre className="log-lines" tabIndex={0} aria-label="Logcat">
              {rows}
            </pre>
          ) : (
            <p className="log-empty">
              {t(
                snapshot?.buffered || snapshot?.active
                  ? '当前过滤范围没有日志。'
                  : '开始查看后显示设备日志',
              )}
            </p>
          )}
        </div>
        <div className="log-console-footer">
          <span title={t('仅显示最近 500 行匹配的日志。')}>
            {t(
              '缓冲 {0} / 10,000 行 · 淘汰 {1} 行',
              (snapshot?.buffered ?? 0).toLocaleString(getLocale()),
              (snapshot?.dropped ?? 0).toLocaleString(getLocale()),
            )}
          </span>
          <span>
            {t(snapshot?.processStatus || '全部进程')}
            {snapshot?.pids.length ? ` · PID ${snapshot.pids.join(', ')}` : ''}
          </span>
        </div>
      </div>
      <div className="log-export">
        <label>
          <input type="checkbox" checked={raw} onChange={(event) => setRaw(event.target.checked)} />
          {t('导出原始内容（含敏感信息）')}
        </label>
        <div className="log-actions">
          <Button
            disabled={!visibleSnapshot}
            onClick={() =>
              onExport({
                kind: 'view',
                target,
                filter: { ...effective, before: visibleSnapshot?.latest ?? 0 },
                redacted: !raw,
              })
            }
          >
            <FileDown size={15} />
            {t('预览并导出当前视图')}
          </Button>
          <button
            className="button"
            aria-expanded={record}
            aria-controls="log-record-options"
            onClick={() => setRecord(!record)}
          >
            {t('文件采集…')}
          </button>
        </div>
      </div>
      <div className="log-record-options" id="log-record-options" hidden={!record}>
        <label>
          {t('总配额 MiB')}
          <input
            aria-label={t('采集总配额')}
            type="number"
            min={8}
            max={4096}
            value={quota}
            onChange={(event) => setQuota(Number(event.target.value))}
          />
        </label>
        <label>
          {t('时限 分钟')}
          <input
            aria-label={t('采集时限')}
            type="number"
            min={1}
            max={1440}
            value={minutes}
            onChange={(event) => setMinutes(Number(event.target.value))}
          />
        </label>
        <Button
          primary
          disabled={
            !Number.isInteger(quota) ||
            quota < 8 ||
            quota > 4096 ||
            !Number.isInteger(minutes) ||
            minutes < 1 ||
            minutes > 1440
          }
          onClick={() =>
            void prepare(
              'log_record',
              {
                filter: JSON.stringify({ ...effective, after: 0 }),
                quotaMiB: String(quota),
                seconds: String(minutes * 60),
              },
              target,
            )
          }
        >
          {t('查看采集计划')}
        </Button>
        <p className="hint">
          {t('按当前过滤条件采集；每段最多 100 MiB。切页不停止采集，在任务中心停止并导出。')}
        </p>
      </div>
    </div>
  );
}
