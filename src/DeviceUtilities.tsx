import { useEffect, useRef, useState } from 'react';
import { AppWindow, Download, Monitor, Play, RefreshCw, Square, Trash2 } from 'lucide-react';
import { call, deviceKey, errorText, taskActive } from './api';
import { t } from './i18n';
import { Button, Field, Modal } from './ui';
import { getPreference, savePreference, usePreferenceValue } from './preferences';
import type { Avd, Workspace } from './types';
import type { Usage } from './bindings';
import type {
  Device,
  DeviceAppDetail,
  DeviceAppList,
  DeviceToolStatus,
  Target,
  Task,
} from './bindings';
import './device-utilities.css';

export const preferenceKey = (avd?: Avd, device?: Device) =>
  avd ? `avd:${avd.id}` : device ? `device:${device.connection}:${device.serial}` : '';
export function defaultRow(data?: Workspace, value = getPreference('defaultTarget')): string {
  const avd = data?.avds.find((avd) => preferenceKey(avd) === value);
  if (avd) return avd.id;
  const matches =
    data?.devices.filter((device) => preferenceKey(undefined, device) === value) ?? [];
  return matches.length === 1 ? deviceKey(matches[0]) : '';
}
export function useDefaultTarget() {
  const value = usePreferenceValue('defaultTarget');
  return [value, (next: string) => savePreference('defaultTarget', next)] as const;
}
export const bytes = (value: number) =>
  value >= 1024 ** 3
    ? `${(value / 1024 ** 3).toFixed(2)} GiB`
    : `${(value / 1024 ** 2).toFixed(1)} MiB`;

export function AvdUsage({ avd }: { avd: Avd }) {
  const [usage, setUsage] = useState<Usage | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let current = true;
    setBusy(true);
    setError('');
    setUsage(null);
    void call<Usage>('avd_usage', { avdId: avd.id })
      .then((value) => {
        if (!value) throw new Error(t('无法读取磁盘占用'));
        if (current) setUsage(value);
      })
      .catch((error) => {
        if (current) setError(errorText(error));
      })
      .finally(() => {
        if (current) setBusy(false);
      });
    return () => {
      current = false;
    };
  }, [avd.id, revision]);
  return (
    <div className="avd-usage">
      <span>
        {t('实际磁盘占用')}：
        {busy
          ? t('正在统计…')
          : usage
            ? `${bytes(usage.bytes)}${usage.complete ? '' : t('（仅已统计部分）')}`
            : t('待获取')}
      </span>
      <Button disabled={busy} onClick={() => setRevision((value) => value + 1)}>
        {t('重新统计')}
      </Button>
      {error && (
        <p className="error-banner" role="alert">
          {t(error)}
        </p>
      )}
      <small>
        {t('按需统计已分配文件空间，不跟随链接；共享镜像和外部存储不计入，运行中占用可能变化。')}
      </small>
    </div>
  );
}

type Prepare = (
  kind: string,
  args?: Record<string, string>,
  target?: Target | null,
) => Promise<void>;
type Mode = 'apps' | 'mirror' | null;
type AppAction = 'launch' | 'stop' | 'clear' | 'uninstall' | 'export';

function sameTarget(task: Task, target: Target) {
  const other = task.operation.target;
  return (
    !!other &&
    other.serial === target.serial &&
    other.transportId === target.transportId &&
    other.serverEpoch === target.serverEpoch
  );
}

