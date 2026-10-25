import { useEffect, useState } from 'react';
import { getPreference, savePreference, usePreferenceValue } from './preferences';
import { t } from './i18n';
import { call, deviceKey, errorText } from './api';
import { Button } from './ui';
import type { Avd, Device, Workspace } from './types';
import type { Usage } from './bindings';

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
