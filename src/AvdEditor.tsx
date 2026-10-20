import { t } from './i18n';
import { useId, useState } from 'react';
import { call, errorText } from './api';
import { Button, Field, Modal } from './ui';
import type { Avd } from './bindings';
export type AvdForm = Record<string, string>;
const mib = (value: string) => {
  const match = value?.trim().match(/^(\d+(?:\.\d+)?)([GMKB]?)$/i);
  if (!match) return '';
  const number = Number(match[1]);
  return String(
    Math.round(
      number *
        ({ G: 1024, M: 1, K: 1 / 1024, B: 1 / 1024 ** 2, '': 1 / 1024 ** 2 }[
          match[2].toUpperCase()
        ] ?? 1),
    ),
  );
};
export function newAvdSettings(): AvdForm {
  return {
    memory: '2048',
    cpuCores: '2',
    vmHeap: '256',
    gpu: 'auto',
    screenWidth: '1080',
    screenHeight: '1920',
    density: '420',
    orientation: 'portrait',
    cameraFront: 'none',
    cameraBack: 'emulated',
    networkSpeed: 'full',
    networkLatency: 'none',
    bootMode: 'quick',
    storageMiB: '8192',
    sdCardMode: 'none',
    sdCardMiB: '512',
    sdCardPath: '',
    skinMode: 'none',
    skinPath: '',
    showDeviceFrame: 'no',
    keyboard: 'yes',
    audioInput: 'yes',
    battery: 'yes',
    gps: 'yes',
  };
}
export function avdForm(avd: Avd): AvdForm {
  const config = avd.config ?? {};
  const mapping: Record<string, string> = {
    cpuCores: 'hw.cpu.ncore',
    vmHeap: 'vm.heapSize',
    screenWidth: 'hw.lcd.width',
    screenHeight: 'hw.lcd.height',
    density: 'hw.lcd.density',
    orientation: 'hw.initialOrientation',
    cameraFront: 'hw.camera.front',
    cameraBack: 'hw.camera.back',
    networkSpeed: 'runtime.network.speed',
    networkLatency: 'runtime.network.latency',
    keyboard: 'hw.keyboard',
    audioInput: 'hw.audioInput',
    battery: 'hw.battery',
    gps: 'hw.gps',
    showDeviceFrame: 'showDeviceFrame',
  };
  return {
    ...Object.fromEntries(
      Object.entries(mapping).map(([key, source]) => [key, config[source] ?? '']),
    ),
    displayName: avd.displayName,
    memory: avd.memory,
    gpu: avd.gpu,
    storageMiB: mib(avd.disk),
    bootMode: config['fastboot.forceColdBoot'] === 'yes' ? 'cold' : 'quick',
    skinMode: 'keep',
    skinPath: config['skin.path'] || '',
    sdCardMode: 'keep',
    sdCardMiB: mib(config['sdcard.size'] || '') || '512',
    sdCardPath: config['sdcard.path'] || '',
  };
}
function StorageField({
  label,
  field,
  values,
  set,
  hint,
}: {
  label: string;
  field: string;
  values: AvdForm;
  set: (key: string, value: string) => void;
  hint?: string;
}) {
  const [unit, setUnit] = useState(
    values[field] && Number(values[field]) % 1024 === 0 ? 'GiB' : 'MiB',
  );
  const amount = values[field] ? String(Number(values[field]) / (unit === 'GiB' ? 1024 : 1)) : '';
  return (
    <Field label={label} hint={hint}>
      <div className="storage-input">
        <input
          aria-label={label}
          type="number"
          min={0}
          step={unit === 'GiB' ? '0.5' : '1'}
          value={amount}
          onChange={(event) =>
            set(
              field,
              event.target.value === ''
                ? ''
                : String(Math.round(Number(event.target.value) * (unit === 'GiB' ? 1024 : 1))),
            )
          }
        />
        <select
          aria-label={t('{0}单位', label)}
          value={unit}
          onChange={(event) => setUnit(event.target.value)}
        >
          <option>MiB</option>
          <option>GiB</option>
        </select>
      </div>
    </Field>
  );
}
export function AvdSettingsFields({
  values,
  set,
  creating = false,
}: {
  values: AvdForm;
  set: (key: string, value: string) => void;
  creating?: boolean;
}) {
  const id = useId();
  const [error, setError] = useState('');
  const number = (label: string, key: string, min: number, max: number) => (
    <Field label={label}>
      <input
        type="number"
        min={min}
        max={max}
        value={values[key] ?? ''}
        placeholder={t('保留原值 / 默认值')}
        onChange={(event) => set(key, event.target.value)}
      />
    </Field>
  );
  const select = (label: string, key: string, options: [string, string][]) => (
    <Field label={label}>
      <select
        value={key === 'showDeviceFrame' && values.skinMode === 'none' ? 'no' : (values[key] ?? '')}
        disabled={key === 'showDeviceFrame' && values.skinMode === 'none'}
        onChange={(event) => set(key, event.target.value)}
      >
        <option value="" disabled={Boolean(values[key])}>
          {t('保留默认值')}
        </option>
        {values[key] && !options.some(([value]) => value === values[key]) && (
          <option value={values[key]}>
            {t('当前：')}
            {values[key]}
          </option>
        )}
        {options.map(([value, title]) => (
          <option value={value} key={value}>
            {title}
          </option>
        ))}
      </select>
    </Field>
  );
  const presets = (label: string, key: string, options: [string, string][], hint: string) => (
    <Field label={label} hint={hint}>
      <input
        list={`${id}-${key}`}
        value={values[key] ?? ''}
        placeholder={t('留空保留原值')}
        onChange={(event) => set(key, event.target.value)}
      />
      <datalist id={`${id}-${key}`}>
        {options.map(([value, title]) => (
          <option key={value} value={value} label={title} />
        ))}
      </datalist>
    </Field>
  );
  const choose = async (key: string, kind: string) => {
    setError('');
    try {
      const path = await call<string | null>('file_choose', { kind });
      if (path) set(key, path);
    } catch (error) {
      setError(errorText(error));
    }
  };
  const cameras: [string, string][] = [
    ['none', t('关闭')],
    ['emulated', t('软件模拟')],
    ['virtualscene', t('虚拟场景')],
    ['webcam0', t('本机摄像头 0')],
    ['webcam1', t('本机摄像头 1')],
  ];
  return (
    <div className="avd-settings">
      <details open>
        <summary>{t('性能与显示')}</summary>
        <div className="form-grid">
          {number(t('CPU 核心数'), 'cpuCores', 1, 16)}
          {number(t('内存（MiB）'), 'memory', 512, 16384)}
          {number('VM Heap（MiB）', 'vmHeap', 16, 2048)}
          {select(t('图形模式'), 'gpu', [
            ['auto', t('自动')],
            ['host', t('主机 GPU')],
            ['software', t('软件渲染')],
          ])}
          {number(t('屏幕宽度（px）'), 'screenWidth', 240, 8192)}
          {number(t('屏幕高度（px）'), 'screenHeight', 240, 8192)}
          {number(t('屏幕密度（dpi）'), 'density', 72, 1000)}
          {select(t('启动方向'), 'orientation', [
            ['portrait', t('竖屏')],
            ['landscape', t('横屏')],
          ])}
        </div>
      </details>
      <details>
        <summary>{t('摄像头与网络')}</summary>
        <div className="form-grid">
          {presets(
            t('前置摄像头'),
            'cameraFront',
            cameras.filter(([value]) => value !== 'virtualscene'),
            t('none / emulated / webcam 编号'),
          )}
          {presets(t('后置摄像头'), 'cameraBack', cameras, t('webcam 编号须对应本机可用摄像头'))}
          {presets(
            t('网络速度'),
            'networkSpeed',
            [
              ['full', t('不限速')],
              ['gsm', 'GSM'],
              ['gprs', 'GPRS'],
              ['edge', 'EDGE'],
              ['umts', '3G'],
              ['hsdpa', 'HSDPA'],
              ['lte', 'LTE'],
              ['5g', '5G'],
            ],
            t('预设或 kbit/s 数值，例 1024:2048（上传:下载）'),
          )}
          {presets(
            t('网络延迟'),
            'networkLatency',
            [
              ['none', t('无延迟')],
              ['gsm', 'GSM'],
              ['gprs', 'GPRS'],
              ['edge', 'EDGE'],
              ['umts', '3G'],
            ],
            t('预设或毫秒数，例 20:100（最小:最大）'),
          )}
        </div>
      </details>
      <details>
        <summary>{t('启动与存储')}</summary>
        <div className="form-grid">
          {select(t('默认启动方式'), 'bootMode', [
            ['quick', t('快速启动')],
            ['cold', t('冷启动')],
          ])}
          <StorageField
            label={t('数据分区目标容量')}
            field="storageMiB"
            values={values}
            set={set}
            hint={t('2～128 GiB，不自动清除已有用户数据。已有分区是否扩容由模拟器处理。')}
          />
          {select(t('扩展存储'), 'sdCardMode', [
            ...(!creating ? [['keep', t('保留现有扩展存储')] as [string, string]] : []),
            ['none', t('不使用')],
            ['size', t('创建新的扩展存储镜像')],
            ['external', t('使用现有镜像文件')],
          ])}
          {values.sdCardMode === 'size' && (
            <StorageField
              label={t('扩展存储容量')}
              field="sdCardMiB"
              values={values}
              set={set}
              hint={t('64 MiB～32 GiB；创建新镜像，原镜像保留。')}
            />
          )}
        </div>
        {values.sdCardMode === 'external' && (
          <Field
            label={t('扩展存储镜像路径')}
            hint={t('选择现有 .img 文件；配置变更不会格式化该文件。')}
          >
            <div className="input-action">
              <input
                value={values.sdCardPath}
                onChange={(event) => set('sdCardPath', event.target.value)}
              />
              <Button onClick={() => void choose('sdCardPath', 'sdcard')}>{t('选择镜像')}</Button>
            </div>
          </Field>
        )}
        {values.sdCardMode === 'keep' && values.sdCardPath && (
          <p className="hint path">
            {t('当前：')}
            {values.sdCardPath}
          </p>
        )}
      </details>
      <details>
        <summary>{t('设备皮肤与边框')}</summary>
        <div className="form-grid">
          {select(t('设备皮肤'), 'skinMode', [
            ...(!creating ? [['keep', t('保留现有皮肤')] as [string, string]] : []),
            ['none', t('无皮肤')],
            ['custom', t('自定义皮肤目录')],
          ])}
          {select(t('显示设备边框'), 'showDeviceFrame', [
            ['yes', t('显示')],
            ['no', t('隐藏')],
          ])}
        </div>
        {values.skinMode === 'custom' && (
          <Field label={t('皮肤目录')} hint={t('目录必须包含 layout 文件；分辨率应与皮肤匹配。')}>
            <div className="input-action">
              <input
                value={values.skinPath}
                onChange={(event) => set('skinPath', event.target.value)}
              />
              <Button onClick={() => void choose('skinPath', 'skin')}>{t('选择目录')}</Button>
            </div>
          </Field>
        )}
      </details>
      <details>
        <summary>{t('输入与设备能力')}</summary>
        <div className="form-grid">
          {select(t('硬件键盘输入'), 'keyboard', [
            ['yes', t('启用')],
            ['no', t('关闭')],
          ])}
          {select(t('麦克风输入'), 'audioInput', [
            ['yes', t('启用')],
            ['no', t('关闭')],
          ])}
          {select(t('电池模拟'), 'battery', [
            ['yes', t('启用')],
            ['no', t('关闭')],
          ])}
          {select('GPS', 'gps', [
            ['yes', t('启用')],
            ['no', t('关闭')],
          ])}
        </div>
      </details>
      {error && (
        <p className="error-banner" role="alert">
          {t(error)}
        </p>
      )}
    </div>
  );
}
export function AvdPreview({
  values,
  title,
  api,
  abi,
}: {
  values: AvdForm;
  title: string;
  api?: string;
  abi?: string;
}) {
  const width = Number(values.screenWidth) || 1080;
  const height = Number(values.screenHeight) || 1920;
  const known = Boolean(values.screenWidth && values.screenHeight);
  const landscape = values.orientation === 'landscape';
  const ratio =
    values.orientation === 'landscape'
      ? Math.max(width, height) / Math.min(width, height)
      : values.orientation === 'portrait'
        ? Math.min(width, height) / Math.max(width, height)
        : width / height;
  return (
    <aside className="avd-preview">
      <strong>{title || t('虚拟设备')}</strong>
      <div
        className="avd-screen"
        style={{ aspectRatio: String(ratio), width: `${Math.min(150, 210 * ratio)}px` }}
      >
        <span>{values.orientation === 'landscape' ? t('横屏') : t('竖屏')}</span>
      </div>
      <dl>
        <dt>{t('分辨率')}</dt>
        <dd>
          {known
            ? `${landscape ? Math.max(width, height) : values.orientation === 'portrait' ? Math.min(width, height) : width} × ${landscape ? Math.min(width, height) : values.orientation === 'portrait' ? Math.max(width, height) : height}`
            : t('沿用硬件配置')}
        </dd>
        <dt>{t('密度')}</dt>
        <dd>{values.density || t('默认')} dpi</dd>
        <dt>API</dt>
        <dd>{api || t('由镜像决定')}</dd>
        <dt>ABI</dt>
        <dd>{abi || 'x86_64'}</dd>
      </dl>
    </aside>
  );
}
export function EditAvd({
  avd,
  close,
  save,
  busy,
  error,
}: {
  avd: Avd;
  close: () => void;
  save: (args: AvdForm) => void;
  busy: boolean;
  error: string;
}) {
  const [initial] = useState(() => ({ values: avdForm(avd), fingerprint: avd.fingerprint }));
  const [values, setValues] = useState(initial.values);
  const set = (key: string, value: string) =>
    setValues((current) => ({ ...current, [key]: value }));
  const conflict = avd.fingerprint !== initial.fingerprint;
  const changed = Object.entries(values).some(([key, value]) => value !== initial.values[key]);
  const submit = () => {
    const args: AvdForm = Object.fromEntries(
      Object.entries(values).filter(([key, value]) => value !== initial.values[key]),
    );
    if (values.sdCardMode === 'size') args.sdCardMiB = values.sdCardMiB;
    if (values.sdCardMode === 'external') args.sdCardPath = values.sdCardPath;
    if (values.skinMode === 'custom') args.skinPath = values.skinPath;
    save({ ...args, fingerprint: initial.fingerprint });
  };
  return (
    <Modal title={t('编辑模拟器配置')} onClose={close} wide className="avd-modal">
      <div className="avd-modal-body">
        <div className="avd-editor-layout">
          <div>
            <Field label={t('显示名称')}>
              <input
                value={values.displayName}
                onChange={(event) => set('displayName', event.target.value)}
              />
            </Field>
            <AvdSettingsFields values={values} set={set} />
          </div>
          <AvdPreview values={values} title={values.displayName} api={avd.api} abi={avd.abi} />
        </div>
        <p className="hint">
          {t(
            '配置在下次启动时生效。内部名称、系统镜像及已有用户数据保留；未修改的配置键保持原值。',
          )}
        </p>
        {(error || conflict) && (
          <p className="error-banner" role="alert">
            {conflict ? t('配置已在其他工具中更新，请关闭后重新打开编辑面板。') : t(error)}
          </p>
        )}
      </div>
      <div className="modal-actions">
        <Button onClick={close}>{t('取消')}</Button>
        <Button primary disabled={busy || conflict || !changed} onClick={submit}>
          {t('查看保存计划')}
        </Button>
      </div>
    </Modal>
  );
}
