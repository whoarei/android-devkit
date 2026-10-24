import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import {
  AlertCircle,
  ChevronDown,
  Eraser,
  List,
  LockKeyhole,
  Monitor,
  Plus,
  Smartphone,
  Terminal,
  X,
} from 'lucide-react';
import { t, useLocale } from './i18n';
import { useTheme } from './theme';
import { native } from './api';
import type { Device, Workspace } from './types';
import type { TerminalInfo } from './bindings';
import { Button } from './ui';
import {
  attachTerminal,
  clearTerminal,
  closeTerminal,
  fitTerminal,
  focusTerminal,
  initializeTerminals,
  insertTerminalText,
  interruptTerminal,
  newTerminal,
  reopenTerminal,
  reportTerminalError,
  resolveTerminalDevice,
  themeTerminals,
  useTerminals,
} from './terminal';
import '@xterm/xterm/css/xterm.css';
import './terminal.css';

export function terminalDeviceName(device: Device, data?: Workspace) {
  const avds = data?.avds.filter((avd) => avd.name === device.avdName) ?? [];
  return avds.length === 1 ? avds[0].displayName : device.model || device.serial;
}
export function TerminalNewSession({
  data,
  compact = false,
}: {
  data?: Workspace;
  compact?: boolean;
}) {
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const sessions = useTerminals((state) => state.sessions);
  const disabled = !native || !data?.environment.adb || sessions.length >= 8;
  useEffect(() => {
    if (!position) return;
    menu.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
    const outside = (event: MouseEvent) => {
      if (
        !menu.current?.contains(event.target as Node) &&
        !trigger.current?.contains(event.target as Node)
      )
        setPosition(null);
    };
    const resize = () => setPosition(null);
    document.addEventListener('mousedown', outside);
    window.addEventListener('resize', resize);
    return () => {
      document.removeEventListener('mousedown', outside);
      window.removeEventListener('resize', resize);
    };
  }, [position]);
  const open = (device?: Device) => {
    setPosition(null);
    void newTerminal(device, device ? terminalDeviceName(device, data) : undefined).catch(
      reportTerminalError,
    );
  };
  return (
    <>
      <button
        ref={trigger}
        className={compact ? 'terminal-add-tab icon-button' : 'button primary'}
        disabled={disabled}
        aria-label={t('新建会话')}
        aria-expanded={Boolean(position)}
        aria-haspopup="menu"
        onClick={() => {
          if (position) {
            setPosition(null);
            return;
          }
          const rect = trigger.current!.getBoundingClientRect();
          setPosition({
            top: Math.min(rect.bottom + 6, Math.max(12, innerHeight - 280)),
            left: Math.max(12, Math.min(rect.right - 300, innerWidth - 312)),
          });
        }}
      >
        <Plus size={16} />
        {!compact && t('新建会话')}
      </button>
      {position &&
        createPortal(
          <div
            ref={menu}
            className="terminal-session-menu"
            role="menu"
            aria-label={t('新建会话')}
            style={position}
            onKeyDown={(event) => {
              const buttons = [
                ...menu.current!.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'),
              ];
              const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
              if (event.key === 'Escape') {
                event.preventDefault();
                setPosition(null);
                trigger.current?.focus();
              }
              if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                event.preventDefault();
                buttons[
                  (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
                ]?.focus();
              }
            }}
          >
            <button role="menuitem" onClick={() => open()}>
              <Monitor size={18} />
              <span>
                <b>{t('通用命令行')}</b>
                <small>{t('本机 PowerShell')}</small>
              </span>
            </button>
            <div className="terminal-menu-label">{t('设备 Shell')}</div>
            {(data?.devices ?? []).map((device) => (
              <button
                key={`${device.serverEpoch}:${device.transportId}:${device.serial}`}
                role="menuitem"
                disabled={device.state !== 'ready'}
                onClick={() => open(device)}
              >
                <Smartphone size={18} />
                <span>
                  <b>{terminalDeviceName(device, data)}</b>
                  <small>
                    {device.serial}
                    {device.state !== 'ready' && ` · ${t('不可用')}`}
                  </small>
                </span>
              </button>
            ))}
            {!data?.devices.length && <p>{t('暂无连接设备')}</p>}
          </div>,
          document.body,
        )}
    </>
  );
}

