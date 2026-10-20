import {
  useEffect,
  useRef,
  useState,
  useId,
  Children,
  isValidElement,
  cloneElement,
  type ReactNode,
  type ReactElement,
} from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { create } from 'zustand';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import {
  Smartphone,
  Monitor,
  Package,
  Activity,
  Settings as SettingsIcon,
  Plus,
  RefreshCw,
  Wifi,
  Play,
  Square,
  Camera,
  Download,
  Search,
  ChevronRight,
  Check,
  Terminal,
  HardDrive,
  AlertCircle,
  X,
  Minus,
  Maximize2,
  Copy,
  Trash2,
  FileUp,
  ArrowLeft,
  SlidersHorizontal,
  ExternalLink,
  Layers,
} from 'lucide-react';
import {
  LogPanel,
  ExportDialog,
  ArtifactsPanel,
  RecordingExport,
  CloseDialog,
  type ExportRequest,
} from './Observability';
import type { ExportPreview } from './bindings';
import { call, native, errorText, stateLabel, taskActive, size, deviceKey } from './api';
import type {
  Settings,
  Workspace,
  Device,
  Avd,
  Plan,
  Operation,
  Task,
  ApkInfo,
  Target,
} from './types';

type Page = 'devices' | 'sdk' | 'tasks' | 'diagnostics' | 'settings';
const useUi = create<{
  page: Page;
  selected: string;
  setPage: (page: Page) => void;
  select: (selected: string) => void;
}>((set) => ({
  page: 'devices',
  selected: '',
  setPage: (page) => set({ page }),
  select: (selected) => set({ selected }),
}));
const titles: Record<Page, string> = {
  devices: '设备工作台',
  sdk: 'SDK 管理',
  tasks: '任务中心',
  diagnostics: '环境诊断',
  settings: '设置',
};
type ModalName = 'create' | 'wireless' | 'apk' | 'edit' | 'close' | null;
function Button({
  children,
  onClick,
  primary = false,
  danger = false,
  disabled = false,
  title,
}: {
  children: ReactNode;
  onClick?: () => void;
  primary?: boolean;
  danger?: boolean;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      title={title}
      disabled={disabled}
      className={`button ${primary ? 'primary' : ''} ${danger ? 'danger' : ''}`}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
function Badge({ state }: { state: string }) {
  return (
    <span className={`badge ${state}`}>
      <i />
      {stateLabel[state] ?? state}
    </span>
  );
}
function Modal({
  title,
  children,
  onClose,
  wide = false,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
}) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [onClose]);
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={`modal ${wide ? 'wide' : ''}`}
      >
        <div className="modal-heading">
          <h2>{title}</h2>
          <button className="icon-button" aria-label="关闭面板" onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        {children}
      </section>
    </div>
  );
}
function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  const id = useId();
  const connect = (node: ReactNode): ReactNode => {
    if (!isValidElement(node)) return node;
    const item = node as ReactElement<{ id?: string; children?: ReactNode }>;
    if (typeof item.type === 'string' && ['input', 'select', 'textarea'].includes(item.type))
      return cloneElement(item, { id });
    return item.props.children
      ? cloneElement(item, { children: Children.map(item.props.children, connect) })
      : item;
  };
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {Children.map(children, connect)}
      {hint && <small>{hint}</small>}
    </div>
  );
}

