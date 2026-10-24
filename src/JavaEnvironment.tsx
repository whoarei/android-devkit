import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { call, errorText, native, size, taskActive, stateLabel } from './api';
import { t } from './i18n';
import { Button, Field, Modal } from './ui';
import type { JavaCheck, JavaEnvironment, Plan, Settings, Task, TaskPage } from './bindings';
import './java-environment.css';

const sourceName = (source: string) =>
  t(
    (
      {
        configured: '当前配置',
        private: '应用私有',
        studio: 'Android Studio',
        JAVA_HOME: 'JAVA_HOME',
        PATH: 'PATH',
      } as Record<string, string>
    )[source] || source,
  );

export function JavaStatus({ settings, open }: { settings?: Settings; open: () => void }) {
  const query = useQuery({
    queryKey: ['java-check', settings?.sdkRoot, settings?.javaPath],
    queryFn: () => call<JavaCheck>('java_check'),
    enabled: native && !!settings,
    staleTime: 30_000,
  });
  return (
    <div className={`java-status ${query.data?.status === 'ready' ? 'ready' : ''}`}>
      <div>
        <b>
          {t('Java 运行环境')} ·{' '}
          {query.isFetching
            ? t('正在检测…')
            : query.data?.status === 'ready'
              ? `Java ${query.data.version}`
              : t('待配置')}
        </b>
        <p className="hint">
          {query.data
            ? t(query.data.message)
            : query.error
              ? t('Java 检测失败，可打开配置面板重试')
              : t('SDK 安装前会检查 Java，浏览和组件选择不受影响。')}
        </p>
      </div>
      <Button onClick={open}>
        {query.data?.status === 'ready' ? t('管理 Java') : t('配置 Java')}
      </Button>
    </div>
  );
}

