import { createContext, useContext, useEffect, useRef, useState, type ComponentProps } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import {
  KeyRound,
  FileCheck2,
  FolderOpen,
  Copy,
  Plus,
  RefreshCw,
  Trash2,
  Wrench,
  CircleAlert,
  LoaderCircle,
  Eye,
  EyeOff,
} from 'lucide-react';
import { call, errorText, native, taskActive, size } from './api';
import { getLocale, t } from './i18n';
import { Button, Field, Modal as BaseModal, Badge } from './ui';
import type {
  ApkSignatureReport,
  Artifact,
  CertificateInfo,
  KeystoreRecord,
  Plan,
  SigningEnvironment,
  SigningRequest,
  Task,
  TaskPage,
  Workspace,
} from './bindings';
import './signing.css';
const SigningError = createContext('');
function Modal({ children, ...props }: ComponentProps<typeof BaseModal>) {
  const feedback = useContext(SigningError);
  return (
    <BaseModal {...props}>
      {children}
      {feedback && (
        <p className="error-banner" role="alert">
          {feedback}
        </p>
      )}
    </BaseModal>
  );
}

const blankRecord = (): KeystoreRecord => ({
  id: '',
  name: '',
  path: '',
  role: 'unknown',
  package: '',
  notes: '',
  sha256: '',
  format: '',
  entries: [],
  checkedAt: 0,
  status: 'needs_review',
});
const blankRequest = (): SigningRequest => ({
  mode: 'create',
  input: '',
  keystoreId: '',
  alias: 'release',
  output: '',
  name: '',
  format: 'PKCS12',
  subject: '',
  replaceSignature: false,
  overwriteSource: false,
});
const roleNames: Record<string, string> = {
  unknown: '未标记',
  upload: '上传密钥',
  app: '应用签名密钥',
  debug: '调试密钥',
};
const signingKind = (kind: string) => kind === 'keystore_create' || kind === 'apk_sign';
const signingArtifact = (a: Artifact) => a.kind === 'keystore' || a.kind === 'signed_apk';
const date = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(getLocale());
type Secrets = { storePassword: string; keyPassword: string };