export function App() {
  const { page, setPage, selected, select } = useUi();
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ['workspace'],
    queryFn: () => call<Workspace>('workspace_snapshot'),
    enabled: native,
    refetchInterval: 5000,
  });
  const data = query.data;
  const [modal, setModal] = useState<ModalName>(null),
    [plan, setPlan] = useState<Plan | null>(null),
    [accepted, setAccepted] = useState<string[]>([]),
    [error, setError] = useState(''),
    [toast, setToast] = useState(''),
    [busy, setBusy] = useState(false),
    [filter, setFilter] = useState('all'),
    [search, setSearch] = useState(''),
    [apk, setApk] = useState<ApkInfo | null>(null),
    [maximized, setMaximized] = useState(false),
    [preview, setPreview] = useState('');
  const [exportPreview, setExportPreview] = useState<ExportPreview | null>(null);
  const requestExport = (request: ExportRequest) => {
    void guard(async () => setExportPreview(await call<ExportPreview>('export_prepare', request)));
  };
  const maxButton = useRef<HTMLButtonElement>(null);
  const [nativeHover, setNativeHover] = useState(false);
  const notify = (message: string) => {
    setToast(message);
    setTimeout(() => setToast(''), 5000);
  };
  const refresh = () => client.invalidateQueries({ queryKey: ['workspace'] });
  const guard = async <T,>(work: () => Promise<T>): Promise<T | undefined> => {
    setError('');
    try {
      return await work();
    } catch (e) {
      setError(errorText(e));
      return undefined;
    }
  };
  const prepare = async (
    kind: string,
    args: Record<string, string> = {},
    target: Target | null = null,
  ) => {
    setBusy(true);
    await guard(async () => {
      const p = await call<Plan>('plan_create', {
        operation: { kind, args, target } satisfies Operation,
      });
      setPlan(p);
      setAccepted([]);
    });
    setBusy(false);
  };
  const chooseApk = async (path?: string) => {
    await guard(async () => {
      const file = path ?? (await call<string | null>('file_choose', { kind: 'apk' }));
      if (file) {
        setApk(await call<ApkInfo>('apk_prepare', { path: file }));
        setModal('apk');
      }
    });
  };
  useEffect(() => {
    if (!native) return;
    let gone = false;
    const cleanups: Promise<() => void>[] = [];
    cleanups.push(listen('workspace-changed', () => refresh()));
    cleanups.push(listen<{ message: string }>('task-error', (event) => setError(event.payload.message)));
    cleanups.push(
      listen<boolean>('native-maximize-hover', (event) => setNativeHover(event.payload)),
    );
    cleanups.push(listen('close-requested', () => setModal('close')));
    cleanups.push(
      getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type !== 'drop') return;
        if (event.payload.paths.length !== 1) {
          setError('每次仅支持拖入一个完整 APK 文件');
          return;
        }
        const row = document
          .elementFromPoint(
            event.payload.position.x / devicePixelRatio,
            event.payload.position.y / devicePixelRatio,
          )
          ?.closest<HTMLElement>('[data-device-key]');
        if (row?.dataset.deviceKey) select(row.dataset.deviceKey);
        void chooseApk(event.payload.paths[0]);
      }),
    );
    return () => {
      gone = true;
      cleanups.forEach(
        (p) =>
          void p.then((f) => {
            if (gone) f();
          }),
      );
    };
  }, []);
  useEffect(() => {
    if (!native) return;
    const sync = () => {
      void call<{ maximized: boolean }>('window_state').then((v) => setMaximized(v.maximized));
      const r = maxButton.current?.getBoundingClientRect();
      if (r)
        void call('window_regions', {
          regions: {
            left: Math.round(r.left),
            top: Math.round(r.top),
            width: Math.round(r.width),
            height: Math.round(r.height),
            scale: devicePixelRatio,
          },
        });
    };
    sync();
    window.addEventListener('resize', sync);
    window.addEventListener('focus', sync);
    return () => {
      window.removeEventListener('resize', sync);
      window.removeEventListener('focus', sync);
    };
  }, []);
  const selectedDevice = data?.devices.find((d) => deviceKey(d) === selected);
  const selectedAvd = data?.avds.find((a) => a.id === selected);
  const hasSelection = Boolean(selectedDevice || selectedAvd);
  useEffect(() => {
    if (data && selected && !selectedDevice && !selectedAvd) select('');
  }, [data, selected, selectedDevice, selectedAvd, select]);
  const target =
    selectedDevice ??
    (selectedAvd && data?.avds.filter((a) => a.name === selectedAvd.name).length === 1
      ? data?.devices.find((d) => d.avdName === selectedAvd.name)
      : undefined);
  const extraSessions =
    data?.devices.filter(
      (d) => !d.avdName || data.avds.filter((a) => a.name === d.avdName).length !== 1,
    ) ?? [];
  const active = data?.tasks.filter((t) => taskActive(t.state)) ?? [];
  const avdAction = (kind: string, a: Avd, args: Record<string, string> = {}) =>
    prepare(kind, { avdId: a.id, ...args });
  const submit = async () => {
    if (!plan) return;
    setBusy(true);
    await guard(async () => {
      await call('task_submit', {
        planId: plan.id,
        planHash: plan.hash,
        requestId: crypto.randomUUID(),
        confirmationToken: plan.confirmationToken,
        acceptedLicenses: accepted,
      });
      setPlan(null);
      setModal(null);
      await refresh();
      notify('任务已提交，可在任务中心查看');
    });
    setBusy(false);
  };
  return (
    <div className="app-shell">
      <header className="titlebar">
        <div className="titlebar-brand">
          <img className="brand-mark" src="/android-devkit.png" alt="" aria-hidden="true" />
          Android DevKit
        </div>
        <div
          className="title-drag"
          onMouseDown={(e) => {
            if (e.button === 0 && e.detail !== 2) void call('window_action', { action: 'drag' });
          }}
          onDoubleClick={() => void call('window_action', { action: 'maximize' })}
        />
        <span className="title-env">
          本机环境 <span>·</span> {data?.environment.adb ? '已连接 SDK' : '环境准备'}
        </span>
        <div className="window-buttons">
          <button
            aria-label="最小化"
            onClick={() => void call('window_action', { action: 'minimize' })}
          >
            <Minus size={13} />
          </button>
          <button
            ref={maxButton}
            className={nativeHover ? 'native-hover' : ''}
            aria-label={maximized ? '还原' : '最大化'}
            onClick={() => void call('window_action', { action: 'maximize' })}
          >
            {maximized ? <Copy size={12} /> : <Square size={12} />}
          </button>
          <button
            aria-label="关闭窗口"
            className="window-close"
            onClick={() =>
              native ? void call('window_action', { action: 'close' }) : setModal('close')
            }
          >
            <X size={15} />
          </button>
        </div>
      </header>
      <aside className="sidebar">
        <div className="sidebar-heading">工作空间</div>
        <nav>
          {(
            [
              ['devices', Smartphone],
              ['sdk', Package],
              ['tasks', Activity],
            ] as const
          ).map(([id, Icon]) => (
            <button
              key={id}
              aria-label={titles[id]}
              className={page === id ? 'active' : ''}
              onClick={() => setPage(id)}
            >
              <Icon size={18} />
              <span>{titles[id]}</span>
              {id === 'tasks' && active.length > 0 && <b>{active.length}</b>}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <nav>
            {(
              [
                ['diagnostics', SlidersHorizontal],
                ['settings', SettingsIcon],
              ] as const
            ).map(([id, Icon]) => (
              <button
                key={id}
                aria-label={titles[id]}
                className={page === id ? 'active' : ''}
                onClick={() => setPage(id)}
              >
                <Icon size={18} />
                {titles[id]}
              </button>
            ))}
          </nav>
          <div className="sdk-card">
            <span className={`connection-dot ${data?.environment.adb ? 'online' : ''}`} />
            <div>
              <b>{data?.environment.adb ? 'SDK 已就位' : '准备开发环境'}</b>
              <small title={data?.settings.sdkRoot}>
                {data?.settings.sdkRoot ?? 'E:\Android\sdk'}
              </small>
            </div>
          </div>
          <small className="build-label">
            ANDROID DEVKIT <span>0.4.0</span>
          </small>
        </div>
      </aside>
      <main className="main">
        <div className="page-top">
          <div>
            <div className="eyebrow">ANDROID DEVELOPMENT ENVIRONMENT</div>
            <h1>{titles[page]}</h1>
            <p>
              {page === 'devices'
                ? '准备好设备，让下一次验证更直接。'
                : page === 'sdk'
                  ? '按需准备组件，复用已有开发环境。'
                  : page === 'tasks'
                    ? '每个操作的目标、阶段和结果都留在这里。'
                    : page === 'diagnostics'
                      ? '从检测到的事实出发，定位环境与连接问题。'
                      : '为当前工作空间选择路径和行为。'}
            </p>
          </div>
          <div className="page-actions">
            {page === 'devices' && (
              <>
                <Button onClick={() => setModal('wireless')}>
                  <Wifi size={15} />
                  连接设备
                </Button>
                <Button primary onClick={() => setModal('create')}>
                  <Plus size={16} />
                  新建模拟器
                </Button>
              </>
            )}
            {page === 'sdk' && (
              <Button
                onClick={() => {
                  setBusy(true);
                  void guard(async () => {
                    const warnings = await call<string[]>('catalog_refresh');
                    notify(
                      warnings.length ? '部分仓库不可用，其余目录已更新' : '官方仓库目录已更新',
                    );
                    await refresh();
                  }).finally(() => setBusy(false));
                }}
                disabled={busy}
              >
                <RefreshCw size={15} className={busy ? 'spin' : ''} />
                刷新仓库
              </Button>
            )}
          </div>
        </div>
        {!native && (
          <div className="notice">
            <Monitor size={19} />
            <div>
              <b>桌面工作台预览</b>
              <p>
                运行 pnpm tauri dev 后，即可读取 E:\Android\sdk
                并连接本机设备。浏览器预览不展示虚构设备。
              </p>
            </div>
          </div>
        )}
        {(error || query.error) && (
          <div role="alert" className="error-banner">
            <AlertCircle size={17} />
            <span>{error || errorText(query.error)}</span>
            <button aria-label="关闭错误" onClick={() => setError('')}>
              <X size={15} />
            </button>
          </div>
        )}
        {query.isLoading && native && (
          <div className="loading">
            <RefreshCw className="spin" size={20} />
            正在检查 SDK、AVD 与共享 ADB 服务…
          </div>
        )}
        {page === 'devices' && (
          <>
            <div className="section-toolbar">
              <div className="tabs">
                {[
                  ['all', '全部'],
                  ['usb', '真机'],
                  ['emulator', '模拟器'],
                ].map(([id, label]) => (
                  <button
                    className={filter === id ? 'selected' : ''}
                    key={id}
                    onClick={() => setFilter(id)}
                  >
                    {label}
                    <span>
                      {id === 'all'
                        ? (data?.avds.length ?? 0) + extraSessions.length
                        : id === 'emulator'
                          ? (data?.avds.length ?? 0)
                          : (data?.devices.filter((d) => d.connection !== 'emulator').length ?? 0)}
                    </span>
                  </button>
                ))}
              </div>
              <button className="text-button" onClick={() => void refresh()}>
                <RefreshCw size={14} />
                重新扫描
              </button>
            </div>
            <div className={`device-layout ${hasSelection ? 'has-selection' : ''}`}>
              <div className="device-list">
                {extraSessions
                  .filter(
                    (d) =>
                      filter === 'all' ||
                      (filter === 'emulator'
                        ? d.connection === 'emulator'
                        : d.connection !== 'emulator'),
                  )
                  .map((d) => (
                    <div
                      key={deviceKey(d)}
                      tabIndex={0}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') select(deviceKey(d));
                      }}
                      onClick={() => select(deviceKey(d))}
                      data-device-key={deviceKey(d)}
                      className={`device-row ${selected === deviceKey(d) ? 'selected' : ''}`}
                    >
                      <div className="device-icon">
                        <Smartphone size={26} />
                      </div>
                      <div className="device-main">
                        <strong>
                          {d.connection === 'emulator'
                            ? `待关联 · ${d.avdName || d.model}`
                            : d.model}
                        </strong>
                        <p>
                          {d.connection === 'wireless'
                            ? '无线'
                            : d.connection === 'emulator'
                              ? '模拟器会话'
                              : 'USB'}{' '}
                          <span>·</span> {d.api ? `API ${d.api}` : d.serial}
                          <span>·</span>
                          {d.serial.slice(-12)}
                        </p>
                      </div>
                      <Badge state={d.state} />
                      <button
                        className="row-button"
                        disabled={d.state !== 'ready'}
                        onClick={(e) => {
                          e.stopPropagation();
                          select(deviceKey(d));
                          void chooseApk();
                        }}
                      >
                        安装 APK
                        <ChevronRight size={14} />
                      </button>
                    </div>
                  ))}
                {data?.avds
                  .filter(() => filter !== 'usb')
                  .map((a) => (
                    <div
                      key={a.id}
                      tabIndex={0}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') select(a.id);
                      }}
                      onClick={() => select(a.id)}
                      data-device-key={a.id}
                      className={`device-row ${selected === a.id ? 'selected' : ''}`}
                    >
                      <div className="device-icon emulator">
                        <Monitor size={26} />
                      </div>
                      <div className="device-main">
                        <strong>{a.displayName}</strong>
                        <p>
                          模拟器 <span>·</span> {a.api ? `API ${a.api}` : a.name}
                          <span>·</span>
                          {a.abi || '未知架构'}
                        </p>
                        {a.error && <small className="warning-text">{a.error}</small>}
                      </div>
                      <Badge state={a.status} />
                      <button
                        className="row-button"
                        disabled={!['stopped', 'ready'].includes(a.status)}
                        onClick={(e) => {
                          e.stopPropagation();
                          if (a.status === 'ready') {
                            select(a.id);
                            void chooseApk();
                          } else void avdAction('avd_start', a);
                        }}
                      >
                        {a.status === 'ready' ? '安装 APK' : '启动'}
                        {a.status === 'ready' ? <ChevronRight size={14} /> : <Play size={14} />}
                      </button>
                    </div>
                  ))}
                {(!data || (data.avds.length === 0 && data.devices.length === 0)) && (
                  <div className="empty-state">
                    <div className="empty-icon">
                      <Smartphone size={35} />
                    </div>
                    <h3>为下一次验证准备设备</h3>
                    <p>
                      连接开启 USB 调试的手机，或创建一台模拟器。
                      <br />
                      已有 SDK 和 AVD 会在扫描后自动显示。
                    </p>
                    <div>
                      <Button onClick={() => void prepare('adb_start')}>启动设备服务</Button>
                      <Button onClick={() => setModal('create')}>
                        <Plus size={15} />
                        创建模拟器
                      </Button>
                    </div>
                  </div>
                )}
                <div className="add-device-hint">
                  <span>拖入单个 APK 安装到所选设备</span>
                  <button className="text-button" onClick={() => void chooseApk()}>
                    <FileUp size={14} />
                    选择 APK
                  </button>
                </div>
              </div>
              {hasSelection && (
                <aside className="detail-panel">
                  <div className="detail-heading">
                    <h3>{selectedAvd?.displayName ?? selectedDevice?.model ?? '原目标已断开'}</h3>
                    <button
                      className="icon-button"
                      aria-label="关闭设备详情"
                      onClick={() => select('')}
                    >
                      <X size={16} />
                    </button>
                  </div>
                  <Badge state={selectedAvd?.status ?? selectedDevice?.state ?? 'offline'} />
                  <dl>
                    <dt>连接 / 标识</dt>
                    <dd>{target?.serial ?? selectedAvd?.name}</dd>
                    <dt>系统 / 架构</dt>
                    <dd>
                      API {target?.api ?? selectedAvd?.api ?? '未知'} ·{' '}
                      {target?.abi ?? selectedAvd?.abi ?? '未知'}
                    </dd>
                    {selectedAvd && (
                      <>
                        <dt>数据目录</dt>
                        <dd className="path">{selectedAvd.dataPath}</dd>
                        <dt>系统镜像</dt>
                        <dd className="path">{selectedAvd.image}</dd>
                        <dt>资源配置</dt>
                        <dd>
                          {selectedAvd.memory} MiB 内存 · {selectedAvd.disk}
                        </dd>
                      </>
                    )}
                  </dl>
                  {selectedDevice?.state === 'unauthorized' && (
                    <p className="hint">请解锁手机并允许此电脑进行 USB 调试；授权后将自动更新。</p>
                  )}
                  <div className="detail-actions">
                    <Button
                      primary
                      disabled={target?.state !== 'ready'}
                      onClick={() => void chooseApk()}
                    >
                      <Download size={15} />
                      安装 APK
                    </Button>
                    <Button
                      disabled={target?.state !== 'ready'}
                      onClick={() => void prepare('capture', {}, target ?? null)}
                    >
                      <Camera size={15} />
                      截图
                    </Button>
                    {target && (
                      <Button
                        onClick={() => {
                          setPage('devices');
                          document.getElementById('logs')?.scrollIntoView({ behavior: 'smooth' });
                        }}
                      >
                        <Terminal size={15} />
                        查看日志
                      </Button>
                    )}
                    {selectedAvd && (
                      <>
                        <Button
                          disabled={selectedAvd.status !== 'stopped'}
                          onClick={() => void avdAction('avd_cold', selectedAvd)}
                        >
                          冷启动
                        </Button>
                        <Button
                          onClick={() => void avdAction('avd_stop', selectedAvd)}
                          disabled={!['ready', 'booting', 'busy'].includes(selectedAvd.status)}
                        >
                          <Square size={13} />
                          正常停止
                        </Button>
                        <Button
                          disabled={selectedAvd.status !== 'stopped'}
                          onClick={() => setModal('edit')}
                        >
                          编辑配置
                        </Button>
                        <Button onClick={() => void avdAction('avd_focus', selectedAvd)}>
                          打开窗口
                        </Button>
                        <Button
                          danger
                          onClick={() => void avdAction('avd_force_stop', selectedAvd)}
                        >
                          强制停止
                        </Button>
                        <Button
                          danger
                          disabled={selectedAvd.status !== 'stopped'}
                          onClick={() => void avdAction('avd_wipe', selectedAvd)}
                        >
                          清除数据
                        </Button>
                        <Button
                          danger
                          disabled={selectedAvd.status !== 'stopped'}
                          onClick={() => void avdAction('avd_delete', selectedAvd)}
                        >
                          <Trash2 size={14} />
                          删除 AVD
                        </Button>
                      </>
                    )}
                    {selectedDevice?.connection === 'wireless' && (
                      <Button
                        onClick={() =>
                          void prepare('wireless_disconnect', { address: selectedDevice.serial })
                        }
                      >
                        断开无线连接
                      </Button>
                    )}
                  </div>
                </aside>
              )}
            </div>
            {target && (
              <LogPanel
                key={deviceKey(target)}
                target={target}
                guard={guard}
                prepare={(kind, args) => prepare(kind, args, target)}
                onExport={requestExport}
              />
            )}
            <div className="environment-note">
              <HardDrive size={17} />
              <div>
                <b>{data?.server ?? '等待桌面环境'}</b>
                <span>SDK 与 AVD 独立管理，退出工作台不会自动结束模拟器。</span>
              </div>
              <button className="text-button" onClick={() => setPage('diagnostics')}>
                查看诊断
                <ChevronRight size={14} />
              </button>
            </div>
          </>
        )}
        {page === 'sdk' && (
          <SdkPanel
            data={data}
            prepare={prepare}
            busy={busy}
            setModal={setModal}
            search={search}
            setSearch={setSearch}
          />
        )}
        {page === 'tasks' && (
          <div className="task-list">
            <ArtifactsPanel guard={guard} onExport={setExportPreview} />
            {data?.tasks.length ? (
              data.tasks.map((t) => (
                <TaskCard
                  key={t.id}
                  task={t}
                  guard={guard}
                  onExport={requestExport}
                  act={async (command, args) => {
                    await guard(() => call(command, args));
                    await refresh();
                  }}
                  retry={async () => {
                    await guard(async () => {
                      const p = await call<Plan>('task_retry', { taskId: t.id });
                      setPlan(p);
                      setAccepted([]);
                    });
                  }}
                  launch={() =>
                    void prepare(
                      'app_launch',
                      { package: t.operation.args.package ?? '' },
                      t.operation.target,
                    )
                  }
                  preview={async () => {
                    await guard(async () =>
                      setPreview(
                        await call<string>('artifact_copy', { taskId: t.id, preview: true }),
                      ),
                    );
                  }}
                />
              ))
            ) : (
              <div className="empty-state">
                <Activity size={32} />
                <h3>任务会留在这里</h3>
                <p>安装、下载、创建和导出的结果均可回看。</p>
              </div>
            )}
          </div>
        )}
        {page === 'settings' && (
          <SettingsPanel
            initial={data?.settings}
            guard={guard}
            done={() => {
              void refresh();
              notify('设置已保存');
            }}
          />
        )}
        {page === 'diagnostics' && (
          <Diagnostics guard={guard} prepare={prepare} onExport={requestExport} />
        )}
      </main>
      <footer className="statusbar">
        <span className="status-dot" />
        <span>
          {active.length ? `${active.length} 个任务进行中 · ${active[0].message}` : '工作空间就绪'}
        </span>
        <button onClick={() => setPage('tasks')}>
          {active.length ? '展开任务中心' : '查看任务记录'}
          <ChevronRight size={12} />
        </button>
        <span className="status-end">Windows · 本地环境</span>
      </footer>
      {toast && (
        <div className="toast" role="status">
          <Check size={17} />
          {toast}
        </div>
      )}
      {modal === 'create' && (
        <CreateAvd data={data} close={() => setModal(null)} prepare={prepare} />
      )}
      {modal === 'wireless' && <Wireless close={() => setModal(null)} prepare={prepare} />}
      {modal === 'edit' && selectedAvd && (
        <EditAvd
          avd={selectedAvd}
          close={() => setModal(null)}
          save={(args) => void avdAction('avd_edit', selectedAvd, args)}
        />
      )}
      {modal === 'apk' && apk && (
        <Modal title="安装 APK" onClose={() => setModal(null)}>
          <div className="file-summary">
            <Package size={28} />
            <div>
              <b>{apk.fileName}</b>
              <small>
                {size(apk.size)} · {apk.package || '包名暂无法读取'} · {apk.version || '版本未知'}
              </small>
            </div>
          </div>
          <Field label="目标设备">
            <select
              value={target ? deviceKey(target) : ''}
              onChange={(e) => select(e.target.value)}
            >
              <option value="">选择明确目标</option>
              {data?.devices
                .filter((d) => d.state === 'ready')
                .map((d) => (
                  <option key={deviceKey(d)} value={deviceKey(d)}>
                    {d.model} · {d.serial}
                  </option>
                ))}
            </select>
          </Field>
          <p className="hint">
            最低 API {apk.minSdk || '未知'} · ABI {apk.abis.join(', ') || '无指定 native ABI'}
            。默认覆盖安装并保留数据。
          </p>
          {apk.warning && <p className="warning-text">{apk.warning}</p>}
          <div className="modal-actions">
            <Button onClick={() => setModal(null)}>取消</Button>
            <Button
              primary
              disabled={!target || target.state !== 'ready'}
              onClick={() => void prepare('apk_install', { apkId: apk.id }, target ?? null)}
            >
              确认安装计划
            </Button>
          </div>
        </Modal>
      )}
      {modal === 'close' && <CloseDialog guard={guard} close={() => setModal(null)} />}
      {exportPreview && (
        <ExportDialog
          key={exportPreview.artifact.id}
          value={exportPreview}
          setValue={setExportPreview}
          guard={guard}
          close={() => setExportPreview(null)}
        />
      )}
      {plan && (
        <Modal title={plan.title} onClose={() => setPlan(null)} wide>
          <div className="plan-summary">
            {plan.effects.map((effect, i) => (
              <p key={i}>
                <Check size={14} />
                {effect}
              </p>
            ))}
          </div>
          {plan.warnings.map((w, i) => (
            <p className="plan-warning" key={i}>
              <AlertCircle size={16} />
              {w}
            </p>
          ))}
          {plan.licenses.map((l) => (
            <div className="license" key={l.hash}>
              <b>{l.id}</b>
              <pre>{l.text}</pre>
              <label>
                <input
                  type="checkbox"
                  checked={accepted.includes(l.hash)}
                  onChange={(e) =>
                    setAccepted((v) =>
                      e.target.checked ? [...v, l.hash] : v.filter((h) => h !== l.hash),
                    )
                  }
                />
                我已查看并接受此许可
              </label>
            </div>
          ))}
          <div className="modal-actions">
            <Button onClick={() => setPlan(null)}>返回修改</Button>
            <Button
              primary
              danger={
                plan.operation.kind.includes('delete') || plan.operation.kind.includes('wipe')
              }
              disabled={busy || plan.licenses.some((l) => !accepted.includes(l.hash))}
              onClick={() => void submit()}
            >
              {busy ? '正在提交…' : plan.title}
            </Button>
          </div>
        </Modal>
      )}
      {preview && (
        <Modal title="设备截图" onClose={() => setPreview('')} wide>
          <img className="screenshot" src={preview} alt="设备截图预览" />
        </Modal>
      )}
    </div>
  );
}