export function JavaConfiguration({
  resume,
  close,
  done,
  refresh,
  showNetwork,
}: {
  resume: boolean;
  close: () => void;
  done: () => Promise<void>;
  refresh: () => Promise<unknown>;
  showNetwork: () => void;
}) {
  const environment = useQuery({
    queryKey: ['java-environment'],
    queryFn: () => call<JavaEnvironment>('java_environment'),
    staleTime: 0,
  });
  const [mode, setMode] = useState<'private' | 'local'>('private');
  const [path, setPath] = useState('');
  const [checked, setChecked] = useState<JavaCheck | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [taskId, setTaskId] = useState('');
  const [complete, setComplete] = useState(false);
  const initialized = useRef(false);
  const revision = useRef(0);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const taskQuery = useQuery({
    queryKey: ['java-preparation-task', taskId],
    enabled: !!taskId,
    queryFn: () =>
      call<TaskPage>('task_list', {
        query: { state: 'all', keyword: taskId, offset: 0, limit: 10 },
      }),
    refetchInterval: (query) => {
      const task = query.state.data?.tasks.find((task) => task.id === taskId);
      return task && !taskActive(task.state) ? false : 1000;
    },
  });
  const task = taskQuery.data?.tasks.find((task) => task.id === taskId);
  const running = !!taskId && (!task || taskActive(task.state));
  useEffect(() => {
    if (!initialized.current && environment.data && !environment.isFetching) {
      initialized.current = true;
      const current = environment.data.current;
      const candidate =
        current.status === 'ready'
          ? current
          : environment.data.candidates.find((item) => item.status === 'ready');
      if (candidate) {
        setMode('local');
        setPath(candidate.path);
        setChecked(candidate);
      }
      if (environment.data.preparation) {
        setTaskId(environment.data.preparation.id);
        setAccepted(true);
      }
    }
  }, [environment.data, environment.isFetching]);
  useEffect(() => {
    if (task?.state === 'succeeded') {
      setComplete(true);
      void refresh();
    }
  }, [task?.state]);
  const guard = async (work: () => Promise<void>) => {
    setBusy(true);
    setError('');
    try {
      await work();
    } catch (e) {
      if (alive.current) setError(errorText(e));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  const changePath = (value: string) => {
    revision.current++;
    setPath(value);
    setChecked(null);
    setError('');
  };
  const validate = () =>
    guard(async () => {
      const current = revision.current;
      const result = await call<JavaCheck>('java_check', { path });
      if (alive.current && revision.current === current) {
        setChecked(result);
        if (result.status !== 'ready') setError(result.message);
      }
    });
  const install = () =>
    guard(async () => {
      const plan = await call<Plan>('plan_create', {
        operation: { kind: 'java_prepare', args: {}, target: null },
      });
      const license = environment.data?.license;
      if (!license || !accepted || plan.licenses.some((item) => item.hash !== license.hash)) {
        setAccepted(false);
        await environment.refetch();
        throw new Error(t('运行时许可已变化，请重新查看并确认'));
      }
      const created = await call<Task>('task_submit', {
        planId: plan.id,
        planHash: plan.hash,
        confirmationToken: plan.confirmationToken,
        requestId: crypto.randomUUID(),
        acceptedLicenses: [license.hash],
      });
      setTaskId(created.id);
      setComplete(false);
      await refresh();
    });
  const apply = () =>
    guard(async () => {
      await call<JavaCheck>('java_apply', { path });
      await refresh();
      await done();
    });
  return (
    <Modal
      title={t('配置 Java 运行环境')}
      onClose={close}
      className="java-dialog"
      dismissible={!busy}
      trapFocus
    >
      <p className="hint">
        {t('仅用于本应用的 SDK 工具，不修改系统 PATH、JAVA_HOME 或 Android Studio 设置。')}
      </p>
      {resume && <p className="java-context">{t('组件选择已保留，配置完成后返回原安装计划。')}</p>}
      {(error || environment.error || taskQuery.error) && (
        <p className="error-banner" role="alert">
          {t(error || errorText(environment.error || taskQuery.error))}
        </p>
      )}
      {taskId ? (
        <>
          <div className="java-task" role="status">
            <b>
              {t('准备 Java 运行时')} ·{' '}
              {t(stateLabel[task?.state || 'queued'] || task?.state || 'queued')}
            </b>
            <p>{task ? t(task.message) : t('正在读取任务进度…')}</p>
            <progress
              max={100}
              value={task?.progress ?? undefined}
              aria-label={t('Java 准备进度')}
            />
            <small>{t('下载 → 校验与解压 → 启动验证 → 保存配置')}</small>
          </div>
          {task && !taskActive(task.state) && task.state !== 'succeeded' && (
            <>
              <p className="hint">
                {t('SDK 安装尚未开始。可重试 Java 下载、检查代理，或使用已有 Java。')}
              </p>
              <div className="java-inline-actions">
                <Button
                  onClick={() => {
                    setTaskId('');
                    setMode('local');
                  }}
                >
                  {t('使用已有 Java')}
                </Button>
                <Button onClick={showNetwork}>{t('检查代理设置')}</Button>
              </div>
            </>
          )}
          <div className="modal-actions">
            {running ? (
              <>
                <Button
                  disabled={busy || !task?.cancellable}
                  onClick={() =>
                    void guard(async () => {
                      await call('task_cancel', { taskId });
                      await taskQuery.refetch();
                    })
                  }
                >
                  {t('取消准备')}
                </Button>
                <Button onClick={close}>{t('关闭并保留任务')}</Button>
              </>
            ) : complete ? (
              <Button primary disabled={busy} onClick={() => void guard(done)}>
                {resume ? t('返回安装计划') : t('完成')}
              </Button>
            ) : (
              <>
                <Button onClick={close}>{t('稍后处理')}</Button>
                <Button primary disabled={busy} onClick={() => void install()}>
                  {t('重试下载')}
                </Button>
              </>
            )}
          </div>
        </>
      ) : (
        <>
          {environment.isPending ? (
            <p role="status">{t('正在扫描 Java 运行环境…')}</p>
          ) : (
            environment.data && (
              <>
                {environment.data.current.path && (
                  <p className="hint">
                    {t('当前配置')}：<code>{environment.data.current.path}</code>
                    <br />
                    {t(environment.data.current.message)}
                  </p>
                )}
                <fieldset className="java-options" disabled={busy} aria-label={t('Java 配置方式')}>
                  <label className={mode === 'private' ? 'selected' : ''}>
                    <input
                      type="radio"
                      name="java-mode"
                      value="private"
                      checked={mode === 'private'}
                      onChange={() => setMode('private')}
                    />
                    <span>
                      <b>{t('一键准备应用私有 Java')}</b>
                      <small>
                        Temurin JRE 21 · {size(environment.data.download.size)}
                        <br />
                        {t('自动下载、校验与配置，无需管理员权限。')}
                      </small>
                    </span>
                  </label>
                  <label className={mode === 'local' ? 'selected' : ''}>
                    <input
                      type="radio"
                      name="java-mode"
                      value="local"
                      checked={mode === 'local'}
                      onChange={() => setMode('local')}
                    />
                    <span>
                      <b>{t('使用已有 Java')}</b>
                      <small>{t('复用 JDK、JRE 或 Android Studio 自带运行时。')}</small>
                    </span>
                  </label>
                </fieldset>
                {mode === 'private' ? (
                  <>
                    <details className="java-details">
                      <summary>{t('下载与安装详情')}</summary>
                      <p>
                        {t('来源：Adoptium 官方发行（GitHub），沿用手动代理，独立于 SDK 镜像。')}
                      </p>
                      <code>{environment.data.download.url}</code>
                      <p>
                        {t('Java 安装位置')}：
                        <code>
                          {environment.data.privatePath.replace(/[\\/]bin[\\/]java\.exe$/i, '')}
                        </code>
                      </p>
                      <p>{t('运行时用于 SDK 管理，不包含项目编译所需的完整 JDK。')}</p>
                    </details>
                    <details className="java-details">
                      <summary>{t('查看 Temurin 运行时许可')}</summary>
                      <pre>{environment.data.license.text}</pre>
                    </details>
                    <label className="java-consent">
                      <input
                        type="checkbox"
                        checked={accepted}
                        onChange={(event) => setAccepted(event.target.checked)}
                      />
                      {t('我已查看并接受 Temurin 运行时许可')}
                    </label>
                  </>
                ) : (
                  <>
                    {environment.data.candidates.length > 0 && (
                      <Field label={t('发现的 Java')}>
                        <select
                          value={
                            environment.data.candidates.some((item) => item.path === path)
                              ? path
                              : ''
                          }
                          disabled={busy}
                          onChange={(event) => {
                            const candidate = environment.data!.candidates.find(
                              (item) => item.path === event.target.value,
                            );
                            changePath(event.target.value);
                            if (candidate) setChecked(candidate);
                          }}
                        >
                          <option value="">{t('选择已有运行时')}</option>
                          {environment.data.candidates.map((item) => (
                            <option key={item.path} value={item.path}>
                              {sourceName(item.source)} · {item.version || t('不可用')} ·{' '}
                              {item.path}
                            </option>
                          ))}
                        </select>
                      </Field>
                    )}
                    <Field label={t('Java 可执行文件')}>
                      <div className="input-action">
                        <input
                          value={path}
                          disabled={busy}
                          placeholder={t('java.exe 完整路径，或 Java 安装目录')}
                          onChange={(event) => changePath(event.target.value)}
                        />
                        <Button
                          disabled={busy}
                          onClick={() =>
                            void guard(async () => {
                              const picked = await call<string | null>('file_choose', {
                                kind: 'java',
                              });
                              if (picked) changePath(picked);
                            })
                          }
                        >
                          {t('选择文件')}
                        </Button>
                      </div>
                    </Field>
                    <div className="java-inline-actions">
                      <Button disabled={busy || !path.trim()} onClick={() => void validate()}>
                        {busy ? t('正在检测…') : t('检测可用性')}
                      </Button>
                      <Button
                        disabled={busy || environment.isFetching}
                        onClick={() => void environment.refetch()}
                      >
                        {t('重新扫描')}
                      </Button>
                    </div>
                    {checked && (
                      <p
                        className={
                          checked.status === 'ready' ? 'java-check-result' : 'error-banner'
                        }
                        role="status"
                      >
                        {checked.version && `Java ${checked.version} · ${checked.architecture} · `}
                        {t(checked.message)}
                      </p>
                    )}
                  </>
                )}
                {environment.data.busy && (
                  <p className="plan-warning">{t('相关任务进行中，结束后可切换 Java')}</p>
                )}
              </>
            )
          )}
          <div className="modal-actions">
            <Button onClick={close} disabled={busy}>
              {t('暂不配置')}
            </Button>
            {environment.error && (
              <Button onClick={() => void environment.refetch()}>{t('重试')}</Button>
            )}
            {environment.data && (
              <Button
                primary
                disabled={
                  busy ||
                  environment.data.busy ||
                  (mode === 'private' ? !accepted : checked?.status !== 'ready')
                }
                onClick={() => void (mode === 'private' ? install() : apply())}
              >
                {busy
                  ? t('正在处理…')
                  : mode === 'private'
                    ? t('同意并准备 Java')
                    : t('使用此 Java')}
              </Button>
            )}
          </div>
        </>
      )}
    </Modal>
  );
}
