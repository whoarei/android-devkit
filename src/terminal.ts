import { Channel } from '@tauri-apps/api/core';
import { Terminal as Xterm } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { create } from 'zustand';
import { call, deviceKey, errorText, native } from './api';
import { t } from './i18n';
import type { Device, TerminalEvent, TerminalInfo } from './bindings';

type Runtime = {
  info: TerminalInfo;
  title: string;
  origin?: Device;
  terminal: Xterm;
  fit: FitAddon;
  element: HTMLDivElement;
  opened: boolean;
  closed: boolean;
  creating: boolean;
  generation: string;
  sequence: number;
  input: Uint8Array[];
  pendingBytes: number;
  pumping: boolean;
  ack: number;
  ackTimer?: ReturnType<typeof setTimeout>;
  channel?: Channel<TerminalEvent>;
};
const owner = crypto.randomUUID();
const runtimes = new Map<string, Runtime>();
let initialized: Promise<void> | undefined;

export const useTerminals = create<{
  sessions: TerminalInfo[];
  activeId: string;
  error: string;
  select: (id: string) => void;
  clearError: () => void;
}>((set) => ({
  sessions: [],
  activeId: '',
  error: '',
  select: (activeId) => {
    set({ activeId });
    requestAnimationFrame(() => focusTerminal(activeId));
  },
  clearError: () => set({ error: '' }),
}));

export function reportTerminalError(error: unknown) {
  useTerminals.setState({ error: errorText(error) });
}
function publish(runtime: Runtime) {
  if (runtime.closed) return;
  useTerminals.setState((state) => ({
    sessions: state.sessions.map((info) =>
      info.id === runtime.info.id ? { ...runtime.info, label: runtime.title } : info,
    ),
  }));
}
export function initializeTerminals() {
  if (!native) return Promise.resolve();
  initialized ??= call<TerminalInfo[]>('terminal_list', { owner }).then(() => {});
  return initialized;
}
function applyTheme(runtime: Runtime) {
  const dark = document.documentElement.dataset.theme === 'dark';
  runtime.terminal.options.theme = {
    background: dark ? '#0d1912' : '#11231b',
    foreground: '#dbe9de',
    cursor: '#a4dab5',
    cursorAccent: '#11231b',
    selectionBackground: '#72998260',
    black: '#15251c',
    red: '#ed9285',
    green: '#a4dab5',
    yellow: '#e9c98b',
    blue: '#9ebde5',
    magenta: '#c5a8d9',
    cyan: '#9bd5d1',
    white: '#dbe9de',
    brightBlack: '#91a997',
    brightRed: '#ffc3b9',
    brightGreen: '#c1edcd',
    brightYellow: '#f9e1b4',
    brightBlue: '#bfd7fa',
    brightMagenta: '#e5caf7',
    brightCyan: '#bcf0eb',
    brightWhite: '#f0f8ef',
  };
}
export function themeTerminals() {
  for (const runtime of runtimes.values()) applyTheme(runtime);
}
export function focusTerminal(id: string) {
  runtimes.get(id)?.terminal.focus();
}
export function clearTerminal(id: string) {
  runtimes.get(id)?.terminal.clear();
}
export function insertTerminalText(id: string, text: string) {
  const runtime = runtimes.get(id);
  if (!runtime || runtime.info.state !== 'active') return;
  // Pasting is handled by the actual shell, including bracketed paste mode.
  runtime.terminal.paste(text);
  runtime.terminal.focus();
}
export function interruptTerminal(id: string) {
  const runtime = runtimes.get(id);
  if (!runtime) return;
  // Control keys must bypass bracketed paste, just like keyboard Ctrl+C.
  input(runtime, Uint8Array.of(3));
  runtime.terminal.focus();
}