function TerminalSurface({ id, visible }: { id: string; visible: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = host.current!;
    attachTerminal(id, element);
    let timer: ReturnType<typeof setTimeout>;
    const observer = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => fitTerminal(id), 30);
    });
    observer.observe(element);
    return () => {
      clearTimeout(timer);
      observer.disconnect();
    };
  }, [id]);
  useEffect(() => {
    if (visible) {
      const frame = requestAnimationFrame(() => {
        fitTerminal(id);
        focusTerminal(id);
      });
      return () => cancelAnimationFrame(frame);
    }
  }, [id, visible]);
  return (
    <div ref={host} className="terminal-surface" hidden={!visible} aria-label={t('交互终端')} />
  );
}
const stateText: Record<string, string> = {
  starting: '连接中',
  active: '已连接',
  exited: '已退出',
  disconnected: '已断开',
  failed: '连接失败',
  closed: '已关闭',
};
export function TerminalPage({
  data,
  visible,
  openSdk,
}: {
  data?: Workspace;
  visible: boolean;
  openSdk: () => void;
}) {
  useLocale();
  const theme = useTheme();
  const { sessions, activeId, error, select, clearError } = useTerminals();
  const active = sessions.find((session) => session.id === activeId);
  const [snippets, setSnippets] = useState(false);
  useEffect(() => {
    void initializeTerminals().catch(reportTerminalError);
  }, []);
  useEffect(() => {
    themeTerminals();
  }, [theme]);
  useEffect(() => {
    setSnippets(false);
  }, [activeId, visible]);
  const ended = Boolean(active && !['active', 'starting'].includes(active.state));
  const freshDevice =
    active?.kind === 'device' ? resolveTerminalDevice(active.id, data?.devices ?? []) : undefined;
  return (
    <section className="terminal-page" hidden={!visible} aria-label={t('ADB 终端')}>
      {error && (
        <div className="error-banner" role="alert">
          <AlertCircle size={17} />
          <span>{t(error)}</span>
          <button aria-label={t('关闭错误')} onClick={clearError}>
            <X size={15} />
          </button>
        </div>
      )}
      {!data?.environment.adb && (
        <div className="notice">
          <AlertCircle size={20} />
          <div>
            <b>{t('当前 SDK 缺少 platform-tools')}</b>
            <p>{t('安装 Platform-Tools 后，即可打开终端。')}</p>
            <button className="text-button" onClick={openSdk}>
              {t('前往 SDK 管理')}
            </button>
          </div>
        </div>
      )}
      {!sessions.length ? (
        <div className="terminal-empty">
          <Terminal size={32} />
          <h2>{t('开始一个 ADB 会话')}</h2>
          <p>{t('选择设备，直接进入 ADB Shell；也可以打开通用命令行。')}</p>
          <TerminalNewSession data={data} />
          <small>{data?.settings.sdkRoot}</small>
        </div>
      ) : (
        <>
          <div className="terminal-tabs" role="group" aria-label={t('终端会话')}>
            {sessions.map((session) => (
              <div
                key={session.id}
                className={`terminal-tab ${session.id === activeId ? 'selected' : ''}`}
              >
                <button
                  className="terminal-tab-select"
                  aria-pressed={session.id === activeId}
                  onClick={() => select(session.id)}
                >
                  <span className={`terminal-dot ${session.state === 'active' ? 'online' : ''}`} />
                  {t(session.label)}
                </button>
                <button
                  className="icon-button"
                  aria-label={t('结束 {0} 会话', t(session.label))}
                  onClick={() => closeTerminal(session.id)}
                >
                  <X size={13} />
                </button>
              </div>
            ))}
            <TerminalNewSession data={data} compact />
          </div>
          {active && (
            <>
              <div className="terminal-context">
                <span className="terminal-target-icon">
                  {active.kind === 'device' ? <Smartphone size={18} /> : <Monitor size={18} />}
                </span>
                <div>
                  <b>
                    {active.kind === 'device'
                      ? t('设备 · {0}', active.label)
                      : t('通用命令行 · 本机 PowerShell')}
                  </b>
                  <small>{active.target?.serial || active.sdkRoot || data?.settings.sdkRoot}</small>
                </div>
                <span className={`terminal-state ${active.state}`}>
                  {t(stateText[active.state] ?? active.state)}
                </span>
              </div>
              {ended && (
                <div className="terminal-ended">
                  <AlertCircle size={16} />
                  <span>
                    {active.reason
                      ? t(active.reason)
                      : active.kind === 'device'
                        ? t('设备 Shell 已退出，输出已保留。')
                        : t('命令行已退出，输出已保留。')}
                  </span>
                  <Button
                    disabled={
                      !native ||
                      !data?.environment.adb ||
                      (active.kind === 'device' && !freshDevice)
                    }
                    onClick={() =>
                      void reopenTerminal(active.id, freshDevice).catch(reportTerminalError)
                    }
                  >
                    {active.state === 'disconnected'
                      ? t('重新连接')
                      : active.kind === 'device'
                        ? t('重新打开 Shell')
                        : t('重新打开')}
                  </Button>
                </div>
              )}
              <div className="terminal-toolbar">
                <span>
                  <Terminal size={14} />
                  {active.kind === 'device' ? 'Android Shell' : 'PowerShell'}
                </span>
                <div>
                  <button
                    className="text-button"
                    disabled={active.state !== 'active'}
                    aria-expanded={snippets}
                    onClick={() => setSnippets(!snippets)}
                  >
                    <List size={14} />
                    {t('常用命令')}
                    <ChevronDown size={12} />
                  </button>
                  <button className="text-button" onClick={() => clearTerminal(active.id)}>
                    <Eraser size={14} />
                    {t('清空显示')}
                  </button>
                  <button
                    className="text-button"
                    disabled={active.state !== 'active'}
                    onClick={() => interruptTerminal(active.id)}
                  >
                    {t('中断')}
                  </button>
                </div>
              </div>
              {snippets && (
                <div className="terminal-snippets" role="group" aria-label={t('常用命令')}>
                  {(active.kind === 'device'
                    ? ['pwd', 'ls', 'getprop ro.build.version.release', 'logcat', 'top']
                    : ['adb devices -l', 'adb version', 'adb shell', 'adb logcat']
                  ).map((command) => (
                    <button
                      key={command}
                      onClick={() => {
                        insertTerminalText(active.id, command);
                        setSnippets(false);
                      }}
                    >
                      {command}
                    </button>
                  ))}
                </div>
              )}
            </>
          )}
          <div className="terminal-views">
            {sessions.map((session) => (
              <TerminalSurface
                key={session.id}
                id={session.id}
                visible={visible && session.id === activeId}
              />
            ))}
          </div>
          {active && (
            <div className="terminal-footnote">
              <span>
                <LockKeyhole size={12} />
                {active.kind === 'device' ? t('设备会话绑定所选设备') : t('通用会话使用本机环境')}
              </span>
              <span>{t('切换页面保留会话与输出')}</span>
            </div>
          )}
        </>
      )}
    </section>
  );
}