export function DeviceUtilities({
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
  const [mode, setMode] = useState<Mode>(null);
  const [busy, setBusy] = useState(false);
  const working = useRef(false);
  const [error, setError] = useState('');
  const [apps, setApps] = useState<DeviceAppList | null>(null);
  const [listLoading, setListLoading] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [query, setQuery] = useState('');
  const [includeSystem, setIncludeSystem] = useState(false);
  const [selected, setSelected] = useState('');
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const [detail, setDetail] = useState<DeviceAppDetail | null>(null);
  const [tools, setTools] = useState<DeviceToolStatus[] | null>(null);
  const [toolsLoading, setToolsLoading] = useState(false);
  const [control, setControl] = useState('true');
  const [maxSize, setMaxSize] = useState('1280');
  const [maxFps, setMaxFps] = useState('60');
  const targetKey = deviceKey(target);
  const ready = target.state === 'ready';
  const generation = useRef(0);
  const listRequest = useRef(0);
  const detailRequest = useRef(0);
  const toolRequest = useRef(0);
  const view = useRef({ mode, targetKey });
  view.current = { mode, targetKey };
  const appList = useRef<HTMLDivElement>(null);

  const valid = (version: number, expectedMode: Mode, key = targetKey) =>
    generation.current === version &&
    view.current.mode === expectedMode &&
    view.current.targetKey === key;

  useEffect(() => {
    // A reused component must never retain a dialog for a previous connection.
    generation.current++;
    setMode(null);
    setApps(null);
    setSelected('');
    setDetail(null);
    setListLoading(false);
    setDetailLoading(false);
    setToolsLoading(false);
    return () => {
      generation.current++;
    };
  }, [targetKey]);

  const close = () => {
    if (working.current) return;
    generation.current++;
    setMode(null);
    setError('');
    setApps(null);
    setSelected('');
    setDetail(null);
    setListLoading(false);
    setDetailLoading(false);
    setToolsLoading(false);
    setQuery('');
    setIncludeSystem(false);
    setControl('true');
    setMaxSize('1280');
    setMaxFps('60');
  };
  const open = (next: Exclude<Mode, null>) => {
    generation.current++;
    setMode(next);
    setError('');
    setTools(null);
  };

  const run = async (work: () => Promise<void>) => {
    if (working.current) return;
    const version = generation.current;
    const currentMode = mode;
    working.current = true;
    setBusy(true);
    setError('');
    try {
      await work();
    } catch (e) {
      if (valid(version, currentMode)) setError(errorText(e));
    } finally {
      working.current = false;
      setBusy(false);
    }
  };

  const loadApps = async () => {
    const version = generation.current;
    const request = ++listRequest.current;
    setListLoading(true);
    setError('');
    try {
      const result = await call<DeviceAppList>('device_app_list', { target });
      if (!valid(version, 'apps') || request !== listRequest.current) return;
      setApps(result);
      if (!result.apps.some((app) => app.package === selectedRef.current)) {
        setSelected('');
        setDetail(null);
      }
    } catch (e) {
      if (valid(version, 'apps') && request === listRequest.current) {
        setApps(null);
        setDetail(null);
        setError(errorText(e));
      }
    } finally {
      if (valid(version, 'apps') && request === listRequest.current) setListLoading(false);
    }
  };
  const loadTools = async () => {
    const version = generation.current;
    const request = ++toolRequest.current;
    setToolsLoading(true);
    setError('');
    try {
      const result = await call<DeviceToolStatus[]>('device_tools');
      if (valid(version, 'mirror') && request === toolRequest.current) setTools(result);
    } catch (e) {
      if (valid(version, 'mirror') && request === toolRequest.current) {
        setTools(null);
        setError(errorText(e));
      }
    } finally {
      if (valid(version, 'mirror') && request === toolRequest.current) setToolsLoading(false);
    }
  };

  const settledApps = tasks
    .filter(
      (task) =>
        task.operation.kind.startsWith('device_app_') &&
        sameTarget(task, target) &&
        !taskActive(task.state),
    )
    .map((task) => `${task.id}:${task.state}`)
    .sort()
    .join(',');
  const settledTools = tasks
    .filter(
      (task) =>
        task.operation.kind === 'device_tool_prepare' &&
        task.operation.args.tool === 'scrcpy' &&
        !taskActive(task.state),
    )
    .map((task) => `${task.id}:${task.state}`)
    .sort()
    .join(',');

  useEffect(() => {
    if (mode === 'apps' && !planOpen && ready) void loadApps();
  }, [mode, planOpen, ready, targetKey, settledApps]);
  useEffect(() => {
    if (mode === 'mirror' && !planOpen) void loadTools();
  }, [mode, planOpen, targetKey, settledTools]);
  useEffect(() => {
    if (mode !== 'apps' || !selected || planOpen || !ready) return;
    const version = generation.current;
    const request = ++detailRequest.current;
    setDetail(null);
    setDetailLoading(true);
    setError('');
    void call<DeviceAppDetail>('device_app_detail', { target, package: selected })
      .then((result) => {
        if (
          valid(version, 'apps') &&
          request === detailRequest.current &&
          selectedRef.current === selected
        )
          setDetail(result);
      })
      .catch((e) => {
        if (valid(version, 'apps') && request === detailRequest.current) setError(errorText(e));
      })
      .finally(() => {
        if (valid(version, 'apps') && request === detailRequest.current) setDetailLoading(false);
      });
    return () => {
      detailRequest.current++;
    };
  }, [mode, selected, planOpen, ready, targetKey, settledApps, apps]);

  const filtered =
    apps?.apps.filter(
      (app) =>
        (includeSystem || !app.system) &&
        app.package.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
    ) ?? [];
  const activeMirror = tasks.find(
    (task) =>
      task.operation.kind === 'mirror_start' && sameTarget(task, target) && taskActive(task.state),
  );
  const lastMirror = [...tasks]
    .filter((task) => task.operation.kind === 'mirror_start' && sameTarget(task, target))
    .sort((a, b) => b.updatedAt - a.updatedAt)[0];
  const preparingTool = tasks.find(
    (task) =>
      task.operation.kind === 'device_tool_prepare' &&
      task.operation.args.tool === 'scrcpy' &&
      taskActive(task.state),
  );
  const scrcpy = tools?.find((tool) => tool.name === 'scrcpy');
  const selectedVisible = filtered.some((app) => app.package === selected);
  const shownDetail = selectedVisible && detail?.app.package === selected ? detail : null;
  const unavailable = !ready || busy || listLoading || detailLoading || !shownDetail;
  const mutate = (action: AppAction) =>
    void run(async () => {
      if (!shownDetail || unavailable) return;
      if (shownDetail.app.system && ['stop', 'clear', 'uninstall'].includes(action)) return;
      const args = await call<Record<string, string>>('device_app_prepare', {
        target,
        package: shownDetail.app.package,
        action,
      });
      await prepare(`device_app_${action}`, args, target);
    });

  return (
    <>
      <div className="detail-actions device-utilities" aria-label={t('设备扩展操作')}>
        <Button disabled={!ready} onClick={() => open('apps')}>
          <AppWindow size={15} />
          {t('应用管理')}
        </Button>
        <Button disabled={!ready} onClick={() => open('mirror')}>
          <Monitor size={15} />
          {t('投屏与控制')}
        </Button>
      </div>
      {mode && (
        <Modal
          title={t(mode === 'apps' ? '应用管理' : '投屏与控制')}
          onClose={close}
          hidden={planOpen}
          dismissible={!busy}
          trapFocus
          wide={mode === 'apps'}
          className={`device-utilities-modal ${mode === 'apps' ? 'device-app-modal' : ''}`}
        >
          <p className="hint device-utilities-target">
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
          {mode === 'apps' ? (
            <>
              <div className="device-app-toolbar">
                <Field label="搜索已安装应用">
                  <input
                    type="search"
                    placeholder={t('输入包名')}
                    value={query}
                    disabled={busy}
                    onChange={(event) => setQuery(event.target.value)}
                  />
                </Field>
                <label className="check-row">
                  <input
                    type="checkbox"
                    checked={includeSystem}
                    disabled={busy}
                    onChange={(event) => setIncludeSystem(event.target.checked)}
                  />
                  {t('显示系统应用')}
                </label>
                <Button
                  disabled={!ready || busy || listLoading}
                  loading={listLoading}
                  onClick={() => void loadApps()}
                >
                  <RefreshCw size={14} />
                  {t('刷新应用列表')}
                </Button>
              </div>
              {apps && (
                <p className="hint" role="status">
                  {t('Android 用户 {0} · {1} 个应用', apps.userId, filtered.length)}
                </p>
              )}
              <div className="device-app-layout" aria-busy={listLoading || detailLoading}>
                <div
                  ref={appList}
                  className="device-app-list"
                  role="listbox"
                  aria-label={t('已安装应用')}
                >
                  {filtered.map((app, index) => (
                    <button
                      key={app.package}
                      role="option"
                      aria-selected={selected === app.package}
                      tabIndex={
                        selectedVisible ? (selected === app.package ? 0 : -1) : index === 0 ? 0 : -1
                      }
                      className={`device-app-row ${selected === app.package ? 'selected' : ''}`}
                      disabled={busy || !ready || listLoading}
                      onClick={() => setSelected(app.package)}
                      onKeyDown={(event) => {
                        const next =
                          event.key === 'ArrowDown'
                            ? Math.min(filtered.length - 1, index + 1)
                            : event.key === 'ArrowUp'
                              ? Math.max(0, index - 1)
                              : event.key === 'Home'
                                ? 0
                                : event.key === 'End'
                                  ? filtered.length - 1
                                  : -1;
                        if (next < 0) return;
                        event.preventDefault();
                        setSelected(filtered[next].package);
                        appList.current
                          ?.querySelectorAll<HTMLButtonElement>('[role=option]')
                          [next]?.focus();
                      }}
                    >
                      <strong>{app.package}</strong>
                      <span>
                        {t(app.system ? '系统应用' : '用户应用')} · {app.versionCode || '—'}
                        {app.disabled ? ` · ${t('已停用')}` : ''}
                      </span>
                    </button>
                  ))}
                  {!listLoading && !filtered.length && (
                    <p className="hint device-app-empty">
                      {t(apps ? '没有匹配的应用' : '点击刷新读取应用列表')}
                    </p>
                  )}
                </div>
                <section className="device-app-detail" aria-label={t('应用详情')}>
                  {shownDetail ? (
                    <>
                      <h3>{shownDetail.app.package}</h3>
                      <dl className="device-app-properties">
                        <dt>{t('版本名称')}</dt>
                        <dd>{shownDetail.versionName || '—'}</dd>
                        <dt>versionCode</dt>
                        <dd>{shownDetail.app.versionCode || '—'}</dd>
                        <dt>{t('Android 用户')}</dt>
                        <dd>{shownDetail.userId}</dd>
                        <dt>UID</dt>
                        <dd>{shownDetail.app.uid || '—'}</dd>
                        <dt>minSdk / targetSdk</dt>
                        <dd>
                          {shownDetail.minSdk || '—'} / {shownDetail.targetSdk || '—'}
                        </dd>
                        <dt>{t('APK 数量')}</dt>
                        <dd>{shownDetail.paths.length}</dd>
                      </dl>
                      <details className="device-app-paths">
                        <summary>{t('查看安装路径')}</summary>
                        <ul>
                          {shownDetail.paths.map((path) => (
                            <li key={path}>{path}</li>
                          ))}
                        </ul>
                      </details>
                      <div className="device-app-actions">
                        <Button
                          disabled={unavailable || shownDetail.app.disabled}
                          onClick={() => mutate('launch')}
                        >
                          <Play size={14} />
                          {t('启动应用')}
                        </Button>
                        <Button
                          disabled={unavailable || shownDetail.app.system}
                          onClick={() => mutate('stop')}
                        >
                          <Square size={14} />
                          {t('停止应用')}
                        </Button>
                        <Button disabled={unavailable} onClick={() => mutate('export')}>
                          <Download size={14} />
                          {t('导出 APK')}
                        </Button>
                        <Button
                          danger
                          disabled={unavailable || shownDetail.app.system}
                          onClick={() => mutate('clear')}
                        >
                          {t('清除应用数据')}
                        </Button>
                        <Button
                          danger
                          disabled={unavailable || shownDetail.app.system}
                          onClick={() => mutate('uninstall')}
                        >
                          <Trash2 size={14} />
                          {t('卸载应用')}
                        </Button>
                      </div>
                      <p className="hint">
                        {t(
                          shownDetail.app.system
                            ? '系统应用仅提供查看、启动与 APK 导出。'
                            : '清除数据不可恢复；卸载会移除此 Android 用户的应用及数据。执行前请核对确认计划。',
                        )}
                      </p>
                      <p className="hint">
                        {t(
                          'APK 导出包含已安装的 base 与 split，不包含应用数据；结果在待保存文件中另存。',
                        )}
                      </p>
                    </>
                  ) : (
                    <p className="hint device-app-empty" role="status">
                      {t(detailLoading ? '正在读取应用详情…' : '选择一个应用查看详情和操作')}
                    </p>
                  )}
                </section>
              </div>
            </>
          ) : (
            <>
              <p className="hint">{t('投屏在独立窗口打开，可使用键盘和鼠标操作所选设备。')}</p>
              <p className="hint">{t('投屏窗口中的键鼠和拖入操作会直接作用于当前设备。')}</p>
              <p className="hint">{t('默认不传输音频，也不自动同步剪贴板。')}</p>
              {toolsLoading ? (
                <p role="status">{t('正在检查投屏工具…')}</p>
              ) : scrcpy?.ready ? (
                <p className="hint">
                  {t('投屏工具已就绪')} · scrcpy {scrcpy.version}
                </p>
              ) : (
                <div className="device-tool-setup">
                  <p>{t('首次使用需要下载并校验官方 scrcpy 工具，仅安装到应用私有目录。')}</p>
                  <Button
                    disabled={busy || !!preparingTool || !!activeMirror}
                    onClick={() =>
                      void run(() => prepare('device_tool_prepare', { tool: 'scrcpy' }))
                    }
                  >
                    <Download size={14} />
                    {t(preparingTool ? '工具准备中' : '准备投屏工具')}
                  </Button>
                </div>
              )}
              <div className="device-mirror-options">
                <Field label="控制模式">
                  <select
                    value={control}
                    disabled={busy || !!activeMirror}
                    onChange={(event) => setControl(event.target.value)}
                  >
                    <option value="true">{t('键盘和鼠标控制')}</option>
                    <option value="false">{t('仅查看画面')}</option>
                  </select>
                </Field>
                <Field label="画面最长边">
                  <select
                    value={maxSize}
                    disabled={busy || !!activeMirror}
                    onChange={(event) => setMaxSize(event.target.value)}
                  >
                    <option value="0">{t('原始分辨率')}</option>
                    <option value="1280">1280 px</option>
                    <option value="1920">1920 px</option>
                    <option value="2560">2560 px</option>
                  </select>
                </Field>
                <Field label="最高帧率">
                  <select
                    value={maxFps}
                    disabled={busy || !!activeMirror}
                    onChange={(event) => setMaxFps(event.target.value)}
                  >
                    <option value="30">30 FPS</option>
                    <option value="60">60 FPS</option>
                  </select>
                </Field>
              </div>
              {activeMirror ? (
                <div className="device-mirror-session" role="status">
                  <p>{t(activeMirror.message) || t('投屏会话正在运行')}</p>
                  <Button
                    disabled={busy || activeMirror.cancelRequested}
                    onClick={() =>
                      void run(async () => {
                        await call('task_cancel', { taskId: activeMirror.id });
                      })
                    }
                  >
                    <Square size={14} />
                    {t(activeMirror.cancelRequested ? '正在停止投屏' : '停止投屏')}
                  </Button>
                </div>
              ) : (
                <>
                  {lastMirror && ['failed', 'needs_review'].includes(lastMirror.state) && (
                    <p className="warning-text" role="status">
                      {t(lastMirror.message)}
                    </p>
                  )}
                  <div className="modal-actions">
                    <Button disabled={busy || toolsLoading} onClick={() => void loadTools()}>
                      <RefreshCw size={14} />
                      {t('重新检查工具')}
                    </Button>
                    <Button
                      primary
                      disabled={!ready || busy || toolsLoading || !scrcpy?.ready || !!preparingTool}
                      onClick={() =>
                        void run(() =>
                          prepare('mirror_start', { control, maxSize, maxFps }, target),
                        )
                      }
                    >
                      <Monitor size={14} />
                      {t('开始投屏')}
                    </Button>
                  </div>
                </>
              )}
              <p className="hint">
                {t('关闭投屏窗口或点击停止可结束本次会话；不会关闭设备或共享 ADB。')}
              </p>
            </>
          )}
        </Modal>
      )}
    </>
  );
}
