import { useEffect, useRef, useState } from 'react';
import { Check, QrCode, RefreshCw, Wifi } from 'lucide-react';
import { call, errorText, taskActive } from './api';
import { t } from './i18n';
import { Button, Field, Modal } from './ui';
import type { Task, WirelessQr, WirelessService } from './bindings';
import './wireless.css';

type Props = {
  close: () => void;
  prepare: (kind: string, args: Record<string, string>) => Promise<void>;
  busy: boolean;
  error: string;
  clearError: () => void;
  tasks: Task[];
  hidden: boolean;
};
type Mode = 'qr' | 'code' | 'manual';
const serviceKey = (service: WirelessService) => `${service.name}|${service.address}`;

export function ConnectDevice(props: Props) {
  const [mode, setMode] = useState<Mode>('qr');
  const [services, setServices] = useState<WirelessService[]>([]);
  const [selected, setSelected] = useState('');
  const [code, setCode] = useState('');
  const [qr, setQr] = useState<WirelessQr | null>(null);
  const [submitted, setSubmitted] = useState<Task | null>(null);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState('');
  const [discoveryError, setDiscoveryError] = useState('');
  const [clock, setClock] = useState(Date.now());
  const [scan, setScan] = useState(0);
  const generation = useRef(0);
  const working = useRef(false);
  const taskRef = useRef<Task | null>(null);
  const requestId = useRef(crypto.randomUUID());
  const task = props.tasks.find((item) => item.id === submitted?.id) ?? submitted;
  const active = !!task && taskActive(task.state);
  taskRef.current = task;
  const selectedService = services.find((service) => serviceKey(service) === selected);
  const remaining = qr ? Math.max(0, qr.expiresAt - Math.floor(clock / 1000)) : 0;

  useEffect(
    () => () => {
      generation.current++;
      const current = taskRef.current;
      if (current && taskActive(current.state))
        void call('task_cancel', { taskId: current.id }).catch(() => {});
    },
    [],
  );

  useEffect(() => {
    if (task?.state === 'succeeded') props.close();
  }, [task?.id, task?.state, props.close]);

  useEffect(() => {
    if (!qr || !active) return;
    const timer = window.setInterval(() => setClock(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [qr, active]);

  useEffect(() => {
    if (mode !== 'code' || active || props.hidden) return;
    let gone = false;
    let timer: ReturnType<typeof setTimeout>;
    const discover = async () => {
      try {
        const found = await call<WirelessService[]>('wireless_discover');
        if (gone) return;
        setServices(found);
        setDiscoveryError('');
      } catch (error) {
        if (gone) return;
        setServices([]);
        setDiscoveryError(errorText(error));
      }
      if (!gone) timer = setTimeout(discover, 2000);
    };
    void discover();
    return () => {
      gone = true;
      clearTimeout(timer);
    };
  }, [mode, active, props.hidden, scan]);

  useEffect(() => {
    if (selected && !selectedService && !active) {
      setSelected('');
      setCode('');
      requestId.current = crypto.randomUUID();
      setFailure('所选设备已停止配对，请重新打开手机配对码页面并选择设备');
    }
  }, [selected, selectedService, active]);

  const changeMode = (next: Mode) => {
    if (working.current || active) return;
    generation.current++;
    setMode(next);
    setCode('');
    setSelected('');
    setServices([]);
    setQr(null);
    setSubmitted(null);
    setFailure('');
    setDiscoveryError('');
    props.clearError();
    requestId.current = crypto.randomUUID();
  };
  const begin = async (kind: 'qr' | 'code') => {
    if (working.current || active) return;
    working.current = true;
    setPending(true);
    setFailure('');
    props.clearError();
    const current = ++generation.current;
    try {
      let next: Task;
      let result: WirelessQr | null = null;
      if (kind === 'qr') {
        result = await call<WirelessQr>('wireless_qr_start');
        next = result.task;
      } else {
        if (!selectedService || !/^\d{6}$/.test(code)) return;
        next = await call<Task>('wireless_pair_start', {
          service: selectedService,
          code,
          requestId: requestId.current,
        });
      }
      if (current !== generation.current) {
        await call('task_cancel', { taskId: next.id });
        return;
      }
      taskRef.current = next;
      setSubmitted(next);
      setQr(result);
      setClock(Date.now());
      setCode('');
      setSelected('');
      requestId.current = crypto.randomUUID();
    } catch (error) {
      if (current === generation.current) setFailure(errorText(error));
    } finally {
      working.current = false;
      if (current === generation.current) setPending(false);
    }
  };
  const cancel = async () => {
    if (!task || working.current) return;
    working.current = true;
    setPending(true);
    setFailure('');
    try {
      await call('task_cancel', { taskId: task.id });
    } catch (error) {
      setFailure(errorText(error));
    } finally {
      working.current = false;
      setPending(false);
    }
  };
  const close = async () => {
    const current = taskRef.current;
    try {
      if (current && taskActive(current.state)) await call('task_cancel', { taskId: current.id });
      props.close();
    } catch (error) {
      setFailure(errorText(error));
    }
  };

  return (
    <Modal
      title={t('连接设备')}
      onClose={() => void close()}
      hidden={props.hidden}
      trapFocus
      className="wireless-dialog"
    >
      <div className="tabs wireless-tabs" aria-label={t('设备连接方式')}>
        {(
          [
            ['qr', '扫码配对'],
            ['code', '配对码'],
            ['manual', '手动连接'],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            aria-pressed={mode === value}
            className={mode === value ? 'selected' : ''}
            disabled={pending || active || props.busy}
            onClick={() => changeMode(value)}
          >
            {t(label)}
          </button>
        ))}
      </div>
      {mode === 'manual' ? (
        <ManualConnection {...props} />
      ) : (
        <>
          <p className="hint">
            {t('手机和电脑连接到可互通的同一局域网，在手机开发者选项中开启无线调试。')}
          </p>
          {mode === 'qr' ? (
            <>
              <p className="hint">{t('在手机选择“使用二维码配对设备”，扫描下方二维码。')}</p>
              <div className="wireless-qr">
                {qr &&
                active &&
                remaining > 0 &&
                ['queued', 'preparing', 'waiting_scan'].includes(task?.phase ?? '') ? (
                  <>
                    <img src={qr.image} alt={t('无线调试配对二维码')} width={240} height={240} />
                    <small>{t('二维码将在 {0} 秒后过期', remaining)}</small>
                  </>
                ) : (
                  <div className="wireless-qr-placeholder">
                    <QrCode size={64} aria-hidden="true" />
                    <p>
                      {t(
                        active
                          ? '正在处理设备连接…'
                          : task?.state === 'succeeded'
                            ? '设备已连接'
                            : '生成二维码后，使用手机扫描即可配对',
                      )}
                    </p>
                  </div>
                )}
              </div>
            </>
          ) : (
            <>
              <p className="hint">
                {t('在手机选择“使用配对码配对设备”，保持 6 位码弹窗打开，然后选择下面的设备。')}
              </p>
              <fieldset className="wireless-services" disabled={pending || active}>
                <legend>{t('可配对设备')}</legend>
                {services.length ? (
                  services.map((service) => (
                    <label key={serviceKey(service)}>
                      <input
                        type="radio"
                        name="wireless-device"
                        checked={selected === serviceKey(service)}
                        onChange={() => {
                          setSelected(serviceKey(service));
                          setCode('');
                          setFailure('');
                          requestId.current = crypto.randomUUID();
                        }}
                      />
                      <Wifi size={18} aria-hidden="true" />
                      <span>
                        <strong>{service.address}</strong>
                        <small>{t('请核对手机配对页的 IP 地址')}</small>
                      </span>
                    </label>
                  ))
                ) : (
                  <p role="status">
                    {t(
                      discoveryError
                        ? '自动发现暂不可用，可使用手动连接'
                        : '正在查找设备，请打开手机的配对码页面…',
                    )}
                  </p>
                )}
              </fieldset>
              {discoveryError && (
                <p className="plan-warning" role="alert">
                  {t(discoveryError)}
                </p>
              )}
              <Field label={t('6 位配对码')} hint={t('配对码仅用于本次配对，不会保存。')}>
                <input
                  inputMode="numeric"
                  autoComplete="off"
                  maxLength={6}
                  value={code}
                  disabled={!selectedService || pending || active}
                  onChange={(event) => {
                    setCode(event.target.value.replace(/\D/g, '').slice(0, 6));
                    requestId.current = crypto.randomUUID();
                  }}
                />
              </Field>
              <p className="hint">
                {t('未发现设备？请检查网络隔离、防火墙或 VPN，也可切换到手动连接。')}
              </p>
            </>
          )}
          {task && (
            <div className={`wireless-result ${task.state}`} role="status" aria-live="polite">
              {task.state === 'succeeded' ? (
                <Check size={18} />
              ) : active ? (
                <RefreshCw size={18} className="spin" />
              ) : (
                <Wifi size={18} />
              )}
              <span>{t(task.message)}</span>
            </div>
          )}
          {failure && (
            <p className="plan-warning" role="alert">
              {t(failure)}
            </p>
          )}
          <div className="modal-actions">
            <Button onClick={() => void close()}>
              {t(active ? '停止并关闭' : '关闭连接面板')}
            </Button>
            {active ? (
              <Button disabled={pending || !!task.cancelRequested} onClick={() => void cancel()}>
                {t(task.cancelRequested ? '正在取消' : '取消配对')}
              </Button>
            ) : mode === 'qr' ? (
              <Button primary loading={pending} onClick={() => void begin('qr')}>
                {t(qr ? '重新生成二维码' : '生成配对二维码')}
              </Button>
            ) : (
              <>
                <Button
                  disabled={pending}
                  onClick={() => {
                    setScan((value) => value + 1);
                    setFailure('');
                  }}
                >
                  {t('重新扫描')}
                </Button>
                <Button
                  primary
                  loading={pending}
                  disabled={!selectedService || !/^\d{6}$/.test(code)}
                  onClick={() => void begin('code')}
                >
                  {t('配对并连接')}
                </Button>
              </>
            )}
          </div>
        </>
      )}
    </Modal>
  );
}

function ManualConnection({
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
    <div className="wireless-manual">
      <div className="tabs" aria-label={t('设备连接方式')}>
        {(
          [
            ['pair', t('手动配对')],
            ['connect', t('已配对连接')],
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
          {t('配对后会自动尝试连接。取消配对需在手机的无线调试设置中选择“忘记”。')}
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
    </div>
  );
}