export function SigningTools({
  visible,
  data,
  openSettings,
  openSdk,
  install,
}: {
  visible: boolean;
  data?: Workspace;
  openSettings: () => void;
  openSdk: () => void;
  install: (path: string) => Promise<void>;
}) {
  const client = useQueryClient();
  const [tab, setTab] = useState<'keys' | 'verify' | 'sign'>('keys');
  const [selected, select] = useState('');
  const [alias, setAlias] = useState('');
  const [search, setSearch] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const environmentPopover = useRef<HTMLDetailsElement>(null);
  const environmentCloseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const environmentTouchPointer = useRef(false);
  const [editor, setEditor] = useState<KeystoreRecord | null>(null);
  const [create, setCreate] = useState(false);
  const [authorize, setAuthorize] = useState(false);
  const [recovery, setRecovery] = useState<Artifact | null>(null);
  const [discard, setDiscard] = useState<Artifact | null>(null);
  const [forget, setForget] = useState<KeystoreRecord | null>(null);
  const [base64Record, setBase64Record] = useState<KeystoreRecord | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const planRequest = plan ? signingPlanRequest(plan) : null;
  const [submissionId, setSubmissionId] = useState('');
  const [apkPath, setApkPath] = useState('');
  const [report, setReport] = useState<ApkSignatureReport | null>(null);
  const [comparison, setComparison] = useState<CertificateInfo | null>(null);
  const [replaceSignature, setReplaceSignature] = useState(false);
  const [overwriteSource, setOverwriteSource] = useState(false);
  const records = useQuery({
    queryKey: ['signing-records'],
    queryFn: () => call<KeystoreRecord[]>('signing_records'),
    enabled: native && visible,
  });
  const environment = useQuery({
    queryKey: ['signing-environment', data?.settings.javaPath, data?.settings.sdkRoot],
    queryFn: () => call<SigningEnvironment>('signing_environment'),
    enabled: native && visible,
    refetchInterval: visible ? 10000 : false,
  });
  const artifacts = useQuery({
    queryKey: ['artifacts'],
    queryFn: () => call<Artifact[]>('artifacts_list'),
    enabled: native && visible,
    refetchInterval: visible ? 2000 : false,
  });
  const tasks = useQuery({
    queryKey: ['signing-tasks'],
    queryFn: () => call<TaskPage>('task_list', { query: { keyword: '', state: 'all', offset: 0 } }),
    enabled: native && visible,
    refetchInterval: visible ? 2000 : false,
  });
  const current = records.data?.find((record) => record.id === selected);
  const entry =
    current?.entries.find((item) => item.alias === alias) ||
    (current?.entries.length === 1 ? current.entries[0] : undefined);
  const pending = artifacts.data?.filter((a) => !a.acknowledged && signingArtifact(a)) || [];
  const savedKeyCopies =
    artifacts.data?.filter((a) => a.acknowledged && a.kind === 'keystore') || [];
  const recent = tasks.data?.tasks.filter((task) => signingKind(task.operation.kind)) || [];
  const missingJava = !!environment.data && (!environment.data.java || !environment.data.keytool);
  const missingBuildTools =
    !!environment.data && (!environment.data.apksigner || !environment.data.zipalign);
  const environmentIssue = !!environment.error || missingJava || missingBuildTools;
  const environmentStatus = t(
    environment.error
      ? '工具状态读取失败'
      : !environment.data
        ? '正在检查工具…'
        : environmentIssue
          ? '缺少签名工具'
          : '签名工具齐全',
  );
  const keepEnvironmentOpen = () => {
    if (environmentCloseTimer.current !== null) {
      clearTimeout(environmentCloseTimer.current);
      environmentCloseTimer.current = null;
    }
  };
  const closeEnvironment = () => {
    keepEnvironmentOpen();
    if (environmentPopover.current) environmentPopover.current.open = false;
  };
  const scheduleEnvironmentClose = () => {
    keepEnvironmentOpen();
    environmentCloseTimer.current = setTimeout(() => {
      environmentCloseTimer.current = null;
      if (environmentPopover.current) environmentPopover.current.open = false;
    }, 180);
  };
  useEffect(() => {
    if (!visible) {
      closeEnvironment();
      return;
    }
    const closeOutside = (event: PointerEvent) => {
      if (event.target instanceof Node && !environmentPopover.current?.contains(event.target)) {
        closeEnvironment();
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeEnvironment();
    };
    document.addEventListener('pointerdown', closeOutside);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      closeEnvironment();
      document.removeEventListener('pointerdown', closeOutside);
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [visible]);
  const refresh = async () => {
    await Promise.all([
      client.invalidateQueries({ queryKey: ['signing-records'] }),
      client.invalidateQueries({ queryKey: ['artifacts'] }),
      client.invalidateQueries({ queryKey: ['signing-tasks'] }),
      client.invalidateQueries({ queryKey: ['task-list'] }),
      client.invalidateQueries({ queryKey: ['workspace'] }),
      client.invalidateQueries({ queryKey: ['close-status'] }),
    ]);
  };
  const run = async (work: () => Promise<unknown>) => {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError('');
    setNotice('');
    try {
      await work();
    } catch (e) {
      setError(t(errorText(e)));
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };
  const releasePlan = () => {
    if (plan) void call('signing_plan_release', { planId: plan.id }).catch(() => {});
    setPlan(null);
  };
  useEffect(() => {
    setError('');
  }, [editor?.id, create, authorize, recovery?.id, discard?.id, forget?.id, base64Record?.id]);
  useEffect(() => {
    if (visible) document.body.dataset.signingDrop = 'active';
    return () => {
      delete document.body.dataset.signingDrop;
    };
  }, [visible]);
  useEffect(() => {
    if (!visible) {
      setEditor(null);
      setCreate(false);
      setAuthorize(false);
      setRecovery(null);
      setDiscard(null);
      setForget(null);
      setBase64Record(null);
      releasePlan();
    }
  }, [visible]);
  useEffect(
    () => () => {
      if (plan) void call('signing_plan_release', { planId: plan.id }).catch(() => {});
    },
    [plan?.id],
  );
  useEffect(() => {
    if (!native || !visible) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (
          event.payload.type === 'drop' &&
          event.payload.paths.length === 1 &&
          !busy &&
          !editor &&
          !create &&
          !authorize &&
          !base64Record &&
          !plan
        ) {
          const path = event.payload.paths[0];
          setApkPath(path);
          setReport(null);
          setReplaceSignature(false);
          setOverwriteSource(false);
          setTab('verify');
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [visible, busy, editor, create, authorize, base64Record, plan]);
  const chooseApk = () =>
    run(async () => {
      const path = await call<string | null>('signing_choose', { kind: 'apk' });
      if (path) {
        setApkPath(path);
        setReport(null);
        setReplaceSignature(false);
        setOverwriteSource(false);
      }
    });
  const inspectApk = () =>
    run(async () => {
      setReport(null);
      setReport(await call<ApkSignatureReport>('apk_signature_inspect', { path: apkPath }));
    });
  const setPrepared = (value: Plan) => {
    setSubmissionId(crypto.randomUUID());
    setPlan(value);
    setCreate(false);
    setAuthorize(false);
  };
  const checkSecrets = (request: SigningRequest, secrets: Secrets) =>
    run(async () => {
      setPrepared(await call<Plan>('signing_prepare', { request, secrets }));
    });
  const save = (artifact: Artifact) =>
    run(async () => {
      const path = await call<string | null>('signing_result_save', { artifactId: artifact.id });
      if (path) {
        setNotice(t('已保存：{0}', path));
        await refresh();
      }
    });
  const refreshRecord = () => {
    if (current) setEditor({ ...current });
  };
  return (
    <SigningError.Provider value={error}>
      <div className="signing-tools" hidden={!visible}>
        <div className="signing-navigation">
          <div className="signing-tabs" role="tablist" aria-label={t('签名工具')}>
            {(
              [
                ['keys', 'Keystore'],
                ['verify', 'APK 校验'],
                ['sign', 'APK 签名'],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                role="tab"
                aria-selected={tab === id}
                onClick={() => {
                  setTab(id);
                  setError('');
                }}
              >
                {t(label)}
              </button>
            ))}
          </div>
          <details
            ref={environmentPopover}
            className={`signing-environment${environmentIssue ? ' incomplete' : ''}`}
            onPointerEnter={(event) => {
              environmentTouchPointer.current = event.pointerType === 'touch';
              if (!environmentTouchPointer.current) {
                keepEnvironmentOpen();
                event.currentTarget.open = true;
              }
            }}
            onPointerLeave={(event) => {
              if (event.pointerType !== 'touch') scheduleEnvironmentClose();
            }}
            onBlurCapture={(event) => {
              if (
                !event.currentTarget.contains(event.relatedTarget as Node | null) &&
                !event.currentTarget.matches(':hover')
              ) {
                closeEnvironment();
              }
            }}
          >
            <summary
              aria-label={t('工具环境：{0}', environmentStatus)}
              onClick={(event) => {
                if (event.detail > 0 && !environmentTouchPointer.current) {
                  event.preventDefault();
                  keepEnvironmentOpen();
                  if (environmentPopover.current) environmentPopover.current.open = true;
                }
              }}
            >
              {environmentIssue ? (
                <CircleAlert size={16} aria-hidden="true" />
              ) : !environment.data ? (
                <LoaderCircle size={16} className="spin" aria-hidden="true" />
              ) : (
                <Wrench size={16} aria-hidden="true" />
              )}
            </summary>
            <div className="signing-environment-details">
              <p>
                Java {environment.data?.java ? '✓' : '—'} · keytool{' '}
                {environment.data?.keytool ? '✓' : '—'} · apksigner{' '}
                {environment.data?.apksigner ? '✓' : '—'} · zipalign{' '}
                {environment.data?.zipalign ? '✓' : '—'}
              </p>
              <div className="signing-actions">
                {(missingJava || environment.error) && (
                  <Button disabled={!native || busy} onClick={openSettings}>
                    {t('前往设置')}
                  </Button>
                )}
                {(missingBuildTools || environment.error) && (
                  <Button disabled={!native || busy} onClick={openSdk}>
                    {t('前往 SDK 管理')}
                  </Button>
                )}
              </div>
            </div>
          </details>
        </div>
        {(error || records.error || environment.error) && (
          <p className="error-banner" role="alert">
            {error || t(errorText(records.error || environment.error))}
          </p>
        )}
        {notice && (
          <p className="hint" role="status">
            {notice}
          </p>
        )}
        {tab === 'keys' && (
          <>
            <div className="signing-toolbar">
              <input
                aria-label={t('搜索密钥')}
                placeholder={t('搜索名称、路径、alias 或关联应用')}
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
              <Button disabled={!native || busy} onClick={() => setEditor(blankRecord())}>
                <Plus size={14} />
                {t('登记已有文件')}
              </Button>
              <Button primary disabled={!native || busy} onClick={() => setCreate(true)}>
                {t('新建 Keystore')}
              </Button>
            </div>
            <div className="signing-grid">
              <div className="signing-records" aria-label={t('Keystore 列表')}>
                {records.data
                  ?.filter((record) =>
                    `${record.name}\n${record.path}\n${record.package}\n${record.notes}\n${record.entries.map((e) => e.alias).join(' ')}`
                      .toLowerCase()
                      .includes(search.toLowerCase()),
                  )
                  .map((record) => (
                    <button
                      key={record.id}
                      className={selected === record.id ? 'selected' : ''}
                      onClick={() => {
                        select(record.id);
                        setAlias(record.entries.length === 1 ? record.entries[0].alias : '');
                      }}
                    >
                      <KeyRound size={16} />
                      <span>
                        <strong>{record.name}</strong>
                        <small>
                          {t(roleNames[record.role] || '未标记')} · {record.format || t('待校验')}
                        </small>
                        <small>{record.path}</small>
                        <Badge state={record.status} />
                      </span>
                    </button>
                  ))}
                {!records.data?.length && (
                  <div className="signing-empty">
                    <KeyRound size={26} />
                    <p>{t('尚未登记 Keystore')}</p>
                    <small>{t('登记只保存文件位置和公开信息；移除记录不会删除原文件。')}</small>
                  </div>
                )}
              </div>
              <div className="signing-detail">
                {current ? (
                  <>
                    <div className="signing-detail-heading">
                      <h2>{current.name}</h2>
                      <div className="signing-actions">
                        <Button disabled={busy} onClick={refreshRecord}>
                          <RefreshCw size={14} />
                          {t('检查或编辑')}
                        </Button>
                        <Button danger disabled={busy} onClick={() => setForget(current)}>
                          <Trash2 size={14} />
                          {t('移除记录')}
                        </Button>
                      </div>
                    </div>
                    <p className="signing-path">{current.path}</p>
                    <p className="hint">
                      {t('用途为用户备注，不代表商店注册状态。')}{' '}
                      {current.checkedAt
                        ? t('上次检查：{0}', date(current.checkedAt))
                        : t('尚未校验文件')}
                    </p>
                    {current.status !== 'ready' && (
                      <p className="error-banner">
                        {t('文件缺失、发生变化或尚未校验，请重新检查。以下为上次记录。')}
                      </p>
                    )}
                    {current.entries.length > 0 && (
                      <Field label="密钥 alias">
                        <select
                          value={entry?.alias || ''}
                          onChange={(event) => setAlias(event.target.value)}
                        >
                          <option value="">{t('选择密钥条目')}</option>
                          {current.entries.map((item) => (
                            <option key={item.alias} value={item.alias}>
                              {item.alias} ·{' '}
                              {item.privateKey ? 'PrivateKeyEntry' : 'TrustedCertificateEntry'}
                            </option>
                          ))}
                        </select>
                      </Field>
                    )}
                    {entry && (
                      <>
                        <CertificateDetails
                          certificate={entry.certificate}
                          onCopy={(text) =>
                            void run(async () => {
                              await call('text_copy', { text });
                              setNotice(t('已复制'));
                            })
                          }
                        />
                        <div className="signing-actions">
                          <Button
                            disabled={busy || current.status !== 'ready'}
                            onClick={() =>
                              void run(async () => {
                                const path = await call<string | null>(
                                  'signing_certificate_export',
                                  {
                                    recordId: current.id,
                                    alias: entry.alias,
                                    expectedSha256: entry.certificate.sha256,
                                    format: 'PEM',
                                  },
                                );
                                if (path) setNotice(t('已保存：{0}', path));
                              })
                            }
                          >
                            {t('导出 PEM')}
                          </Button>
                          <Button
                            disabled={busy || current.status !== 'ready'}
                            onClick={() =>
                              void run(async () => {
                                const path = await call<string | null>(
                                  'signing_certificate_export',
                                  {
                                    recordId: current.id,
                                    alias: entry.alias,
                                    expectedSha256: entry.certificate.sha256,
                                    format: 'DER',
                                  },
                                );
                                if (path) setNotice(t('已保存：{0}', path));
                              })
                            }
                          >
                            {t('导出 DER')}
                          </Button>
                          <Button
                            primary
                            disabled={!entry.privateKey || current.status !== 'ready' || busy}
                            onClick={() => {
                              setAlias(entry.alias);
                              setTab('sign');
                            }}
                          >
                            {t('使用此密钥签名')}
                          </Button>
                        </div>
                      </>
                    )}
                    <div className="signing-actions">
                      <Button
                        disabled={busy || current.status !== 'ready' || !environment.data?.java}
                        onClick={() => {
                          setNotice('');
                          setBase64Record(current);
                        }}
                      >
                        {t('生成 Base64')}
                      </Button>
                      <Button
                        disabled={busy || current.status === 'missing_dependency'}
                        onClick={() =>
                          void run(() => call('signing_open_file', { path: current.path }))
                        }
                      >
                        <FolderOpen size={14} />
                        {t('打开所在目录')}
                      </Button>
                    </div>
                  </>
                ) : (
                  <div className="signing-empty">
                    <p>{t('选择一个 Keystore 查看 alias 与证书')}</p>
                  </div>
                )}
              </div>
            </div>
          </>
        )}
        {tab !== 'keys' && (
          <div className="signing-workflow">
            <div className="signing-file-row">
              <Field label="APK 文件">
                <input
                  value={apkPath}
                  placeholder={t('选择或拖入单个完整 APK')}
                  onChange={(event) => {
                    setApkPath(event.target.value);
                    setReport(null);
                    setReplaceSignature(false);
                    setOverwriteSource(false);
                  }}
                />
              </Field>
              <Button disabled={!native || busy} onClick={() => void chooseApk()}>
                {t('选择 APK')}
              </Button>
              <Button
                primary
                disabled={!native || busy || !apkPath}
                loading={busy}
                onClick={() => void inspectApk()}
              >
                <FileCheck2 size={14} />
                {t('检查签名')}
              </Button>
            </div>
            {report && tab === 'sign' && (
              <div className="signing-apk-summary">
                <p>
                  <strong>
                    {report.package || t('包信息未识别')} {report.version}
                  </strong>
                  <span>
                    {t(report.verified ? '已签名' : report.unsigned ? '未签名' : '签名验证未通过')}
                  </span>
                </p>
                <button
                  type="button"
                  className="text-button"
                  onClick={() => {
                    setTab('verify');
                    setError('');
                  }}
                >
                  {t('查看签名详情')}
                </button>
              </div>
            )}
            {report && tab === 'verify' && (
              <div className="signing-report">
                <h2>
                  {report.package || t('包信息未识别')} {report.version}
                </h2>
                <Badge state={report.verified ? 'succeeded' : 'needs_review'} />
                <p>
                  {report.verified
                    ? t('签名完整性验证通过')
                    : t(report.unsigned ? '未签名' : '签名验证未通过')}
                </p>
                <p className="hint">
                  {t('有效签名不代表发布者可信、应用安全或一定可以覆盖升级。')}
                </p>
                <p>
                  {t('签名方案')}：{report.schemes.join(', ') || '—'} ·{' '}
                  {report.lineage ? t('包含轮换谱系') : t('未检测到轮换谱系')}
                </p>
                <code className="signing-fingerprint">{report.sha256}</code>
                {report.signers.map((cert) => (
                  <CertificateDetails
                    key={cert.sha256}
                    certificate={cert}
                    onCopy={(text) => void run(() => call('text_copy', { text }))}
                  />
                ))}
                {[...report.errors, ...report.warnings].length > 0 && (
                  <details>
                    <summary>{t('查看验证诊断')}</summary>
                    <pre>{[...report.errors, ...report.warnings].join('\n')}</pre>
                  </details>
                )}
              </div>
            )}
            <div className="signing-form-grid">
              <Field label="Keystore">
                <select
                  value={selected}
                  onChange={(event) => {
                    select(event.target.value);
                    setAlias('');
                  }}
                >
                  <option value="">{t('选择已登记文件')}</option>
                  {records.data?.map((record) => (
                    <option key={record.id} value={record.id}>
                      {record.name}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="密钥 alias">
                <select
                  value={entry?.alias || ''}
                  onChange={(event) => setAlias(event.target.value)}
                >
                  <option value="">{t('选择密钥条目')}</option>
                  {current?.entries.map((item) => (
                    <option key={item.alias} value={item.alias}>
                      {item.alias}
                      {item.privateKey ? '' : ` (${t('仅证书')})`}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
            {tab === 'verify' && (
              <>
                <div className="signing-actions">
                  <Button
                    disabled={!native || busy}
                    onClick={() =>
                      void run(async () => {
                        const path = await call<string | null>('signing_choose', {
                          kind: 'certificate',
                        });
                        if (path)
                          setComparison(
                            await call<CertificateInfo>('signing_certificate_inspect', { path }),
                          );
                      })
                    }
                  >
                    {t('选择独立公开证书')}
                  </Button>
                  {comparison && (
                    <Button onClick={() => setComparison(null)}>
                      {t('改用所选 Keystore 证书')}
                    </Button>
                  )}
                </div>
                {report && (comparison || entry) && (
                  <div className="signing-comparison" role="status">
                    {t('证书对比')}：
                    {!report.verified ||
                    report.lineage ||
                    report.signers.length !== 1 ||
                    (!comparison && current?.status !== 'ready')
                      ? t('无法确定；请检查签名状态、签名者与轮换信息')
                      : report.signers[0].sha256 === (comparison || entry?.certificate)?.sha256
                        ? t('当前证书一致')
                        : t('当前证书不同')}
                    <p className="hint">
                      {t('Play 分发 APK 可能使用应用签名证书，本机文件可能是上传密钥。')}
                    </p>
                  </div>
                )}
              </>
            )}
            {tab === 'sign' && (
              <>
                <div className="signing-output-options">
                  <label className="check-row">
                    <input
                      type="checkbox"
                      checked={replaceSignature}
                      onChange={(event) => setReplaceSignature(event.target.checked)}
                    />
                    {t('覆盖原签名')}
                  </label>
                  <label className="check-row">
                    <input
                      type="checkbox"
                      checked={overwriteSource}
                      onChange={(event) => setOverwriteSource(event.target.checked)}
                    />
                    {t('覆盖源文件')}
                  </label>
                </div>
                <p className="hint">
                  {t(
                    overwriteSource
                      ? '签名验证通过后替换源 APK，无需选择输出路径。'
                      : '选择输出文件，已有文件将被替换。',
                  )}
                </p>
                <Button
                  primary
                  disabled={!native || busy || !apkPath || !current || !entry?.privateKey}
                  onClick={() => {
                    setError('');
                    setAuthorize(true);
                  }}
                >
                  {t(overwriteSource ? '核对签名计划' : '选择输出并核对签名计划')}
                </Button>
              </>
            )}
          </div>
        )}
        {pending.length > 0 && (
          <section className="signing-results">
            <h2>{t('待保存签名结果')}</h2>
            <p className="hint">
              {t('新密钥受保护；请保存或逐项明确放弃，不会被普通缓存清理删除。')}
            </p>
            {pending.map((artifact) => (
              <div className="signing-result" key={artifact.id}>
                <div>
                  <strong>{t(artifact.title)}</strong>
                  <small>
                    {size(artifact.bytes)} · {artifact.sha256}
                  </small>
                </div>
                <div className="signing-actions">
                  {artifact.kind === 'keystore' && (
                    <Button disabled={busy} onClick={() => setRecovery(artifact)}>
                      {t('核验恢复密钥')}
                    </Button>
                  )}
                  <Button primary disabled={busy} onClick={() => void save(artifact)}>
                    {t('另存结果')}
                  </Button>
                  <Button danger disabled={busy} onClick={() => setDiscard(artifact)}>
                    {t('放弃此结果')}
                  </Button>
                </div>
              </div>
            ))}
          </section>
        )}
        {savedKeyCopies.length > 0 && (
          <section className="signing-results">
            <h2>{t('已另存密钥的加密缓存')}</h2>
            <p className="hint">
              {t('这些新密钥已另存成功，应用仍保留加密副本。可单独清理副本，已登记文件保持不变。')}
            </p>
            {savedKeyCopies.map((a) => (
              <div className="signing-result" key={a.id}>
                <div>
                  <strong>{t(a.title)}</strong>
                  <small>{a.sha256}</small>
                </div>
                <Button danger disabled={busy} onClick={() => setDiscard(a)}>
                  {t('清理此加密副本')}
                </Button>
              </div>
            ))}
          </section>
        )}
        {recent.length > 0 && (
          <section className="signing-results">
            <h2>{t('最近签名任务')}</h2>
            {recent.map((task) => (
              <div className="signing-result" key={task.id}>
                <div>
                  <strong>
                    {t(task.title)} <Badge state={task.state} />
                  </strong>
                  <p>{t(task.message)}</p>
                  {task.operation.args.savedOutput && (
                    <small>{task.operation.args.savedOutput}</small>
                  )}
                </div>
                <div className="signing-actions">
                  {taskActive(task.state) && (
                    <Button
                      disabled={busy || task.cancelRequested}
                      onClick={() => void run(() => call('task_cancel', { taskId: task.id }))}
                    >
                      {t('取消')}
                    </Button>
                  )}
                  {task.operation.args.savedOutput && (
                    <>
                      <Button
                        disabled={busy}
                        onClick={() =>
                          void run(() =>
                            call('signing_open_file', { path: task.operation.args.savedOutput }),
                          )
                        }
                      >
                        {t('打开所在目录')}
                      </Button>
                      {task.operation.kind === 'apk_sign' && (
                        <Button
                          primary
                          disabled={busy}
                          onClick={() => void run(() => install(task.operation.args.savedOutput!))}
                        >
                          {t('安装到设备')}
                        </Button>
                      )}
                    </>
                  )}
                </div>
              </div>
            ))}
          </section>
        )}
        {editor && (
          <RecordEditor
            record={editor}
            busy={busy}
            close={() => setEditor(null)}
            save={(record, secrets) =>
              void run(async () => {
                const saved = await call<KeystoreRecord>('keystore_register', { record, secrets });
                select(saved.id);
                setAlias(saved.entries.length === 1 ? saved.entries[0].alias : '');
                setEditor(null);
                await refresh();
              })
            }
          />
        )}
        {create && (
          <CreateKey
            busy={busy}
            close={() => setCreate(false)}
            submit={(request, secrets) => void checkSecrets(request, secrets)}
          />
        )}
        {base64Record && visible && (
          <KeystoreBase64Dialog
            record={base64Record}
            busy={busy}
            close={() => setBase64Record(null)}
            copied={() => {
              setBase64Record(null);
              setNotice(t('已复制'));
            }}
            run={run}
          />
        )}
        {authorize && (
          <PasswordDialog
            title="提供本次签名密码"
            busy={busy}
            close={() => setAuthorize(false)}
            submit={(secrets) =>
              void run(async () => {
                const output = overwriteSource
                  ? apkPath
                  : await call<string | null>('signing_choose', {
                      kind: 'output-apk',
                      input: apkPath,
                    });
                if (!output) return;
                const request = {
                  ...blankRequest(),
                  mode: 'sign',
                  input: apkPath,
                  keystoreId: selected,
                  alias: entry?.alias || '',
                  output,
                  replaceSignature,
                  overwriteSource,
                };
                setPrepared(await call<Plan>('signing_prepare', { request, secrets }));
              })
            }
          />
        )}
        {recovery && (
          <PasswordDialog
            title="核验恢复密钥"
            busy={busy}
            close={() => setRecovery(null)}
            submit={(secrets) =>
              void run(async () => {
                await call('signing_result_check', { artifactId: recovery.id, secrets });
                setRecovery(null);
                setNotice(t('密钥已核验，请另存结果'));
                await refresh();
              })
            }
          />
        )}
        {plan && (
          <Modal title="核对签名计划" onClose={releasePlan} dismissible={!busy} wide trapFocus>
            <SigningPlanReview plan={plan} request={planRequest} />
            <div className="modal-actions">
              <Button disabled={busy} onClick={releasePlan}>
                {t('返回修改')}
              </Button>
              <Button
                primary
                loading={busy}
                disabled={!planRequest}
                onClick={() =>
                  void run(async () => {
                    await call<Task>('signing_submit', {
                      planId: plan.id,
                      planHash: plan.hash,
                      confirmationToken: plan.confirmationToken,
                      requestId: submissionId,
                    });
                    setPlan(null);
                    setNotice(t('任务已提交，可在本页或任务中心查看'));
                    await refresh();
                  })
                }
              >
                {t(
                  plan.operation.kind === 'keystore_create'
                    ? '创建 Keystore'
                    : plan.operation.args.overwriteSource === 'true' ||
                        plan.operation.args.overwriteOutput === 'true'
                      ? '签名并覆盖'
                      : '确认签名',
                )}
              </Button>
            </div>
          </Modal>
        )}
        {discard && (
          <Modal
            title={
              discard.acknowledged
                ? '清理已另存密钥的缓存'
                : discard.kind === 'keystore'
                  ? '放弃未保存密钥'
                  : '放弃签名结果'
            }
            onClose={() => setDiscard(null)}
            dismissible={!busy}
            trapFocus
          >
            <p>
              {t(
                discard.acknowledged
                  ? '仅删除应用内的加密副本，已另存的 Keystore 和登记信息保持不变。'
                  : discard.kind === 'keystore'
                    ? '此密钥尚未另存，放弃后无法恢复，也无法用它签署后续版本。请确认已有其他备份。'
                    : '将删除这份待保存结果，输入原件和已另存文件保留。',
              )}
            </p>
            <code className="signing-fingerprint">{discard.sha256}</code>
            <div className="modal-actions">
              <Button onClick={() => setDiscard(null)} disabled={busy}>
                {t('保留')}
              </Button>
              <Button
                danger
                loading={busy}
                onClick={() =>
                  void run(async () => {
                    await call('signing_result_discard', {
                      artifactId: discard.id,
                      expectedSha256: discard.sha256,
                    });
                    setDiscard(null);
                    await refresh();
                  })
                }
              >
                {t(discard.acknowledged ? '清理此加密副本' : '明确放弃此结果')}
              </Button>
            </div>
          </Modal>
        )}
        {forget && (
          <Modal
            title="移除 Keystore 登记"
            onClose={() => setForget(null)}
            dismissible={!busy}
            trapFocus
          >
            <p>{t('只移除登记信息，原 Keystore 文件保持不变。')}</p>
            <p>{forget.path}</p>
            <div className="modal-actions">
              <Button onClick={() => setForget(null)} disabled={busy}>
                {t('返回')}
              </Button>
              <Button
                danger
                loading={busy}
                onClick={() =>
                  void run(async () => {
                    await call('keystore_forget', { recordId: forget.id });
                    setForget(null);
                    select('');
                    await refresh();
                  })
                }
              >
                {t('移除记录')}
              </Button>
            </div>
          </Modal>
        )}
      </div>
    </SigningError.Provider>
  );
}

function CertificateDetails({
  certificate: cert,
  onCopy,
}: {
  certificate: CertificateInfo;
  onCopy: (value: string) => void;
}) {
  const valid = cert.notBefore <= Date.now() / 1000 && cert.notAfter > Date.now() / 1000;
  return (
    <dl className="signing-certificate">
      <dt>{t('证书主体')}</dt>
      <dd>{cert.subject}</dd>
      <dt>{t('颁发者')}</dt>
      <dd>{cert.issuer}</dd>
      <dt>{t('有效期')}</dt>
      <dd>
        {date(cert.notBefore)} → {date(cert.notAfter)} ·{' '}
        {t(valid ? '当前有效' : '已过期或尚未生效')}
        {valid && cert.notAfter - Date.now() / 1000 < 365 * 86400 && (
          <p className="hint">{t('证书将在一年内到期，请检查后续更新计划。')}</p>
        )}
      </dd>
      <dt>{t('密钥算法')}</dt>
      <dd>
        {cert.algorithm} {cert.bits}
      </dd>
      {(['sha1', 'sha256'] as const).map((kind) => (
        <div className="signing-cert-row" key={kind}>
          <dt>{kind === 'sha1' ? 'SHA-1' : 'SHA-256'}</dt>
          <dd>
            <code>
              {cert[kind]
                .match(/.{1,2}/g)
                ?.join(':')
                .toUpperCase()}
            </code>
            <button
              className="icon-button"
              title={t('复制指纹')}
              aria-label={t('复制指纹')}
              onClick={() =>
                onCopy(
                  cert[kind]
                    .match(/.{1,2}/g)
                    ?.join(':')
                    .toUpperCase() || '',
                )
              }
            >
              <Copy size={14} />
            </button>
            <button className="text-button" onClick={() => onCopy(cert[kind].toUpperCase())}>
              {t('复制无分隔值')}
            </button>
          </dd>
        </div>
      ))}
    </dl>
  );
}
function readableSigningPath(value: string) {
  if (value.startsWith('\\\\?\\UNC\\')) return '\\\\' + value.slice(8);
  return value.startsWith('\\\\?\\') ? value.slice(4) : value;
}
function signingPlanRequest(plan: Plan): SigningRequest | null {
  try {
    const request = JSON.parse(plan.operation.args.request || '');
    return request &&
      typeof request.input === 'string' &&
      typeof request.output === 'string' &&
      typeof request.alias === 'string'
      ? request
      : null;
  } catch {
    return null;
  }
}
function SigningPlanReview({ plan, request }: { plan: Plan; request: SigningRequest | null }) {
  if (!request) return <p className="error-banner">{t('无法读取计划信息，请返回重新核对。')}</p>;
  const args = plan.operation.args;
  const creating = plan.operation.kind === 'keystore_create';
  const fileName = (path: string) => readableSigningPath(path).split(/[\\/]/).at(-1) || path;
  const overwriteSource = !!request.overwriteSource;
  const overwriteOutput = args.overwriteOutput === 'true';
  const overwrite = overwriteSource || overwriteOutput;
  const title = creating ? request.name : args.package || fileName(request.input);
  const PathValue = ({ path }: { path: string }) => (
    <>
      <strong>{fileName(path)}</strong>
      <span className="signing-plan-path">{readableSigningPath(path)}</span>
    </>
  );
  return (
    <div className="signing-plan-review">
      <div className="signing-plan-heading">
        <span className="signing-plan-icon" aria-hidden="true">
          {creating ? <KeyRound size={20} /> : <FileCheck2 size={20} />}
        </span>
        <div>
          <strong>{title}</strong>
          <p>
            {creating
              ? `${request.format} · RSA 3072`
              : [args.version, Number(args.inputBytes) ? size(Number(args.inputBytes)) : '']
                  .filter(Boolean)
                  .join(' · ')}
          </p>
        </div>
        <span className="signing-plan-mode">
          {t(
            creating
              ? '新建密钥'
              : overwriteSource
                ? '覆盖源文件'
                : overwriteOutput
                  ? '覆盖输出文件'
                  : '另存新文件',
          )}
        </span>
      </div>
      <dl className="signing-plan-fields">
        {!creating && !overwriteSource && (
          <div>
            <dt>{t('输入 APK')}</dt>
            <dd>
              <PathValue path={request.input} />
            </dd>
          </div>
        )}
        <div className={`signing-plan-output${overwrite ? ' overwrite' : ''}`}>
          <dt>{t(overwrite ? '覆盖目标' : '输出文件')}</dt>
          <dd>
            <PathValue path={request.output} />
          </dd>
        </div>
        <div>
          <dt>{t(creating ? '密钥 alias' : '签名密钥')}</dt>
          <dd>
            <strong>
              {creating
                ? request.alias
                : `${args.keystoreName || fileName(args.keystorePath || '')} / ${request.alias}`}
            </strong>
            {!creating && args.keystorePath && (
              <span className="signing-plan-path">{readableSigningPath(args.keystorePath)}</span>
            )}
          </dd>
        </div>
        {creating && (
          <div>
            <dt>{t('证书主体')}</dt>
            <dd>{request.subject}</dd>
          </div>
        )}
      </dl>
      {(overwrite || request.replaceSignature || creating) && (
        <p className="signing-plan-notice">
          {t(
            creating
              ? '生成后请备份密钥文件与密码。'
              : overwriteSource
                ? request.replaceSignature
                  ? '验证通过后覆盖源 APK；证书不同可能无法覆盖原安装。'
                  : '验证通过后替换源 APK；源文件已变化时停止覆盖。'
                : overwriteOutput
                  ? '验证通过后替换所选输出文件。'
                  : '将替换新文件中的原签名；证书不同可能无法覆盖安装。',
          )}
        </p>
      )}
      <p className="hint signing-plan-expiry">{t('密码仅本次使用 · 授权有效期 5 分钟')}</p>
    </div>
  );
}
function PasswordField({
  label,
  value,
  change,
}: {
  label: string;
  value: string;
  change: (value: string) => void;
}) {
  const [show, setShow] = useState(false);
  return (
    <Field label={label}>
      <div className="signing-password-input">
        <input
          type={show ? 'text' : 'password'}
          autoComplete="off"
          value={value}
          onChange={(event) => change(event.target.value)}
        />
        <button
          type="button"
          className="icon-button signing-password-toggle"
          aria-label={t(show ? '隐藏{0}' : '显示{0}', t(label))}
          aria-pressed={show}
          title={t(show ? '隐藏密码' : '显示密码')}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => setShow((current) => !current)}
        >
          {show ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
        </button>
      </div>
    </Field>
  );
}
function PasswordFields({
  value,
  change,
  creating = false,
  format = 'JKS',
}: {
  value: Secrets;
  change: (value: Secrets) => void;
  creating?: boolean;
  format?: string;
}) {
  return (
    <>
      <PasswordField
        label="容器密码"
        value={value.storePassword}
        change={(storePassword) => change({ ...value, storePassword })}
      />
      {format !== 'PKCS12' && (
        <PasswordField
          label="私钥密码（留空使用容器密码）"
          value={value.keyPassword}
          change={(keyPassword) => change({ ...value, keyPassword })}
        />
      )}
      <p className="hint">
        {t(
          creating
            ? '新建密码至少 12 个字符；请自行妥善保存。'
            : '密码仅用于本次操作，关闭面板即清空。',
        )}
      </p>
    </>
  );
}
function KeystoreBase64Dialog({
  record,
  busy,
  close,
  copied,
  run,
}: {
  record: KeystoreRecord;
  busy: boolean;
  close: () => void;
  copied: () => void;
  run: (work: () => Promise<unknown>) => Promise<void>;
}) {
  const [password, setPassword] = useState('');
  return (
    <Modal title="生成 Keystore Base64" onClose={close} dismissible={!busy} trapFocus>
      <p className="signing-path">
        {record.name} · {record.path}
      </p>
      <p className="hint">
        {t('验证容器密码后，直接将整个 Keystore 文件的单行 Base64 复制到剪贴板。')}
      </p>
      <PasswordField label="容器密码" value={password} change={setPassword} />
      <p className="hint">{t('每次生成均需验证容器密码；密码仅用于本次操作。')}</p>
      <p className="signing-warning">
        {t('Base64 不是加密，请像原 Keystore 文件一样保管；复制内容会保留在系统剪贴板。')}
      </p>
      <div className="modal-actions">
        <Button disabled={busy} onClick={close}>
          {t('返回')}
        </Button>
        <Button
          primary
          loading={busy}
          disabled={!password}
          onClick={() =>
            void run(async () => {
              try {
                await call('keystore_copy_base64', {
                  recordId: record.id,
                  secrets: { storePassword: password },
                });
                copied();
              } finally {
                setPassword('');
              }
            })
          }
        >
          <Copy size={14} />
          {t('验证密码并复制')}
        </Button>
      </div>
    </Modal>
  );
}
function PasswordDialog({
  title,
  busy,
  close,
  submit,
}: {
  title: string;
  busy: boolean;
  close: () => void;
  submit: (value: Secrets) => void;
}) {
  const [secrets, setSecrets] = useState<Secrets>({ storePassword: '', keyPassword: '' });
  return (
    <Modal title={title} onClose={close} dismissible={!busy} trapFocus>
      <PasswordFields value={secrets} change={setSecrets} />
      <div className="modal-actions">
        <Button disabled={busy} onClick={close}>
          {t('返回')}
        </Button>
        <Button
          primary
          loading={busy}
          disabled={!secrets.storePassword}
          onClick={() => submit(secrets)}
        >
          {t('检查并继续')}
        </Button>
      </div>
    </Modal>
  );
}
function RecordEditor({
  record,
  busy,
  close,
  save,
}: {
  record: KeystoreRecord;
  busy: boolean;
  close: () => void;
  save: (record: KeystoreRecord, secrets: Secrets | null) => void;
}) {
  const [value, setValue] = useState({ ...record });
  const [secrets, setSecrets] = useState<Secrets>({ storePassword: '', keyPassword: '' });
  const [picking, setPicking] = useState(false);
  const [pickerError, setPickerError] = useState('');
  return (
    <Modal
      title={record.id ? '检查或编辑 Keystore' : '登记已有 Keystore'}
      onClose={close}
      dismissible={!busy && !picking}
      wide
      trapFocus
    >
      <div className="signing-form-grid">
        <Field label="名称">
          <input
            value={value.name}
            maxLength={200}
            onChange={(e) => setValue({ ...value, name: e.target.value })}
          />
        </Field>
        <Field label="用途">
          <select value={value.role} onChange={(e) => setValue({ ...value, role: e.target.value })}>
            {Object.entries(roleNames).map(([id, label]) => (
              <option key={id} value={id}>
                {t(label)}
              </option>
            ))}
          </select>
        </Field>
      </div>
      <Field label="Keystore 文件">
        <div className="signing-actions">
          <input
            value={value.path}
            onChange={(e) => setValue({ ...value, path: e.target.value })}
          />
          <Button
            disabled={busy || picking}
            onClick={() => {
              setPicking(true);
              setPickerError('');
              void call<string | null>('signing_choose', { kind: 'keystore' })
                .then((path) => {
                  if (path)
                    setValue((current) => ({
                      ...current,
                      path,
                      name: current.name || path.split(/[\\/]/).at(-1) || '',
                    }));
                })
                .catch((e) => setPickerError(t(errorText(e))))
                .finally(() => setPicking(false));
            }}
          >
            {t('选择文件')}
          </Button>
        </div>
      </Field>
      <Field label="关联包名（可选）">
        <input
          value={value.package}
          maxLength={200}
          onChange={(e) => setValue({ ...value, package: e.target.value })}
        />
      </Field>
      <Field label="备注">
        <textarea
          value={value.notes}
          maxLength={2000}
          onChange={(e) => setValue({ ...value, notes: e.target.value })}
        />
      </Field>
      <PasswordFields value={secrets} change={setSecrets} format="PKCS12" />
      {pickerError && (
        <p className="error-banner" role="alert">
          {pickerError}
        </p>
      )}
      <p className="hint">{t('不输入密码时仅登记位置，文件尚未校验。')}</p>
      <div className="modal-actions">
        <Button onClick={close} disabled={busy || picking}>
          {t('返回')}
        </Button>
        <Button
          primary
          loading={busy}
          disabled={picking || !value.path || !value.name}
          onClick={() => save(value, secrets.storePassword ? secrets : null)}
        >
          {t('保存登记')}
        </Button>
      </div>
    </Modal>
  );
}
function CreateKey({
  busy,
  close,
  submit,
}: {
  busy: boolean;
  close: () => void;
  submit: (request: SigningRequest, secrets: Secrets) => void;
}) {
  const [request, setRequest] = useState(blankRequest);
  const [secrets, setSecrets] = useState<Secrets>({ storePassword: '', keyPassword: '' });
  const [confirmation, setConfirmation] = useState('');
  const [keyConfirmation, setKeyConfirmation] = useState('');
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState('');
  const valid =
    secrets.storePassword.length >= 12 &&
    secrets.storePassword === confirmation &&
    (!secrets.keyPassword || secrets.keyPassword === keyConfirmation);
  return (
    <Modal title="新建 Keystore" onClose={close} dismissible={!busy && !picking} wide trapFocus>
      <div className="signing-form-grid">
        <Field label="名称">
          <input
            value={request.name}
            maxLength={200}
            onChange={(e) => setRequest({ ...request, name: e.target.value })}
          />
        </Field>
        <Field label="格式">
          <select
            value={request.format}
            onChange={(e) => {
              setRequest({ ...request, format: e.target.value, output: '' });
              setSecrets({ ...secrets, keyPassword: '' });
            }}
          >
            <option>PKCS12</option>
            <option>JKS</option>
          </select>
        </Field>
        <Field label="密钥 alias">
          <input
            value={request.alias}
            maxLength={200}
            onChange={(e) => setRequest({ ...request, alias: e.target.value })}
          />
        </Field>
        <Field label="证书主体（DN）">
          <input
            placeholder="CN=Example, O=Example, C=CN"
            value={request.subject}
            maxLength={1000}
            onChange={(e) => setRequest({ ...request, subject: e.target.value })}
          />
        </Field>
      </div>
      <Field label="新的输出文件">
        <div className="signing-actions">
          <input
            value={request.output}
            onChange={(e) => setRequest({ ...request, output: e.target.value })}
          />
          <Button
            disabled={busy || picking}
            onClick={() => {
              setPicking(true);
              setError('');
              void call<string | null>('signing_choose', {
                kind: request.format === 'JKS' ? 'output-jks' : 'output-p12',
              })
                .then((output) => {
                  if (output) setRequest((value) => ({ ...value, output }));
                })
                .catch((e) => setError(t(errorText(e))))
                .finally(() => setPicking(false));
            }}
          >
            {t('选择新文件')}
          </Button>
        </div>
      </Field>
      <p className="hint">
        RSA 3072 · SHA256withRSA · 10950 {t('天')} · {t('已有文件不会被覆盖')}
      </p>
      <PasswordFields value={secrets} change={setSecrets} creating format={request.format} />
      {request.format === 'PKCS12' && (
        <p className="hint">
          {t('当前 Java 的 PKCS12 新建密码需使用 ASCII 字符；需要中文密码时请选择 JKS')}
        </p>
      )}
      <PasswordField label="确认容器密码" value={confirmation} change={setConfirmation} />
      {request.format === 'JKS' && secrets.keyPassword && (
        <PasswordField label="确认私钥密码" value={keyConfirmation} change={setKeyConfirmation} />
      )}
      {error && (
        <p className="error-banner" role="alert">
          {error}
        </p>
      )}
      <div className="modal-actions">
        <Button disabled={busy || picking} onClick={close}>
          {t('返回')}
        </Button>
        <Button
          primary
          loading={busy}
          disabled={
            picking ||
            !valid ||
            !request.name ||
            !request.alias ||
            !request.subject ||
            !request.output
          }
          onClick={() => submit(request, secrets)}
        >
          {t('核对生成计划')}
        </Button>
      </div>
    </Modal>
  );
}