function SdkPanel({
  data,
  prepare,
  busy,
  search,
  setSearch,
}: {
  data?: Workspace;
  prepare: (kind: string, args?: Record<string, string>) => Promise<void>;
  busy: boolean;
  setModal: (m: ModalName) => void;
  search: string;
  setSearch: (v: string) => void;
}) {
  const [tab, setTab] = useState('all'),
    [selected, setSelected] = useState<Record<string, string>>({});
  const [allVersions, setAllVersions] = useState(false),
    [limit, setLimit] = useState(150);
  const newestApi = Math.max(
    0,
    ...(data?.packages
      .filter((p) => p.category === 'platform' && !p.obsolete)
      .map((p) => Number(p.api) || 0) ?? []),
  );
  const packages =
    data?.packages.filter(
      (p) =>
        (tab === 'all' || p.category === tab) &&
        !p.obsolete &&
        (allVersions ||
          search.trim() !== '' ||
          p.installed ||
          ['platform-tools', 'emulator', 'cmdline-tools;22.0'].includes(p.id) ||
          (Number(p.api) >= newestApi - 1 && p.category === 'platform') ||
          (p.category === 'image' && p.abi === 'x86_64' && Number(p.api) >= newestApi - 1)) &&
        `${p.name} ${p.id} ${p.api}`.toLowerCase().includes(search.toLowerCase()),
    ) ?? [];
  packages.sort(
    (a, b) =>
      Number(b.installed) - Number(a.installed) ||
      b.id.localeCompare(a.id, undefined, { numeric: true }),
  );
  return (
    <>
      <div className="sdk-overview">
        <div>
          <HardDrive size={22} />
          <div>
            <b>{data?.settings.sdkRoot ?? 'E:\Android\sdk'}</b>
            <small>
              {data?.packages.filter((p) => p.installed).length ?? 0} 个已安装组件 · 可用空间{' '}
              {data?.freeBytes ? `${(data.freeBytes / 1024 ** 3).toFixed(1)} GiB` : '读取中'}
            </small>
          </div>
        </div>
        <Button onClick={() => void prepare('bootstrap', { purpose: 'usb' })}>准备真机环境</Button>
        <Button onClick={() => void prepare('bootstrap', { purpose: 'emulator' })}>
          准备模拟器环境
        </Button>
      </div>
      <div className="section-toolbar">
        <div className="tabs">
          {[
            ['all', '全部'],
            ['platform', '平台'],
            ['tool', '工具'],
            ['image', '系统镜像'],
          ].map(([id, label]) => (
            <button key={id} className={tab === id ? 'selected' : ''} onClick={() => setTab(id)}>
              {label}
            </button>
          ))}
        </div>
        <label className="search">
          <Search size={15} />
          <input
            aria-label="搜索组件"
            placeholder="搜索组件、API 或名称"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
      </div>
      <div className="catalog-filter">
        <span>稳定通道 · 已安装与近期稳定组件；按用途选择，不会自动升级</span>
        <label>
          <input
            type="checkbox"
            checked={allVersions}
            onChange={(e) => setAllVersions(e.target.checked)}
          />
          查看全部稳定组件
        </label>
      </div>
      <div className="package-table">
        <div className="package-header">
          <span>组件</span>
          <span>版本</span>
          <span>状态</span>
          <span>变更</span>
        </div>
        {packages.slice(0, limit).map((p) => (
          <div className="package-row" key={p.id}>
            <div>
              <b>{p.name || p.id}</b>
              <small>
                {p.id} {p.abi && `· ${p.abi}`}
              </small>
            </div>
            <span>{p.installedVersion || p.version}</span>
            <span className={p.installed ? 'installed' : 'muted'}>
              {p.installed
                ? p.installedVersion && p.installedVersion !== p.version
                  ? '有可选更新'
                  : '已安装'
                : size(p.size)}
            </span>
            <select
              aria-label={`${p.id} 变更`}
              value={selected[p.id] ?? ''}
              onChange={(e) => setSelected((s) => ({ ...s, [p.id]: e.target.value }))}
            >
              <option value="">{p.installed ? '保留' : '不安装'}</option>
              {p.installed && p.installedVersion && p.installedVersion !== p.version && (
                <option value="install">更新至 {p.version}</option>
              )}
              <option value={p.installed ? 'remove' : 'install'}>
                {p.installed ? '卸载' : '安装'}
              </option>
            </select>
          </div>
        ))}
        {packages.length === 0 && (
          <div className="empty-state">
            <Package size={30} />
            <h3>按需准备组件</h3>
            <p>刷新官方仓库目录后，选择要安装的工具、平台或镜像。</p>
          </div>
        )}
      </div>
      {packages.length > limit && (
        <Button onClick={() => setLimit((n) => n + 150)}>
          再显示 150 项（共 {packages.length} 项）
        </Button>
      )}
      {Object.values(selected).some(Boolean) && (
        <div className="change-bar">
          <div>
            <b>待执行变更</b>
            <small>确认后才会开始；已完成项保留</small>
          </div>
          <Button
            disabled={busy}
            onClick={() => {
              const ids = Object.keys(selected).filter((k) => selected[k] === 'remove');
              if (ids.length) void prepare('sdk_remove', { packages: ids.join('\n') });
            }}
          >
            确认卸载 {Object.values(selected).filter((v) => v === 'remove').length} 项
          </Button>
          <Button
            primary
            disabled={busy}
            onClick={() => {
              const ids = Object.keys(selected).filter((k) => selected[k] === 'install');
              if (ids.length) void prepare('sdk_install', { packages: ids.join('\n') });
            }}
          >
            确认安装 {Object.values(selected).filter((v) => v === 'install').length} 项
          </Button>
        </div>
      )}
    </>
  );
}

