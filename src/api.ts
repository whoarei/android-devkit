import { invoke, isTauri } from '@tauri-apps/api/core';
export const native = isTauri();
export function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!native)
    return Promise.reject({
      code: 'DESKTOP_REQUIRED',
      message: '请使用桌面应用连接本机 SDK；浏览器中不执行设备操作。',
    });
  return invoke<T>(command, args);
}
export function errorText(e: unknown): string {
  if (typeof e === 'object' && e !== null && 'message' in e)
    return String((e as { message: string }).message);
  return String(e);
}
export const stateLabel: Record<string, string> = {
  ready: '可用',
  stopped: '已停止',
  booting: '系统启动中',
  offline: '离线',
  unauthorized: '未授权',
  missing_dependency: '缺少依赖',
  busy: '外部占用',
  queued: '排队',
  running: '执行中',
  succeeded: '已完成',
  failed: '失败',
  cancelled: '已取消',
  needs_review: '结果待检查',
  waiting_confirmation: '待确认',
};
export const taskActive = (state: string) =>
  ['queued', 'running', 'waiting_confirmation'].includes(state);
export const deviceKey = (d: { serial: string; transportId: string; serverEpoch: number }) =>
  `device:${d.serverEpoch}:${d.transportId}:${d.serial}`;
export const size = (n: number) => (n ? `${(n / 1024 / 1024).toFixed(1)} MiB` : '待获取');
