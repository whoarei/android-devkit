import { t, useLocale, setLocale, type Locale } from './i18n';
import { useTheme, setTheme, type Theme } from './theme';
import { version } from '../package.json';
import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
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
  Sun,
  Moon,
} from 'lucide-react';
import { LogPanel, ExportDialog, CloseDialog, type ExportRequest } from './Observability';
import type { ExportPreview } from './bindings';
import { TaskCenter } from './TaskCenter';
import { PhoneTools } from './PhoneTools';
import { SdkManager } from './SdkManager';
import { JavaConfiguration, JavaStatus } from './JavaEnvironment';
import { defaultSdkDownload, sdkSourceName } from './SdkDownloadSettings';
import { TerminalNewSession, TerminalPage, terminalDeviceName } from './TerminalPage';
import { openDeviceTerminal, useTerminals } from './terminal';
import { useSoftwareUpdate, SoftwareUpdateSection, SoftwareUpdateDialog } from './SoftwareUpdate';
import { AvdSettingsFields, AvdPreview, EditAvd, newAvdSettings, type AvdForm } from './AvdEditor';
import { avdImageGroups, hasSdkUpdate } from './sdk';
import { Button, Badge, Modal, Field } from './ui';
import { EnvironmentSetup } from './EnvironmentSetup';
import { AvdUsage, defaultRow, preferenceKey, useDefaultTarget } from './DeviceUtilities';
import { call, native, errorText, stateLabel, taskActive, size, deviceKey } from './api';
import type { Settings, Workspace, Device, Avd, Plan, Operation, ApkInfo, Target } from './types';
type Page = 'devices' | 'terminal' | 'sdk' | 'tasks' | 'diagnostics' | 'settings';
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
  terminal: 'ADB 终端',
  sdk: 'SDK 管理',
  tasks: '任务中心',
  diagnostics: '环境诊断',
  settings: '设置',
};
type ModalName = 'create' | 'wireless' | 'apk' | 'edit' | 'close' | null;
export function App() {
  const locale = useLocale();
  const theme = useTheme();
  const terminalCount = useTerminals((state) => state.sessions.length);
  const updater = useSoftwareUpdate();
  const { page, setPage, selected, select } = useUi();
  const toggleSelection = (key: string) => select(selected === key ? '' : key);
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ['workspace'],
    queryFn: () => call<Workspace>('workspace_snapshot'),
    enabled: native,
    refetchInterval: 5000,
  });
  const data = query.data;
  const [defaultTarget, setDefaultTarget] = useDefaultTarget();
  const [environmentOpen, setEnvironmentOpen] = useState(false);
  const environmentPrompted = useRef(false);
  useEffect(() => {
    if (data?.setupRequired && !environmentPrompted.current) {
      environmentPrompted.current = true;
      setEnvironmentOpen(true);
    }
  }, [data?.setupRequired]);
  const [sdkSources, setSdkSources] = useState(0);
  const [javaOpen, setJavaOpen] = useState(false);
  const [javaOperation, setJavaOperation] = useState<Operation | null>(null);
  const [modal, setModal] = useState<ModalName>(null),
    [plan, setPlan] = useState<Plan | null>(null),
    [accepted, setAccepted] = useState<string[]>([]),
    [error, setError] = useState(''),
    [toast, setToast] = useState(''),
    [busy, setBusy] = useState(false),
    [filter, setFilter] = useState('all'),
    [apk, setApk] = useState<ApkInfo | null>(null),
    [maximized, setMaximized] = useState(false),
    [preview, setPreview] = useState('');
  const [exportPreview, setExportPreview] = useState<ExportPreview | null>(null);
  const [apkDevice, setApkDevice] = useState<Device | null>(null);
  const [apkAvd, setApkAvd] = useState<Avd | null>(null);
  const [choosingApk, setChoosingApk] = useState(false);
  const apkPickerBusy = useRef(false);
  const apkContext = useRef({ data, selected, defaultTarget });
  useEffect(() => {
    apkContext.current = { data, selected, defaultTarget };
  }, [data, selected, defaultTarget]);
  const requestExport = (request: ExportRequest) => {
    void guard(async () => setExportPreview(await call<ExportPreview>('export_prepare', request)));
  };
  const maxButton = useRef<HTMLButtonElement>(null);
  const [nativeHover, setNativeHover] = useState(false);
  const notify = (message: string) => {
    setToast(message);
    setTimeout(() => setToast(''), 5000);
  };
  const refresh = async () => {
    await Promise.all([
      client.invalidateQueries({ queryKey: ['workspace'] }),
      client.invalidateQueries({ queryKey: ['task-list'] }),
      client.invalidateQueries({ queryKey: ['sdk-sites'] }),
    ]);
  };
  const catalogRefresh = useMutation({
    mutationFn: async (beforeRefresh?: () => Promise<void>) => {
      await beforeRefresh?.();
      const warnings = await call<string[]>('catalog_refresh');
      await refresh();
      return warnings;
    },
  });
  const guard = async <T,>(work: () => Promise<T>): Promise<T | undefined> => {
    setError('');
    try {
      return await work();
    } catch (e) {
      setError(errorText(e));
      return undefined;
    }
  };
  useEffect(() => {
    if (native) void call('ui_locale_set', { locale }).catch((error) => setError(errorText(error)));
  }, [locale]);
  useEffect(() => {
    if (native)
      void call('window_action', { action: theme === 'dark' ? 'theme_dark' : 'theme_light' }).catch(
        (error) => setError(errorText(error)),
      );
  }, [theme]);
  const prepare = async (
    kind: string,
    args: Record<string, string> = {},
    target: Target | null = null,
  ) => {
    setBusy(true);
    await guard(async () => {
      const operation = { kind, args, target } satisfies Operation;
      let p: Plan;
      try {
        p = await call<Plan>('plan_create', { operation });
      } catch (e) {
        if (typeof e === 'object' && e && 'code' in e && e.code === 'JAVA_CONFIGURATION_REQUIRED') {
          setJavaOperation(operation);
          setJavaOpen(true);
          return;
        }
        throw e;
      }
      setPlan(p);
      setAccepted([]);
      if (['sdk_install', 'sdk_remove', 'avd_create'].includes(kind)) setJavaOperation(null);
    });
    setBusy(false);
  };
  const chooseApk = async (path?: string, requestedTarget?: string) => {
    if (apkPickerBusy.current) return;
    const context = apkContext.current;
    const key =
      requestedTarget ?? (context.selected || defaultRow(context.data, context.defaultTarget));
    const avd = context.data?.avds.find((a) => a.id === key);
    const sessions =
      avd && context.data?.avds.filter((a) => a.name === avd.name).length === 1
        ? context.data.devices.filter((d) => d.avdName === avd.name)
        : [];
    const initialTarget =
      context.data?.devices.find((d) => deviceKey(d) === key) ??
      (sessions.length === 1 ? sessions[0] : null);
    apkPickerBusy.current = true;
    setChoosingApk(true);
    try {
      await guard(async () => {
        const file = path ?? (await call<string | null>('file_choose', { kind: 'apk' }));
        if (file) {
          setApk(await call<ApkInfo>('apk_prepare', { path: file }));
          setApkDevice(initialTarget ? { ...initialTarget } : null);
          setApkAvd(
            avd && initialTarget?.state !== 'ready' && ['booting', 'busy'].includes(avd.status)
              ? { ...avd }
              : null,
          );
          setModal('apk');
        }
      });
    } finally {
      apkPickerBusy.current = false;
      setChoosingApk(false);
    }
  };
  useEffect(() => {
    if (!native) return;
    let gone = false;
    const cleanups: Promise<() => void>[] = [];
    cleanups.push(listen('workspace-changed', () => refresh()));
    cleanups.push(
      listen<{
        message: string;
      }>('task-error', (event) => setError(event.payload.message)),
    );
    cleanups.push(
      listen<boolean>('native-maximize-hover', (event) => setNativeHover(event.payload)),
    );
    cleanups.push(listen('close-requested', () => setModal('close')));
    cleanups.push(
      getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === 'drop' && document.body.dataset.phoneDrop === 'upload') {
          window.dispatchEvent(
            new CustomEvent('devkit-phone-drop', { detail: event.payload.paths }),
          );
          return;
        }
        if (event.payload.type !== 'drop') return;
        if (event.payload.paths.length !== 1) {
          setError(t('每次仅支持拖入一个完整 APK 文件'));
          return;
        }
        const row = document
          .elementFromPoint(
            event.payload.position.x / devicePixelRatio,
            event.payload.position.y / devicePixelRatio,
          )
          ?.closest<HTMLElement>('[data-device-key]');
        void chooseApk(event.payload.paths[0], row?.dataset.deviceKey);
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
      void call<{
        maximized: boolean;
      }>('window_state').then((v) => setMaximized(v.maximized));
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
  const apkTarget = apkDevice
    ? data?.devices.find((d) => deviceKey(d) === deviceKey(apkDevice))
    : undefined;
  const extraSessions =
    data?.devices.filter(
      (d) => !d.avdName || data.avds.filter((a) => a.name === d.avdName).length !== 1,
    ) ?? [];
  const active = data?.tasks.filter((t) => taskActive(t.state)) ?? [];
  const avdAction = (kind: string, a: Avd, args: Record<string, string> = {}) =>
    prepare(kind, { avdId: a.id, ...args });
  const deviceKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    if (event.key === 'Escape') {
      select('');
      return;
    }
    if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return;
    const rows = [
      ...event.currentTarget.parentElement!.querySelectorAll<HTMLElement>('.device-row'),
    ];
    const index = rows.indexOf(event.currentTarget);
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? rows.length - 1
          : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
    event.preventDefault();
    rows[next]?.focus();
    const key = rows[next]?.dataset.deviceKey;
    if (key) select(key);
  };
  const copyText = (text: string) =>
    guard(async () => {
      await call('text_copy', { text });
      notify(t('已复制'));
    });
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
      notify(t('任务已提交，可在任务中心查看'));
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
        <button
          className="locale-switch"
          aria-label={theme === 'light' ? t('切换为暗色主题') : t('切换为亮色主题')}
          title={theme === 'light' ? t('切换为暗色主题') : t('切换为亮色主题')}
          onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}
        >
          {theme === 'light' ? <Moon size={13} /> : <Sun size={13} />}
        </button>
        <button
          className="locale-switch"
          aria-label={t('切换界面语言')}
          title={t('切换界面语言')}
          onClick={() => setLocale(locale === 'zh-CN' ? 'en-US' : 'zh-CN')}
        >
          {locale === 'zh-CN' ? 'EN' : '中文'}
        </button>
        <div className="window-buttons">
          <button
            aria-label={t('最小化')}
            onClick={() => void call('window_action', { action: 'minimize' })}
          >
            <Minus size={13} />
          </button>
          <button
            ref={maxButton}
            className={nativeHover ? 'native-hover' : ''}
            aria-label={maximized ? t('还原') : t('最大化')}
            onClick={() => void call('window_action', { action: 'maximize' })}
          >
            {maximized ? <Copy size={12} /> : <Square size={12} />}
          </button>
          <button
            aria-label={t('关闭窗口')}
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
        <div className="sidebar-heading">{t('工作空间')}</div>
        <nav>
          {(
            [
              ['devices', Smartphone],
              ['terminal', Terminal],
              ['sdk', Package],
              ['tasks', Activity],
            ] as const
          ).map(([id, Icon]) => (
            <button
              key={id}
              aria-label={t(titles[id])}
              className={page === id ? 'active' : ''}
              onClick={() => setPage(id)}
            >
              <Icon size={18} />
              <span>{t(titles[id])}</span>
              {id === 'tasks' && active.length > 0 && <b>{active.length}</b>}
              {id === 'terminal' && terminalCount > 0 && <b>{terminalCount}</b>}
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
                aria-label={t(titles[id])}
                className={page === id ? 'active' : ''}
                onClick={() => setPage(id)}
              >
                <Icon size={18} />
                {t(titles[id])}
              </button>
            ))}
          </nav>
          <button
            type="button"
            className="sdk-card"
            aria-label={t('打开 SDK 目录')}
            title={`${t('双击在资源管理器中打开 SDK 目录')}\n${data?.settings.sdkRoot ?? ''}`}
            disabled={!native || !data?.settings.sdkRoot}
            onDoubleClick={() => void guard(() => call('sdk_open_directory'))}
            onClick={(event) => {
              if (event.detail === 0) void guard(() => call('sdk_open_directory'));
            }}
          >
            <span className={`connection-dot ${data?.environment.adb ? 'online' : ''}`} />
            <span>
              <b>{data?.environment.adb ? t('SDK 已就位') : t('准备开发环境')}</b>
              <small>{data?.settings.sdkRoot ?? 'E:\\Android\\sdk'}</small>
            </span>
          </button>
          <small className="build-label">
            {updater.info && (
              <button className="text-button update-shortcut" onClick={() => setPage('settings')}>
                <Download size={13} />
                {t('发现新版本：{0}', updater.info.latestVersion)}
              </button>
            )}
            ANDROID DEVKIT <span>{version}</span>
          </small>
        </div>
      </aside>
      <main className="main">
        <div className="page-top">
          <div>
            <div className="eyebrow">ANDROID DEVELOPMENT ENVIRONMENT</div>
            <h1>{t(titles[page])}</h1>
            <p>
              {page === 'devices'
                ? t('准备好设备，让下一次验证更直接。')
                : page === 'terminal'
                  ? t('设备会话直接进入 ADB Shell，通用命令行使用本机 PowerShell。')
                  : page === 'sdk'
                    ? t('按需准备组件，复用已有开发环境。')
                    : page === 'tasks'
                      ? t('每个操作的目标、阶段和结果都留在这里。')
                      : page === 'diagnostics'
                        ? t('从检测到的事实出发，定位环境与连接问题。')
                        : t('为当前工作空间选择路径和行为。')}
            </p>
          </div>
          <div className="page-actions">
            {page === 'terminal' && <TerminalNewSession data={data} />}
            {page === 'devices' && (
              <>
                <Button onClick={() => setModal('wireless')}>
                  <Wifi size={15} />
                  {t('连接设备')}
                </Button>
                <Button primary onClick={() => setModal('create')}>
                  <Plus size={16} />
                  {t('新建模拟器')}
                </Button>
              </>
            )}
            {page === 'sdk' && (
              <Button
                onClick={() => void guard(() => catalogRefresh.mutateAsync(undefined))}
                disabled={busy}
                loading={catalogRefresh.isPending}
              >
                <RefreshCw size={15} className={catalogRefresh.isPending ? 'spin' : ''} />
                {t('刷新')}
              </Button>
            )}
          </div>
        </div>
        {!native && (
          <div className="notice">
            <Monitor size={19} />
            <div>
              <b>{t('桌面工作台预览')}</b>
              <p>
                {t(
                  '运行 pnpm tauri dev 后，即可读取 E:\\Android\\sdk 并连接本机设备。浏览器预览不展示虚构设备。',
                )}
              </p>
            </div>
          </div>
        )}
        {(query.error ||
          (error && !plan && !['create', 'wireless', 'edit'].includes(modal ?? ''))) && (
          <div role="alert" className="error-banner">
            <AlertCircle size={17} />
            <span>{t(query.error ? errorText(query.error) : error)}</span>
            <button aria-label={t('关闭错误')} onClick={() => setError('')}>
              <X size={15} />
            </button>
          </div>
        )}
        {query.isLoading && native && (
          <div className="loading">
            <RefreshCw className="spin" size={20} />
            {t('正在检查 SDK、AVD 与共享 ADB 服务…')}
          </div>
        )}
        {page === 'devices' && (
          <>
            {data && !data.environment.adb && (
              <div className="notice environment-setup-hint">
                <p>{t('当前环境缺少设备工具，可复用已有 SDK 或准备最小环境。')}</p>
                <Button onClick={() => setEnvironmentOpen(true)}>{t('选择或创建 SDK 环境')}</Button>
              </div>
            )}
            <div className="section-toolbar">
              <div className="tabs">
                {[
                  ['all', t('全部')],
                  ['usb', t('真机')],
                  ['emulator', t('模拟器')],
                ].map(([id, label]) => (
                  <button
                    className={filter === id ? 'selected' : ''}
                    key={id}
                    onClick={() => setFilter(id)}
                  >
                    {t(label)}
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
                {t('重新扫描')}
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
                      title={selected === deviceKey(d) ? t('取消选中设备') : t('选中设备')}
                      onKeyDown={(e) => {
                        deviceKeyboard(e);
                        if (['Enter', ' '].includes(e.key) && e.target === e.currentTarget) {
                          e.preventDefault();
                          toggleSelection(deviceKey(d));
                        }
                      }}
                      onClick={() => toggleSelection(deviceKey(d))}
                      data-device-key={deviceKey(d)}
                      className={`device-row ${selected === deviceKey(d) ? 'selected' : ''}`}
                    >
                      <div className="device-icon">
                        <Smartphone size={26} />
                      </div>
                      <div className="device-main">
                        <strong>
                          {d.connection === 'emulator'
                            ? t('待关联 \u00B7 {0}', d.avdName || d.model)
                            : d.model}
                        </strong>
                        <p>
                          {d.connection === 'wireless'
                            ? t('无线调试')
                            : d.connection === 'network'
                              ? t('网络 ADB')
                              : d.connection === 'emulator'
                                ? t('模拟器会话')
                                : 'USB'}{' '}
                          <span>·</span> {d.api ? `API ${d.api}` : d.serial}
                          <span>·</span>
                          {d.serial.slice(-12)}
                        </p>
                      </div>
                      <Badge state={d.state} />
                      <button
                        className="row-button"
                        disabled={d.state !== 'ready' || choosingApk}
                        onClick={(e) => {
                          e.stopPropagation();
                          void chooseApk(undefined, deviceKey(d));
                        }}
                      >
                        {t('安装 APK')}
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
                      title={selected === a.id ? t('取消选中设备') : t('选中设备')}
                      onKeyDown={(e) => {
                        deviceKeyboard(e);
                        if (['Enter', ' '].includes(e.key) && e.target === e.currentTarget) {
                          e.preventDefault();
                          toggleSelection(a.id);
                        }
                      }}
                      onClick={() => toggleSelection(a.id)}
                      data-device-key={a.id}
                      className={`device-row ${selected === a.id ? 'selected' : ''}`}
                    >
                      <div className="device-icon emulator">
                        <Monitor size={26} />
                      </div>
                      <div className="device-main">
                        <strong>{a.displayName}</strong>
                        <p>
                          {t('模拟器 ')}
                          <span>·</span> {a.api ? `API ${a.api}` : a.name}
                          <span>·</span>
                          {a.abi || t('未知架构')}
                        </p>
                        {a.error && <small className="warning-text">{t(a.error)}</small>}
                      </div>
                      <Badge state={a.status} />
                      <button
                        className="row-button"
                        disabled={!['stopped', 'ready'].includes(a.status) || choosingApk}
                        onClick={(e) => {
                          e.stopPropagation();
                          if (a.status === 'ready') {
                            void chooseApk(undefined, a.id);
                          } else void avdAction('avd_start', a);
                        }}
                      >
                        {a.status === 'ready' ? t('安装 APK') : t('启动')}
                        {a.status === 'ready' ? <ChevronRight size={14} /> : <Play size={14} />}
                      </button>
                    </div>
                  ))}
                {(!data || (data.avds.length === 0 && data.devices.length === 0)) && (
                  <div className="empty-state">
                    <div className="empty-icon">
                      <Smartphone size={35} />
                    </div>
                    <h3>{t('为下一次验证准备设备')}</h3>
                    <p>
                      {t('连接开启 USB 调试的手机，或创建一台模拟器。')}
                      <br />
                      {t('已有 SDK 和 AVD 会在扫描后自动显示。')}
                    </p>
                    <div>
                      <Button onClick={() => void prepare('adb_start')}>{t('启动设备服务')}</Button>
                      <Button onClick={() => setModal('create')}>
                        <Plus size={15} />
                        {t('创建模拟器')}
                      </Button>
                    </div>
                  </div>
                )}
                <div className="add-device-hint">
                  <span>{t('拖入单个 APK 安装到所选设备')}</span>
                  <button
                    className="text-button"
                    disabled={choosingApk}
                    onClick={() => void chooseApk()}
                  >
                    <FileUp size={14} />
                    {t('选择 APK')}
                  </button>
                </div>
              </div>
              {hasSelection && (
                <aside className="detail-panel">
                  <div className="detail-heading">
                    <h3>
                      {selectedAvd?.displayName ?? selectedDevice?.model ?? t('原目标已断开')}
                    </h3>
                    <button
                      className="icon-button"
                      aria-label={t('关闭设备详情')}
                      onClick={() => select('')}
                    >
                      <X size={16} />
                    </button>
                  </div>
                  <Badge state={selectedAvd?.status ?? selectedDevice?.state ?? 'offline'} />
                  <dl>
                    <dt>{t('连接 / 标识')}</dt>
                    <dd>{target?.serial ?? selectedAvd?.name}</dd>
                    <dt>{t('系统 / 架构')}</dt>
                    <dd>
                      API {target?.api ?? selectedAvd?.api ?? t('未知')} ·{' '}
                      {target?.abi ?? selectedAvd?.abi ?? t('未知')}
                    </dd>
                    {selectedAvd && (
                      <>
                        <dt>{t('数据目录')}</dt>
                        <dd className="path">{selectedAvd.dataPath}</dd>
                        <dt>{t('系统镜像')}</dt>
                        <dd className="path">{selectedAvd.image}</dd>
                        <dt>{t('资源配置')}</dt>
                        <dd>
                          {selectedAvd.memory}
                          {t(' MiB 内存 \u00B7 ')}
                          {selectedAvd.disk}
                        </dd>
                      </>
                    )}
                  </dl>
                  {selectedAvd && <AvdUsage key={selectedAvd.id} avd={selectedAvd} />}
                  {selectedDevice?.state === 'unauthorized' && (
                    <p className="hint">
                      {t('请解锁设备并允许此电脑进行 ADB 调试；授权后将自动更新。')}
                    </p>
                  )}
                  <div className="detail-actions">
                    <Button
                      onClick={() => void copyText(target?.serial ?? selectedAvd?.name ?? '')}
                    >
                      {t('复制设备标识')}
                    </Button>
                    {selectedAvd && (
                      <Button onClick={() => void copyText(selectedAvd.dataPath)}>
                        {t('复制完整路径')}
                      </Button>
                    )}
                    <Button
                      onClick={() =>
                        void guard(async () => {
                          const key = preferenceKey(selectedAvd, selectedDevice);
                          setDefaultTarget(defaultTarget === key ? '' : key);
                          notify(t(defaultTarget === key ? '已取消默认目标' : '已保存默认目标'));
                        })
                      }
                    >
                      {t(
                        defaultTarget === preferenceKey(selectedAvd, selectedDevice)
                          ? '取消默认目标'
                          : '设为默认目标',
                      )}
                    </Button>
                    <Button
                      primary
                      disabled={
                        (target?.state !== 'ready' &&
                          !selectedAvd?.status.match(/^(booting|busy)$/)) ||
                        choosingApk
                      }
                      onClick={() => void chooseApk()}
                    >
                      <Download size={15} />
                      {t('安装 APK')}
                    </Button>
                    <Button
                      disabled={target?.state !== 'ready'}
                      onClick={() => void prepare('capture', {}, target ?? null)}
                    >
                      <Camera size={15} />
                      {t('截图')}
                    </Button>
                    {target && (
                      <Button
                        onClick={() => {
                          setPage('devices');
                          document.getElementById('logs')?.scrollIntoView({ behavior: 'smooth' });
                        }}
                      >
                        <Terminal size={15} />
                        {t('查看日志')}
                      </Button>
                    )}
                    <Button
                      disabled={target?.state !== 'ready'}
                      onClick={() => {
                        if (!target) return;
                        setPage('terminal');
                        void guard(() =>
                          openDeviceTerminal(target, terminalDeviceName(target, data)),
                        );
                      }}
                    >
                      <Terminal size={15} />
                      {t('ADB 终端')}
                    </Button>
                    {selectedAvd && (
                      <>
                        {['booting', 'busy'].includes(selectedAvd.status) && (
                          <Button
                            disabled={busy}
                            onClick={() => void avdAction('avd_wait', selectedAvd)}
                          >
                            {t('继续等待系统就绪')}
                          </Button>
                        )}
                        <Button
                          disabled={selectedAvd.status !== 'stopped'}
                          onClick={() => void avdAction('avd_cold', selectedAvd)}
                        >
                          {t('冷启动')}
                        </Button>
                        <Button
                          onClick={() => void avdAction('avd_stop', selectedAvd)}
                          disabled={!['ready', 'booting', 'busy'].includes(selectedAvd.status)}
                        >
                          <Square size={13} />
                          {t('正常停止')}
                        </Button>
                        <Button
                          disabled={selectedAvd.status !== 'stopped'}
                          onClick={() => setModal('edit')}
                        >
                          {t('编辑配置')}
                        </Button>
                        <Button onClick={() => void avdAction('avd_focus', selectedAvd)}>
                          {t('打开窗口')}
                        </Button>
                        <Button
                          danger
                          onClick={() => void avdAction('avd_force_stop', selectedAvd)}
                        >
                          {t('强制停止')}
                        </Button>
                        <Button
                          danger
                          disabled={selectedAvd.status !== 'stopped'}
                          onClick={() => void avdAction('avd_wipe', selectedAvd)}
                        >
                          {t('清除数据')}
                        </Button>
                        <Button
                          danger
                          disabled={selectedAvd.status !== 'stopped'}
                          onClick={() => void avdAction('avd_delete', selectedAvd)}
                        >
                          <Trash2 size={14} />
                          {t('删除 AVD')}
                        </Button>
                      </>
                    )}
                    {selectedDevice &&
                      ['wireless', 'network'].includes(selectedDevice.connection) && (
                        <Button
                          onClick={() =>
                            void prepare(
                              selectedDevice.connection === 'network'
                                ? 'network_disconnect'
                                : 'wireless_disconnect',
                              { address: selectedDevice.serial },
                            )
                          }
                        >
                          {t('断开网络连接')}
                        </Button>
                      )}
                  </div>
                  {target && (
                    <PhoneTools
                      key={deviceKey(target)}
                      target={target}
                      tasks={data?.tasks ?? []}
                      prepare={prepare}
                      planOpen={Boolean(plan)}
                    />
                  )}
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
                <b>{data?.server ? t(data.server) : t('等待桌面环境')}</b>
                <span>{t('SDK 与 AVD 独立管理，退出工作台不会自动结束模拟器。')}</span>
              </div>
              <button className="text-button" onClick={() => setPage('diagnostics')}>
                {t('查看诊断')}
                <ChevronRight size={14} />
              </button>
            </div>
          </>
        )}
        <TerminalPage data={data} visible={page === 'terminal'} openSdk={() => setPage('sdk')} />
        <div hidden={page !== 'sdk'}>
          <SdkManager
            data={data}
            prepare={prepare}
            busy={busy}
            refresh={refresh}
            refreshing={catalogRefresh.isPending}
            refreshCatalog={(beforeRefresh) => catalogRefresh.mutateAsync(beforeRefresh)}
            warnings={catalogRefresh.isSuccess ? catalogRefresh.data : []}
            startWithSources={sdkSources}
            configureJava={() => setJavaOpen(true)}
          />
        </div>
        {page === 'tasks' && (
          <TaskCenter
            guard={guard}
            prepare={prepare}
            refresh={refresh}
            showPlan={(value) => {
              setPlan(value);
              setAccepted([]);
            }}
            showPreview={setPreview}
            onExport={requestExport}
            onExportPreview={setExportPreview}
            previewOpen={Boolean(exportPreview || preview)}
          />
        )}
        {page === 'settings' && (
          <SettingsPanel
            updater={updater}
            chooseEnvironment={() => setEnvironmentOpen(true)}
            initial={data?.settings}
            configureJava={() => setJavaOpen(true)}
            manageSdkSources={() => {
              setSdkSources((value) => value + 1);
              setPage('sdk');
            }}
            guard={guard}
            done={() => {
              void refresh();
              notify(t('设置已保存'));
            }}
          />
        )}
        {page === 'diagnostics' && (
          <>
            <JavaStatus settings={data?.settings} open={() => setJavaOpen(true)} />
            <Diagnostics
              guard={guard}
              prepare={prepare}
              onExport={requestExport}
              copyText={copyText}
            />
          </>
        )}
      </main>
      {toast && (
        <div className="toast" role="status">
          <Check size={17} />
          {t(toast)}
        </div>
      )}
      {environmentOpen && data && (
        <EnvironmentSetup
          data={data}
          close={() => setEnvironmentOpen(false)}
          done={async (purpose) => {
            await refresh();
            if (purpose !== 'reuse') setPage('sdk');
            if (purpose === 'usb' || purpose === 'emulator') {
              await catalogRefresh.mutateAsync(undefined);
              await prepare('bootstrap', { purpose });
            }
            setEnvironmentOpen(false);
          }}
        />
      )}
      {modal === 'create' && (
        <CreateAvd
          data={data}
          close={() => setModal(null)}
          prepare={prepare}
          busy={busy}
          error={t(error)}
          hidden={javaOpen || !!plan}
        />
      )}
      <SoftwareUpdateDialog updater={updater} />
      {javaOpen && (
        <JavaConfiguration
          resume={!!javaOperation}
          close={() => setJavaOpen(false)}
          refresh={refresh}
          showNetwork={() => {
            setJavaOpen(false);
            setPage('settings');
          }}
          done={async () => {
            const operation = javaOperation;
            await refresh();
            if (operation) {
              const next = await call<Plan>('plan_create', { operation });
              if (operation.kind.startsWith('sdk_')) setPage('sdk');
              setAccepted([]);
              setPlan(next);
            }
            setJavaOperation(null);
            setJavaOpen(false);
          }}
        />
      )}
      {modal === 'wireless' && (
        <ConnectDevice
          close={() => setModal(null)}
          prepare={prepare}
          busy={busy}
          error={t(error)}
          clearError={() => setError('')}
        />
      )}
      {modal === 'edit' && selectedAvd && (
        <EditAvd
          avd={selectedAvd}
          busy={busy}
          error={t(error)}
          close={() => setModal(null)}
          save={(args) => void avdAction('avd_edit', selectedAvd, args)}
        />
      )}
      {modal === 'apk' && apk && (
        <Modal title={t('安装 APK')} onClose={() => setModal(null)} hidden={Boolean(plan)}>
          <div className="file-summary">
            <Package size={28} />
            <div>
              <b>{apk.fileName}</b>
              <small>
                {size(apk.size)} · {apk.package || t('包名暂无法读取')} ·{' '}
                {apk.version || t('版本未知')}
              </small>
            </div>
          </div>
          <Field label={t('目标设备')}>
            <select
              value={apkAvd ? `avdwait:${apkAvd.id}` : apkDevice ? deviceKey(apkDevice) : ''}
              disabled={busy}
              onChange={(e) => {
                const avd = data?.avds.find((avd) => `avdwait:${avd.id}` === e.target.value);
                const sessions =
                  avd && data?.avds.filter((other) => other.name === avd.name).length === 1
                    ? data.devices.filter((device) => device.avdName === avd.name)
                    : [];
                setApkAvd(avd ?? null);
                setApkDevice(
                  avd
                    ? sessions.length === 1
                      ? { ...sessions[0] }
                      : null
                    : (data?.devices.find((device) => deviceKey(device) === e.target.value) ??
                        null),
                );
              }}
            >
              <option value="">{t('选择明确目标')}</option>
              {apkDevice && !apkAvd && apkTarget?.state !== 'ready' && (
                <option value={deviceKey(apkDevice)} disabled>
                  {apkDevice.model} · {apkDevice.serial} · {t('暂不可用')}
                </option>
              )}
              {data?.devices
                .filter((d) => d.state === 'ready')
                .map((d) => (
                  <option key={deviceKey(d)} value={deviceKey(d)}>
                    {d.model} · {d.serial}
                  </option>
                ))}
              {apkAvd &&
                !data?.avds.some(
                  (avd) => avd.id === apkAvd.id && ['booting', 'busy'].includes(avd.status),
                ) && (
                  <option value={`avdwait:${apkAvd.id}`}>
                    {apkAvd.displayName} · {t('就绪后安装 APK')}
                  </option>
                )}
              {data?.avds
                .filter((avd) => ['booting', 'busy'].includes(avd.status))
                .map((avd) => (
                  <option key={avd.id} value={`avdwait:${avd.id}`}>
                    {avd.displayName} · {t('就绪后安装 APK')}
                  </option>
                ))}
            </select>
          </Field>
          {apkDevice && !apkAvd && apkTarget?.state !== 'ready' && (
            <p className="warning-text" role="status">
              {t('目标设备暂不可用，请重新选择或等待设备恢复。')}
            </p>
          )}
          {apkAvd && (
            <p className="hint">
              {t(
                '等待所选模拟器系统就绪，最多 {0} 秒；可在任务中心取消。连接变化或超时后不自动改投或重试。',
                data?.settings.bootTimeoutSecs ?? 180,
              )}
            </p>
          )}
          <p className="hint">
            {t('最低 API ')}
            {apk.minSdk || t('未知')} · ABI {apk.abis.join(', ') || t('无指定 native ABI')}
            {t('。默认覆盖安装并保留数据。')}
          </p>
          {apk.warning && <p className="warning-text">{t(apk.warning)}</p>}
          <div className="modal-actions">
            <Button onClick={() => setModal(null)}>{t('取消')}</Button>
            <Button
              primary
              disabled={busy || (!apkAvd && (!apkTarget || apkTarget.state !== 'ready'))}
              onClick={() =>
                void prepare(
                  apkAvd ? 'apk_install_wait' : 'apk_install',
                  { apkId: apk.id, ...(apkAvd ? { avdId: apkAvd.id } : {}) },
                  apkAvd ? apkDevice : (apkTarget ?? null),
                )
              }
            >
              {t('确认安装计划')}
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
          {error && (
            <p className="error-banner" role="alert">
              {t(error)}
            </p>
          )}
          <div className="plan-summary">
            {plan.effects.map((effect, i) => (
              <p key={i}>
                <Check size={14} />
                {t(effect)}
              </p>
            ))}
          </div>
          {plan.warnings.map((w, i) => (
            <p className="plan-warning" key={i}>
              <AlertCircle size={16} />
              {t(w)}
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
                {t('我已查看并接受此许可')}
              </label>
            </div>
          ))}
          <div className="modal-actions">
            <Button onClick={() => setPlan(null)}>{t('返回修改')}</Button>
            <Button
              primary
              danger={
                plan.operation.kind.includes('delete') ||
                plan.operation.kind.includes('wipe') ||
                (['file_upload', 'file_download'].includes(plan.operation.kind) &&
                  plan.operation.args.conflicts === 'replace')
              }
              disabled={busy || plan.licenses.some((l) => !accepted.includes(l.hash))}
              onClick={() => void submit()}
            >
              {busy ? t('正在提交…') : t(plan.title)}
            </Button>
          </div>
        </Modal>
      )}
      {preview && (
        <Modal title={t('设备截图')} onClose={() => setPreview('')} wide>
          <img className="screenshot" src={preview} alt={t('设备截图预览')} />
        </Modal>
      )}
    </div>
  );
}
function CreateAvd({
  data,
  close,
  prepare,
  busy,
  error,
  hidden = false,
}: {
  data?: Workspace;
  close: () => void;
  prepare: (k: string, a: Record<string, string>) => Promise<void>;
  busy: boolean;
  error: string;
  hidden?: boolean;
}) {
  const [step, setStep] = useState(1),
    [form, setForm] = useState<AvdForm>({
      ...newAvdSettings(),
      name: 'DevKit_Phone',
      displayName: t('日常测试手机'),
      profile: 'pixel_2',
      image:
        data?.packages.find((p) => p.category === 'image' && p.installed && p.abi === 'x86_64')
          ?.id ?? '',
    });
  const set = (key: string, v: string) => setForm((f) => ({ ...f, [key]: v }));
  const [expandedImageApis, setExpandedImageApis] = useState<Record<string, boolean>>({});
  const imageGroups = avdImageGroups(data?.packages ?? []);
  return (
    <Modal title={t('新建模拟器')} onClose={close} wide className="avd-modal" hidden={hidden}>
      <div className="avd-modal-body">
        <div className="wizard-steps">
          {[t('设备形态'), t('系统镜像'), t('确认配置')].map((label, i) => (
            <span className={step === i + 1 ? 'current' : ''} key={t(label)}>
              <b>{i + 1}</b>
              {t(label)}
            </span>
          ))}
        </div>
        {step === 1 ? (
          <>
            <h3>{t('选择适合验证的设备')}</h3>
            <div className="device-choices">
              <button
                className={form.profile === 'pixel_2' ? 'selected' : ''}
                onClick={() => {
                  set('profile', 'pixel_2');
                  set('displayName', t('日常测试手机'));
                  set('screenWidth', '1080');
                  set('screenHeight', '1920');
                  set('density', '420');
                }}
              >
                <Smartphone size={36} />
                <b>{t('手机')}</b>
                <small>{t('Pixel 2 \u00B7 常用触屏设备')}</small>
              </button>
              <button
                className={form.profile === 'Nexus 9' ? 'selected' : ''}
                onClick={() => {
                  set('profile', 'Nexus 9');
                  set('displayName', t('平板适配测试'));
                  set('screenWidth', '1536');
                  set('screenHeight', '2048');
                  set('density', '320');
                }}
              >
                <Monitor size={38} />
                <b>{t('平板')}</b>
                <small>{t('Nexus 9 \u00B7 大屏适配')}</small>
              </button>
            </div>
            <p className="hint">
              {t('使用官方硬件 profile，性能、显示、摄像头、网络和存储可在确认配置时调整。')}
            </p>
          </>
        ) : step === 2 ? (
          <>
            <h3>{t('选择 Android 系统')}</h3>
            <p className="hint">
              {t('首发 Windows x64 优先选择 x86_64 镜像。缺失镜像会加入创建任务。')}
            </p>
            <div className="image-options">
              {imageGroups.map(([api, images], index) => {
                const selected = images.some((p) => form.image === p.id);
                const expanded = expandedImageApis[api] ?? (form.image ? selected : index === 0);
                const title = api ? `Android · API ${api}` : t('API 未知');
                return (
                  <section className="image-api-group" key={api} aria-label={title}>
                    <button
                      className="image-api-heading"
                      aria-expanded={expanded}
                      aria-controls={`avd-images-${api || 'unknown'}`}
                      onClick={() =>
                        setExpandedImageApis((values) => ({ ...values, [api]: !expanded }))
                      }
                    >
                      <ChevronRight size={15} aria-hidden="true" />
                      <b>{title}</b>
                      <span>{t('{0} 个镜像', images.length)}</span>
                      {selected && <span className="image-api-selected">{t('已选择')}</span>}
                    </button>
                    <div id={`avd-images-${api || 'unknown'}`} hidden={!expanded}>
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
                                ? hasSdkUpdate(p)
                                  ? t('有可选更新')
                                  : t('已安装')
                                : size(p.size)}
                            </small>
                          </div>
                        </label>
                      ))}
                    </div>
                  </section>
                );
              })}
              {!imageGroups.length && (
                <p>{t('尚无镜像目录，请先关闭向导并在 SDK 页面刷新官方仓库。')}</p>
              )}
            </div>
          </>
        ) : (
          <>
            <div className="form-grid">
              <Field label={t('内部名称')}>
                <input value={form.name} onChange={(e) => set('name', e.target.value)} />
              </Field>
              <Field label={t('显示名称')}>
                <input
                  value={form.displayName}
                  onChange={(e) => set('displayName', e.target.value)}
                />
              </Field>
            </div>
            <div className="avd-editor-layout">
              <AvdSettingsFields values={form} set={set} creating />
              <AvdPreview
                values={form}
                title={form.displayName}
                api={form.image.split(';')[1]?.replace('android-', '')}
                abi="x86_64"
              />
            </div>
            <p className="hint">
              {t('数据目录：')}
              {data?.settings.avdRoots.at(-1)}
              <br />
              {t('系统镜像：')}
              {form.image}
            </p>
          </>
        )}
        {error && (
          <p className="error-banner" role="alert">
            {t(error)}
          </p>
        )}
      </div>
      <div className="modal-actions">
        <Button onClick={step === 1 ? close : () => setStep((s) => s - 1)}>
          {step === 1 ? t('取消') : t('上一步')}
        </Button>
        {step < 3 ? (
          <Button
            primary
            disabled={step === 2 && !form.image}
            onClick={() => setStep((s) => s + 1)}
          >
            {t('下一步')}
            <ChevronRight size={14} />
          </Button>
        ) : (
          <Button primary disabled={busy} onClick={() => void prepare('avd_create', form)}>
            {t('查看创建计划')}
          </Button>
        )}
      </div>
    </Modal>
  );
}
function ConnectDevice({
  close,
  prepare,
  busy,
  error,
  clearError,
}: {
  close: () => void;
  prepare: (k: string, a: Record<string, string>) => Promise<void>;
  busy: boolean;
  error: string;
  clearError: () => void;
}) {
  const [mode, setMode] = useState<'pair' | 'connect' | 'network'>('pair'),
    [host, setHost] = useState(''),
    [pairPort, setPairPort] = useState(''),
    [connectPort, setConnectPort] = useState(''),
    [networkPort, setNetworkPort] = useState('5555'),
    [code, setCode] = useState('');
  const port = mode === 'pair' ? pairPort : mode === 'connect' ? connectPort : networkPort;
  const setPort =
    mode === 'pair' ? setPairPort : mode === 'connect' ? setConnectPort : setNetworkPort;
  const addressHost = host.trim();
  const address = `${addressHost.includes(':') && !addressHost.startsWith('[') ? `[${addressHost}]` : addressHost}:${port.trim()}`;
  return (
    <Modal title={t('连接设备')} onClose={close}>
      <div className="tabs" aria-label={t('设备连接方式')}>
        {(
          [
            ['pair', t('1. 无线配对')],
            ['connect', t('2. 已配对连接')],
            ['network', t('网络 ADB')],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            className={mode === value ? 'selected' : ''}
            aria-pressed={mode === value}
            disabled={busy}
            onClick={() => {
              setMode(value);
              setCode('');
              clearError();
            }}
          >
            {t(label)}
          </button>
        ))}
      </div>
      <p className="hint">
        {mode === 'network'
          ? t('直接连接设备已开启的 ADB TCP/IP 端口，无需配对码。电脑需能访问设备所在网络。')
          : t('手机开启开发者选项 → 无线调试。电脑和手机应在可互通的同一网络。')}
      </p>
      <Field label={t('设备 IP 地址')}>
        <input
          placeholder="192.168.1.10"
          value={host}
          disabled={busy}
          onChange={(e) => setHost(e.target.value)}
        />
      </Field>
      <Field
        label={mode === 'pair' ? t('配对端口') : t('连接端口')}
        hint={
          mode === 'network'
            ? t('默认 5555，可改为设备实际监听的端口。支持 IPv4 / IPv6 地址。')
            : t('配对端口与连接端口通常不同，请以手机当前页面为准。')
        }
      >
        <input
          inputMode="numeric"
          value={port}
          disabled={busy}
          onChange={(e) => setPort(e.target.value)}
        />
      </Field>
      {mode === 'pair' && (
        <Field label={t('6 位配对码')}>
          <input
            autoComplete="off"
            maxLength={6}
            inputMode="numeric"
            value={code}
            disabled={busy}
            onChange={(e) => setCode(e.target.value)}
          />
        </Field>
      )}
      {mode === 'network' ? (
        <p className="hint">
          {t('首次开启：先通过 USB 连接并允许调试，再执行')}{' '}
          <code>{t('adb -s <USB 序列号> tcpip 5555')}</code>
          {t('，然后使用设备 IP 连接。 已开启网络调试的设备可直接连接，建议仅在可信网络使用。')}
        </p>
      ) : (
        <p className="hint">
          {t('配对成功不代表已连接。取消配对需在手机的无线调试设置中选择“忘记”。')}
        </p>
      )}
      {error && (
        <p className="plan-warning" role="alert">
          {t(error)}
        </p>
      )}
      <div className="modal-actions">
        <Button onClick={close}>{t('取消')}</Button>
        <Button
          primary
          disabled={busy}
          onClick={() =>
            void prepare(
              mode === 'pair'
                ? 'wireless_pair'
                : mode === 'network'
                  ? 'network_connect'
                  : 'wireless_connect',
              {
                address,
                ...(mode === 'pair' ? { code: code.trim() } : {}),
              },
            )
          }
        >
          {busy ? t('正在检查…') : mode === 'pair' ? t('配对') : t('连接并验证')}
        </Button>
      </div>
    </Modal>
  );
}
function SettingsPanel({
  initial,
  updater,
  guard,
  done,
  manageSdkSources,
  configureJava,
  chooseEnvironment,
}: {
  initial?: Settings;
  manageSdkSources: () => void;
  configureJava: () => void;
  chooseEnvironment: () => void;
  updater: ReturnType<typeof useSoftwareUpdate>;
  guard: <T>(f: () => Promise<T>) => Promise<T | undefined>;
  done: () => void;
}) {
  const locale = useLocale();
  const theme = useTheme();
  const [form, setForm] = useState<Settings>(
    initial ?? {
      sdkRoot: '',
      avdRoots: [],
      javaPath: '',
      proxy: '',
      bootTimeoutSecs: 180,
      trayEnabled: false,
      sdkSites: [],
      sdkChannel: 0,
      sdkDownload: defaultSdkDownload(),
    },
  );
  useEffect(() => {
    if (initial) setForm(initial);
  }, [initial?.sdkRoot]);
  useEffect(() => {
    if (initial) setForm((current) => ({ ...current, javaPath: initial.javaPath }));
  }, [initial?.javaPath]);
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
      <SoftwareUpdateSection updater={updater} />
      <section>
        <h3>{t('界面偏好')}</h3>
        <Field label={t('界面主题')} hint={t('立即生效并自动保存')}>
          <select value={theme} onChange={(event) => setTheme(event.target.value as Theme)}>
            <option value="light">{t('亮色')}</option>
            <option value="dark">{t('暗色')}</option>
          </select>
        </Field>
        <Field label={t('界面语言')} hint={t('立即生效并自动保存')}>
          <select value={locale} onChange={(event) => setLocale(event.target.value as Locale)}>
            <option value="zh-CN">简体中文</option>
            <option value="en-US">English</option>
          </select>
        </Field>
      </section>
      <section>
        <h3>{t('工具与数据路径')}</h3>
        <Button onClick={chooseEnvironment}>{t('发现并选择 SDK 环境')}</Button>
        <p className="hint">
          {t('只影响本应用子进程，不修改系统 PATH、JAVA_HOME 或 Android Studio 配置。')}
        </p>
        <Field label={t('当前 SDK 根目录')}>
          <div className="input-action">
            <input value={form.sdkRoot} onChange={(e) => set('sdkRoot', e.target.value)} />
            <Button onClick={() => void guard(() => choose('sdkRoot'))}>{t('选择目录')}</Button>
          </div>
        </Field>
        <JavaStatus settings={initial} open={configureJava} />
        {form.javaPath && (
          <p className="hint">
            <code>{form.javaPath}</code>
          </p>
        )}
        <Field label={t('AVD 根目录（一行一个，最后一个用于新建）')}>
          <textarea
            rows={3}
            value={form.avdRoots.join('\n')}
            onChange={(e) => set('avdRoots', e.target.value.split('\n'))}
          />
        </Field>
      </section>
      <section>
        <h3>{t('窗口与退出')}</h3>
        <label className="check-row">
          <input
            type="checkbox"
            checked={form.trayEnabled}
            onChange={(e) => set('trayEnabled', e.target.checked)}
          />
          {t('启用托盘：关闭窗口后隐藏并继续任务')}
        </label>
        <p className="hint">
          {t(
            '默认关闭。启用后，可通过托盘打开工作台或选择退出；退出仍检查任务、文件采集与未保存产物。',
          )}
        </p>
        <h3>{t('网络与启动')}</h3>
        <div className="sdk-source-setting">
          <div>
            <b>{t('SDK 下载源')}</b>
            <p className="hint">{sdkSourceName(initial?.sdkDownload)}</p>
          </div>
          <Button onClick={manageSdkSources}>{t('管理下载源')}</Button>
        </div>
        <Field
          label={t('手动代理')}
          hint={t('示例 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080；留空直连。')}
        >
          <input value={form.proxy} onChange={(e) => set('proxy', e.target.value)} />
        </Field>
        <Field label={t('模拟器启动提醒阈值（秒）')}>
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
        {t('保存设置')}
      </Button>
    </div>
  );
}
function Diagnostics({
  onExport,
  guard,
  prepare,
  copyText,
}: {
  guard: <T>(f: () => Promise<T>) => Promise<T | undefined>;
  onExport: (request: ExportRequest) => void;
  prepare: (k: string, a?: Record<string, string>) => Promise<void>;
  copyText: (text: string) => Promise<unknown>;
}) {
  const [raw, setRaw] = useState(false);
  const [summary, setSummary] = useState('');
  const [report, setReport] = useState<Record<string, unknown> | null>(null);
  useEffect(() => {
    void guard(async () => setReport(await call('diagnostics')));
  }, []);
  return (
    <div className="diagnostic-panel">
      <div className="notice">
        <SlidersHorizontal size={22} />
        <div>
          <b>{t('检查实际工具链与连接事实')}</b>
          <p>{t('不会自动重启共享 ADB、修改系统虚拟化功能或清除设备数据。')}</p>
        </div>
      </div>
      <div className="detail-actions">
        <Button onClick={() => void guard(async () => setReport(await call('diagnostics')))}>
          <RefreshCw size={14} />
          {t('重新检测')}
        </Button>
        <label>
          <input
            type="checkbox"
            checked={raw}
            onChange={(e) => {
              setRaw(e.target.checked);
              setSummary('');
            }}
          />
          {t('包含原始标识')}
        </label>
        <Button onClick={() => onExport({ kind: 'diagnostics', redacted: !raw })}>
          {t('预览诊断导出')}
        </Button>
        <Button
          onClick={() =>
            void guard(async () =>
              setSummary(await call<string>('environment_summary', { redacted: !raw })),
            )
          }
        >
          {t('预览并复制环境摘要')}
        </Button>
        <Button onClick={() => void prepare('adb_start')}>{t('启动 ADB')}</Button>
        <Button danger onClick={() => void prepare('adb_restart')}>
          {t('重启共享 ADB')}
        </Button>
      </div>
      {summary && (
        <Modal title={t('环境摘要预览')} onClose={() => setSummary('')} wide>
          <p>{t(raw ? '包含原始标识，请检查后复制。' : '已默认脱敏，请检查后复制。')}</p>
          <pre className="export-preview">{summary}</pre>
          <div className="modal-actions">
            <Button primary onClick={() => void copyText(summary)}>
              {t('复制环境摘要')}
            </Button>
          </div>
        </Modal>
      )}
      {report &&
        Object.entries(report).map(([key, value]) => (
          <section className="diagnostic-item" key={key}>
            <h3>
              {(
                {
                  acceleration: t('硬件加速'),
                  java: t('Java 运行时'),
                  mdns: t('无线设备发现'),
                  sdkRoot: t('当前 SDK'),
                  environment: t('环境能力'),
                  serverEpoch: t('ADB 会话代次'),
                } as Record<string, string>
              )[key] ?? key}
            </h3>
            <pre>{typeof value === 'string' ? value : JSON.stringify(value, null, 2)}</pre>
          </section>
        ))}
    </div>
  );
}