function CreateAvd({
  data,
  close,
  prepare,
}: {
  data?: Workspace;
  close: () => void;
  prepare: (k: string, a: Record<string, string>) => Promise<void>;
}) {
  const [step, setStep] = useState(1),
    [form, setForm] = useState({
      name: 'DevKit_Phone',
      displayName: '日常测试手机',
      profile: 'pixel_2',
      image:
        data?.packages.find((p) => p.category === 'image' && p.installed && p.abi === 'x86_64')
          ?.id ?? '',
      memory: '2048',
      storage: '8',
      gpu: 'auto',
    });
  const set = (key: string, v: string) => setForm((f) => ({ ...f, [key]: v }));
  const images =
    data?.packages.filter(
      (p) => p.category === 'image' && (p.abi === 'x86_64' || p.id.endsWith(';x86_64')),
    ) ?? [];
  return (
    <Modal title="新建模拟器" onClose={close} wide>
      <div className="wizard-steps">
        {['设备形态', '系统镜像', '确认配置'].map((label, i) => (
          <span className={step === i + 1 ? 'current' : ''} key={label}>
            <b>{i + 1}</b>
            {label}
          </span>
        ))}
      </div>
      {step === 1 ? (
        <>
          <h3>选择适合验证的设备</h3>
          <div className="device-choices">
            <button
              className={form.profile === 'pixel_2' ? 'selected' : ''}
              onClick={() => {
                set('profile', 'pixel_2');
                set('displayName', '日常测试手机');
              }}
            >
              <Smartphone size={36} />
              <b>手机</b>
              <small>Pixel 2 · 常用触屏设备</small>
            </button>
            <button
              className={form.profile === 'Nexus 9' ? 'selected' : ''}
              onClick={() => {
                set('profile', 'Nexus 9');
                set('displayName', '平板适配测试');
              }}
            >
              <Monitor size={38} />
              <b>平板</b>
              <small>Nexus 9 · 大屏适配</small>
            </button>
          </div>
          <p className="hint">使用官方硬件 profile，内存与图形参数可在下一步调整。</p>
        </>
      ) : step === 2 ? (
        <>
          <h3>选择 Android 系统</h3>
          <p className="hint">首发 Windows x64 优先选择 x86_64 镜像。缺失镜像会加入创建任务。</p>
          <div className="image-options">
            {images.map((p) => (
              <label key={p.id} className={form.image === p.id ? 'selected' : ''}>
                <input
                  type="radio"
                  name="image"
                  checked={form.image === p.id}
                  onChange={() => set('image', p.id)}
                />
                <div>
                  <b>{p.name || p.id}</b>
                  <small>
                    {p.id} ·{' '}
                    {p.installed
                      ? p.installedVersion && p.installedVersion !== p.version
                        ? '有可选更新'
                        : '已安装'
                      : size(p.size)}
                  </small>
                </div>
              </label>
            ))}
            {!images.length && <p>尚无镜像目录，请先关闭向导并在 SDK 页面刷新官方仓库。</p>}
          </div>
        </>
      ) : (
        <>
          <div className="form-grid">
            <Field label="内部名称">
              <input value={form.name} onChange={(e) => set('name', e.target.value)} />
            </Field>
            <Field label="显示名称">
              <input
                value={form.displayName}
                onChange={(e) => set('displayName', e.target.value)}
              />
            </Field>
            <Field label="内存（MiB）">
              <input
                type="number"
                min={512}
                max={16384}
                value={form.memory}
                onChange={(e) => set('memory', e.target.value)}
              />
            </Field>
            <Field label="数据分区（GiB）">
              <input
                type="number"
                min={2}
                max={128}
                value={form.storage}
                onChange={(e) => set('storage', e.target.value)}
              />
            </Field>
            <Field label="图形模式">
              <select value={form.gpu} onChange={(e) => set('gpu', e.target.value)}>
                <option value="auto">自动</option>
                <option value="host">主机 GPU</option>
                <option value="software">软件渲染</option>
              </select>
            </Field>
          </div>
          <p className="hint">
            数据目录：{data?.settings.avdRoots.at(-1)}
            <br />
            系统镜像：{form.image}
          </p>
        </>
      )}
      <div className="modal-actions">
        <Button onClick={step === 1 ? close : () => setStep((s) => s - 1)}>
          {step === 1 ? '取消' : '上一步'}
        </Button>
        {step < 3 ? (
          <Button
            primary
            disabled={step === 2 && !form.image}
            onClick={() => setStep((s) => s + 1)}
          >
            下一步
            <ChevronRight size={14} />
          </Button>
        ) : (
          <Button primary onClick={() => void prepare('avd_create', form)}>
            查看创建计划
          </Button>
        )}
      </div>
    </Modal>
  );
}
function Wireless({
  close,
  prepare,
}: {
  close: () => void;
  prepare: (k: string, a: Record<string, string>) => Promise<void>;
}) {
  const [mode, setMode] = useState('pair'),
    [host, setHost] = useState(''),
    [port, setPort] = useState(''),
    [code, setCode] = useState('');
  return (
    <Modal title="无线连接设备" onClose={close}>
      <div className="tabs">
        <button className={mode === 'pair' ? 'selected' : ''} onClick={() => setMode('pair')}>
          1. 配对码
        </button>
        <button className={mode === 'connect' ? 'selected' : ''} onClick={() => setMode('connect')}>
          2. 连接设备
        </button>
      </div>
      <p className="hint">手机开启开发者选项 → 无线调试。电脑和手机应在可互通的同一网络。</p>
      <Field label="设备 IP 地址">
        <input placeholder="192.168.1.10" value={host} onChange={(e) => setHost(e.target.value)} />
      </Field>
      <Field
        label={mode === 'pair' ? '配对端口' : '连接端口'}
        hint="配对端口与连接端口通常不同，请以手机当前页面为准。"
      >
        <input inputMode="numeric" value={port} onChange={(e) => setPort(e.target.value)} />
      </Field>
      {mode === 'pair' && (
        <Field label="6 位配对码">
          <input
            autoComplete="off"
            maxLength={6}
            inputMode="numeric"
            value={code}
            onChange={(e) => setCode(e.target.value)}
          />
        </Field>
      )}
      <p className="hint">配对成功不代表已连接。取消配对需在手机的无线调试设置中选择“忘记”。</p>
      <div className="modal-actions">
        <Button onClick={close}>取消</Button>
        <Button
          primary
          onClick={() =>
            void prepare(mode === 'pair' ? 'wireless_pair' : 'wireless_connect', {
              address: `${host}:${port}`,
              ...(mode === 'pair' ? { code } : {}),
            })
          }
        >
          {mode === 'pair' ? '配对' : '连接并验证'}
        </Button>
      </div>
    </Modal>
  );
}
function EditAvd({
  avd,
  close,
  save,
}: {
  avd: Avd;
  close: () => void;
  save: (a: Record<string, string>) => void;
}) {
  const [name, setName] = useState(avd.displayName),
    [memory, setMemory] = useState(avd.memory),
    [gpu, setGpu] = useState(avd.gpu || 'auto');
  return (
    <Modal title="编辑模拟器配置" onClose={close}>
      <Field label="显示名称">
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label="内存（MiB）">
        <input value={memory} onChange={(e) => setMemory(e.target.value)} />
      </Field>
      <Field label="图形模式">
        <select value={gpu} onChange={(e) => setGpu(e.target.value)}>
          {['auto', 'host', 'software'].map((g) => (
            <option key={g}>{g}</option>
          ))}
        </select>
      </Field>
      <p className="hint">内部名称、镜像、未知配置键和数据目录保持原样。外部修改会阻止覆盖。</p>
      <div className="modal-actions">
        <Button onClick={close}>取消</Button>
        <Button
          primary
          onClick={() => save({ displayName: name, memory, gpu, fingerprint: avd.fingerprint })}
        >
          查看保存计划
        </Button>
      </div>
    </Modal>
  );
}
function TaskCard({
  guard,
  onExport,
  task,
  act,
  retry,
  preview,
  launch,
}: {
  guard: <T>(fn: () => Promise<T>) => Promise<T | undefined>;
  onExport: (request: ExportRequest) => void;
  task: Task;
  act: (c: string, a: Record<string, unknown>) => Promise<void>;
  retry: () => void;
  preview: () => void;
  launch: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <article className="task-card">
      <div className="task-top">
        <div className="task-symbol">
          <Activity size={19} />
        </div>
        <div className="task-title">
          <strong>{task.title}</strong>
          <small>
            {new Date(task.createdAt * 1000).toLocaleString()} ·{' '}
            {task.operation.target?.serial ??
              task.operation.args.avdId?.split('\\').at(-1) ??
              '本机环境'}
          </small>
        </div>
        <Badge state={task.state} />
      </div>
      <p>{task.message}</p>
      {task.progress !== null && taskActive(task.state) && (
        <progress value={task.progress} max={100} />
      )}
      <div className="task-actions">
        {task.state === 'succeeded' &&
          ['apk_install', 'app_reinstall'].includes(task.operation.kind) &&
          task.operation.args.package && (
            <Button onClick={launch}>
              <Play size={13} />
              启动应用
            </Button>
          )}
        {taskActive(task.state) && (
          <Button onClick={() => void act('task_cancel', { taskId: task.id })}>
            {task.cancellable ? '取消' : '此步骤完成后取消'}
          </Button>
        )}
        {['failed', 'needs_review', 'cancelled'].includes(task.state) && (
          <Button onClick={retry}>重新检查并重试</Button>
        )}
        {task.artifact && task.operation.kind === 'capture' && (
          <>
            <Button onClick={preview}>预览截图</Button>
            <Button onClick={() => void act('artifact_save', { taskId: task.id })}>保存</Button>
            <Button onClick={() => void act('artifact_copy', { taskId: task.id, preview: false })}>
              <Copy size={13} />
              复制
            </Button>
          </>
        )}
        <button className="text-button" onClick={() => setExpanded((v) => !v)}>
          {expanded ? '收起诊断' : '查看诊断'}
        </button>
      </div>
      {task.operation.kind === 'log_record' && !taskActive(task.state) && task.artifact && (
        <RecordingExport taskId={task.id} guard={guard} onExport={onExport} />
      )}
      {expanded && <pre className="task-log">{task.log.join('\n') || '暂无额外诊断输出'}</pre>}
      {task.licensePrompt && (
        <div className="license">
          <b>官方工具正在等待许可确认</b>
          <pre>{task.licensePrompt}</pre>
          <Button onClick={() => void act('license_respond', { taskId: task.id, accept: false })}>
            拒绝
          </Button>
          <Button
            primary
            onClick={() => void act('license_respond', { taskId: task.id, accept: true })}
          >
            接受当前许可
          </Button>
        </div>
      )}
    </article>
  );
}
function SettingsPanel({
  initial,
  guard,
  done,
}: {
  initial?: Settings;
  guard: <T>(f: () => Promise<T>) => Promise<T | undefined>;
  done: () => void;
}) {
  const [form, setForm] = useState<Settings>(
    initial ?? {
      sdkRoot: 'E:\\Android\\sdk',
      avdRoots: [],
      javaPath: '',
      proxy: '',
      bootTimeoutSecs: 180,
      trayEnabled: false,
    },
  );
  useEffect(() => {
    if (initial) setForm(initial);
  }, [initial?.sdkRoot]);
  const set = (key: keyof Settings, value: string | number | string[] | boolean) =>
    setForm((s) => ({ ...s, [key]: value }));
  const choose = async (key: 'sdkRoot' | 'javaPath') => {
    const p = await call<string | null>('file_choose', {
      kind: key === 'sdkRoot' ? 'sdk' : 'java',
    });
    if (p) set(key, p);
  };
  return (
    <div className="settings-panel">
      <section>
        <h3>工具与数据路径</h3>
        <p className="hint">
          只影响本应用子进程，不修改系统 PATH、JAVA_HOME 或 Android Studio 配置。
        </p>
        <Field label="当前 SDK 根目录">
          <div className="input-action">
            <input value={form.sdkRoot} onChange={(e) => set('sdkRoot', e.target.value)} />
            <Button onClick={() => void guard(() => choose('sdkRoot'))}>选择目录</Button>
          </div>
        </Field>
        <Field label="Java 运行时（java.exe）">
          <div className="input-action">
            <input value={form.javaPath} onChange={(e) => set('javaPath', e.target.value)} />
            <Button onClick={() => void guard(() => choose('javaPath'))}>选择</Button>
          </div>
        </Field>
        <Field label="AVD 根目录（一行一个，最后一个用于新建）">
          <textarea
            rows={3}
            value={form.avdRoots.join('\n')}
            onChange={(e) => set('avdRoots', e.target.value.split('\n'))}
          />
        </Field>
      </section>
      <section>
        <h3>窗口与退出</h3>
        <label className="check-row">
          <input
            type="checkbox"
            checked={form.trayEnabled}
            onChange={(e) => set('trayEnabled', e.target.checked)}
          />
          启用托盘：关闭窗口后隐藏并继续任务
        </label>
        <p className="hint">
          默认关闭。启用后，可通过托盘打开工作台或选择退出；退出仍检查任务、文件采集与未保存产物。
        </p>
        <h3>网络与启动</h3>
        <Field
          label="手动代理"
          hint="示例 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080；留空直连。"
        >
          <input value={form.proxy} onChange={(e) => set('proxy', e.target.value)} />
        </Field>
        <Field label="模拟器启动提醒阈值（秒）">
          <input
            type="number"
            min={30}
            max={900}
            value={form.bootTimeoutSecs}
            onChange={(e) => set('bootTimeoutSecs', Number(e.target.value))}
          />
        </Field>
      </section>
      <Button
        primary
        onClick={() =>
          void guard(async () => {
            await call('settings_save', { settings: form });
            done();
          })
        }
      >
        保存设置
      </Button>
    </div>
  );
}
function Diagnostics({
  onExport,
  guard,
  prepare,
}: {
  guard: <T>(f: () => Promise<T>) => Promise<T | undefined>;
  onExport: (request: ExportRequest) => void;
  prepare: (k: string, a?: Record<string, string>) => Promise<void>;
}) {
  const [raw, setRaw] = useState(false);
  const [report, setReport] = useState<Record<string, unknown> | null>(null);
  useEffect(() => {
    void guard(async () => setReport(await call('diagnostics')));
  }, []);
  return (
    <div className="diagnostic-panel">
      <div className="notice">
        <SlidersHorizontal size={22} />
        <div>
          <b>检查实际工具链与连接事实</b>
          <p>不会自动重启共享 ADB、修改系统虚拟化功能或清除设备数据。</p>
        </div>
      </div>
      <div className="detail-actions">
        <Button onClick={() => void guard(async () => setReport(await call('diagnostics')))}>
          <RefreshCw size={14} />
          重新检测
        </Button>
        <label>
          <input type="checkbox" checked={raw} onChange={(e) => setRaw(e.target.checked)} />
          包含原始标识
        </label>
        <Button onClick={() => onExport({ kind: 'diagnostics', redacted: !raw })}>
          预览诊断导出
        </Button>
        <Button onClick={() => void prepare('adb_start')}>启动 ADB</Button>
        <Button danger onClick={() => void prepare('adb_restart')}>
          重启共享 ADB
        </Button>
      </div>
      {report &&
        Object.entries(report).map(([key, value]) => (
          <section className="diagnostic-item" key={key}>
            <h3>
              {(
                {
                  acceleration: '硬件加速',
                  java: 'Java 运行时',
                  mdns: '无线设备发现',
                  sdkRoot: '当前 SDK',
                  environment: '环境能力',
                  serverEpoch: 'ADB 会话代次',
                } as Record<string, string>
              )[key] ?? key}
            </h3>
            <pre>{typeof value === 'string' ? value : JSON.stringify(value, null, 2)}</pre>
          </section>
        ))}
    </div>
  );
}