async function pump(runtime: Runtime) {
  if (runtime.pumping) return;
  runtime.pumping = true;
  const generation = runtime.generation;
  try {
    while (runtime.input.length && !runtime.closed && runtime.generation === generation) {
      const data = runtime.input[0];
      try {
        await call('terminal_write', {
          id: runtime.info.id,
          generation,
          sequence: runtime.sequence + 1,
          data: Array.from(data),
        });
      } catch (error) {
        if (
          typeof error === 'object' &&
          error &&
          'code' in error &&
          error.code === 'TERMINAL_INPUT_BUSY'
        ) {
          await new Promise((resolve) => setTimeout(resolve, 25));
          continue;
        }
        throw error;
      }
      if (runtime.closed || runtime.generation !== generation) break;
      runtime.sequence++;
      runtime.input.shift();
      runtime.pendingBytes -= data.byteLength;
    }
  } catch (error) {
    if (!runtime.closed && runtime.generation === generation) {
      reportTerminalError(error);
      runtime.input = [];
      runtime.pendingBytes = 0;
    }
  } finally {
    runtime.pumping = false;
    if (runtime.input.length && !runtime.closed) void pump(runtime);
  }
}
function input(runtime: Runtime, bytes: Uint8Array) {
  if (runtime.closed || runtime.info.state !== 'active' || !bytes.length) return;
  if (runtime.pendingBytes + bytes.length > 256 * 1024) {
    reportTerminalError(t('终端输入过长，请分段粘贴'));
    return;
  }
  for (let offset = 0; offset < bytes.length; offset += 16384)
    runtime.input.push(bytes.slice(offset, offset + 16384));
  runtime.pendingBytes += bytes.length;
  void pump(runtime);
}
function acknowledge(runtime: Runtime, generation: string, offset: number) {
  if (runtime.closed || runtime.generation !== generation) return;
  runtime.ack = Math.max(runtime.ack, offset);
  runtime.ackTimer ??= setTimeout(() => {
    runtime.ackTimer = undefined;
    if (runtime.closed || runtime.generation !== generation) return;
    void call('terminal_ack', { id: runtime.info.id, generation, offset: runtime.ack }).catch(
      () => {},
    );
  }, 16);
}
function receive(runtime: Runtime, event: TerminalEvent) {
  if (runtime.closed) return;
  if (!runtime.generation && runtime.creating) runtime.generation = event.generation;
  if (runtime.generation !== event.generation) return;
  if (event.kind === 'data') {
    // ConPTY can ask for the cursor position before create resolves. Its
    // terminal-generated replies must already be allowed through the PTY.
    if (runtime.info.state === 'starting') {
      runtime.info = { ...runtime.info, state: 'active' };
      runtime.terminal.options.disableStdin = false;
      publish(runtime);
    }
    runtime.terminal.write(Uint8Array.from(event.data), () =>
      acknowledge(runtime, event.generation, event.offset),
    );
  } else {
    runtime.info = {
      ...runtime.info,
      state: event.state,
      reason: event.reason,
      exitCode: event.exitCode,
    };
    runtime.input = [];
    runtime.pendingBytes = 0;
    runtime.terminal.options.disableStdin = true;
    publish(runtime);
  }
}
function makeRuntime(device?: Device, label?: string): Runtime {
  const terminal = new Xterm({
    fontFamily: 'Consolas, "Cascadia Mono", "Microsoft YaHei", monospace',
    fontSize: 13,
    lineHeight: 1.45,
    cursorBlink: true,
    scrollback: 5000,
    allowProposedApi: false,
    disableStdin: true,
    convertEol: false,
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  const runtime: Runtime = {
    info: {
      id: crypto.randomUUID(),
      generation: '',
      kind: device ? 'device' : 'host',
      label: label || device?.model || device?.serial || '通用命令行',
      target: device
        ? {
            serial: device.serial,
            transportId: device.transportId,
            serverEpoch: device.serverEpoch,
          }
        : null,
      sdkRoot: '',
      state: 'starting',
      reason: '',
      exitCode: null,
    },
    title: label || device?.model || device?.serial || '通用命令行',
    origin: device,
    terminal,
    fit,
    element: document.createElement('div'),
    opened: false,
    closed: false,
    creating: false,
    generation: '',
    sequence: 0,
    input: [],
    pendingBytes: 0,
    pumping: false,
    ack: 0,
  };
  runtime.element.className = 'terminal-emulator';
  applyTheme(runtime);
  terminal.onData((data) => input(runtime, new TextEncoder().encode(data)));
  terminal.onBinary((data) =>
    input(
      runtime,
      Uint8Array.from(data, (char) => char.charCodeAt(0) & 255),
    ),
  );
  terminal.attachCustomKeyEventHandler((event) => {
    if (
      (event.ctrlKey || event.metaKey) &&
      event.key.toLowerCase() === 'c' &&
      terminal.hasSelection()
    ) {
      if (event.type === 'keydown') {
        event.preventDefault();
        void navigator.clipboard.writeText(terminal.getSelection()).catch(reportTerminalError);
      }
      return false;
    }
    return true;
  });
  return runtime;
}
async function connect(runtime: Runtime, device?: Device) {
  runtime.creating = true;
  runtime.info = {
    ...runtime.info,
    generation: '',
    state: 'starting',
    reason: '',
    exitCode: null,
    target: device
      ? { serial: device.serial, transportId: device.transportId, serverEpoch: device.serverEpoch }
      : null,
  };
  runtime.generation = '';
  runtime.sequence = 0;
  runtime.input = [];
  runtime.pendingBytes = 0;
  runtime.ack = 0;
  clearTimeout(runtime.ackTimer);
  runtime.ackTimer = undefined;
  runtime.terminal.options.disableStdin = true;
  publish(runtime);
  const channel = new Channel<TerminalEvent>();
  runtime.channel = channel;
  channel.onmessage = (event) => {
    if (runtime.channel === channel) receive(runtime, event);
  };
  try {
    await initializeTerminals();
    if (runtime.closed) return;
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    if (runtime.closed) return;
    const info = await call<TerminalInfo>('terminal_create', {
      id: runtime.info.id,
      owner,
      kind: runtime.info.kind,
      target: runtime.info.target,
      cols: Math.min(500, Math.max(2, runtime.terminal.cols)),
      rows: Math.min(250, Math.max(1, runtime.terminal.rows)),
      onEvent: channel,
    });
    if (runtime.closed) {
      await call('terminal_close', { id: info.id, generation: info.generation });
      return;
    }
    runtime.generation = info.generation;
    const earlyState = runtime.info.state !== 'starting' ? runtime.info.state : info.state;
    runtime.info = {
      ...info,
      state: earlyState,
      reason: runtime.info.reason || info.reason,
      exitCode: runtime.info.exitCode ?? info.exitCode,
    };
    runtime.terminal.options.disableStdin = runtime.info.state !== 'active';
    fitTerminal(runtime.info.id);
    publish(runtime);
    runtime.terminal.focus();
  } catch (error) {
    if (!runtime.closed) {
      runtime.info = { ...runtime.info, state: 'failed', reason: errorText(error) };
      publish(runtime);
      throw error;
    }
  } finally {
    runtime.creating = false;
  }
}
export async function newTerminal(device?: Device, label?: string) {
  if (useTerminals.getState().sessions.length >= 8)
    throw new Error(t('最多保留 8 个终端会话，请先关闭一个会话'));
  const runtime = makeRuntime(device, label);
  runtimes.set(runtime.info.id, runtime);
  useTerminals.setState((state) => ({
    sessions: [...state.sessions, runtime.info],
    activeId: runtime.info.id,
    error: '',
  }));
  await connect(runtime, device);
}
export async function openDeviceTerminal(device: Device, label?: string) {
  const existing = [...runtimes.values()].find(
    (runtime) => runtime.origin && deviceKey(runtime.origin) === deviceKey(device),
  );
  if (existing) {
    useTerminals.getState().select(existing.info.id);
    if (!existing.creating && existing.info.state !== 'active')
      await reopenTerminal(existing.info.id, device);
  } else await newTerminal(device, label);
}
export async function reopenTerminal(id: string, device?: Device) {
  const runtime = runtimes.get(id);
  if (!runtime || runtime.closed || runtime.creating) return;
  const generation = runtime.generation;
  runtime.creating = true;
  runtime.channel = undefined;
  runtime.generation = ''; // Reject late output from the previous connection immediately.
  runtime.info = { ...runtime.info, state: 'starting' };
  publish(runtime);
  runtime.terminal.options.disableStdin = true;
  try {
    if (generation) await call('terminal_close', { id, generation });
  } catch (error) {
    runtime.creating = false;
    runtime.info = { ...runtime.info, state: 'failed', reason: errorText(error) };
    publish(runtime);
    throw error;
  }
  if (runtime.closed) {
    runtime.creating = false;
    return;
  }
  runtime.origin = device;
  runtime.terminal.writeln('\r\n──────── ' + t('重新连接') + ' ────────\r\n');
  await connect(runtime, device);
}
export function closeTerminal(id: string) {
  const runtime = runtimes.get(id);
  if (!runtime) return;
  runtime.closed = true;
  clearTimeout(runtime.ackTimer);
  runtime.input = [];
  runtime.terminal.dispose();
  runtimes.delete(id);
  useTerminals.setState((state) => {
    const index = state.sessions.findIndex((info) => info.id === id);
    const sessions = state.sessions.filter((info) => info.id !== id);
    return {
      sessions,
      activeId:
        state.activeId === id
          ? sessions[Math.min(index, sessions.length - 1)]?.id || ''
          : state.activeId,
    };
  });
  if (runtime.generation)
    void call('terminal_close', { id, generation: runtime.generation }).catch(reportTerminalError);
}
export function attachTerminal(id: string, host: HTMLElement) {
  const runtime = runtimes.get(id);
  if (!runtime) return;
  host.appendChild(runtime.element);
  if (!runtime.opened) {
    runtime.terminal.open(runtime.element);
    runtime.opened = true;
  }
}
export function fitTerminal(id: string) {
  const runtime = runtimes.get(id);
  if (
    !runtime?.opened ||
    runtime.closed ||
    runtime.element.clientWidth < 30 ||
    runtime.element.clientHeight < 30
  )
    return;
  runtime.fit.fit();
  const { cols, rows } = runtime.terminal;
  if (runtime.generation && runtime.info.state === 'active') {
    void call('terminal_resize', {
      id,
      generation: runtime.generation,
      cols: Math.min(500, Math.max(2, cols)),
      rows: Math.min(250, Math.max(1, rows)),
    }).catch(() => {});
  }
}
export function resolveTerminalDevice(id: string, devices: Device[]) {
  const origin = runtimes.get(id)?.origin;
  if (!origin) return undefined;
  const matching = devices.filter(
    (device) =>
      device.serial === origin.serial &&
      device.avdName === origin.avdName &&
      device.state === 'ready',
  );
  return matching.length === 1 ? matching[0] : undefined;
}
