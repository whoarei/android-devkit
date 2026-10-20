import { t, useLocale, getLocale } from './i18n';
import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ChevronDown, ChevronRight, HardDrive, Plus, Search, X } from 'lucide-react';
import { call, errorText, native, size } from './api';
import { Button, Field, Modal } from './ui';
import {
  hasSdkUpdate as updated,
  imageFamily,
  imageAbi,
  imageNodes,
  packageNode,
  type SdkTreeNode,
} from './sdk';
import type { SdkPackage, SdkSite, SdkSiteStatus, Settings, Workspace } from './bindings';
const isPlatform = (p: SdkPackage) => /^(platforms|sources|system-images);/.test(p.id);
const apiKey = (p: SdkPackage) => {
  const key =
    p.id
      .split(';')
      .find((part) => part.startsWith('android-'))
      ?.slice(8) ||
    p.api ||
    '其他';
  return key.toLowerCase().includes('canary') ? 'canary' : key;
};
const toolNames: Record<string, string> = {
  'build-tools': 'Android SDK Build-Tools',
  'platform-tools': 'Android SDK Platform-Tools',
  'cmdline-tools': 'Android SDK Command-line Tools',
  emulator: 'Android Emulator',
  ndk: 'NDK（并行版本）',
  cmake: 'CMake',
  extras: '驱动与附加组件',
  'add-ons': '厂商扩展',
  tools: '旧版 SDK Tools',
};
export function SdkManager({
  data,
  prepare,
  busy,
  refresh,
  refreshing,
  refreshCatalog,
  warnings,
}: {
  data?: Workspace;
  busy: boolean;
  refresh: () => Promise<unknown>;
  refreshing: boolean;
  refreshCatalog: (beforeRefresh?: () => Promise<void>) => Promise<string[]>;
  warnings: string[];
  prepare: (kind: string, args?: Record<string, string>) => Promise<void>;
}) {
  const locale = useLocale();
  const [tab, setTab] = useState<'platforms' | 'tools' | 'sites'>('platforms');
  const [search, setSearch] = useState('');
  const [status, setStatus] = useState('all');
  const [hideObsolete, setHideObsolete] = useState(true);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [selected, setSelected] = useState<Record<string, string>>({});
  const [editing, setEditing] = useState<{
    index: number;
    site: SdkSite;
  } | null>(null);
  const saving = refreshing;
  const [error, setError] = useState('');
  useEffect(() => {
    if (refreshing) setError('');
  }, [refreshing]);
  useEffect(() => {
    if (!data) return;
    setSelected((values) => {
      const entries = Object.entries(values).filter(([id, action]) => {
        const p = data.packages.find((p) => p.id === id);
        return (
          p &&
          (action === 'install' ? !p.installed || updated(p) : action === 'remove' && p.installed)
        );
      });
      return entries.length === Object.keys(values).length ? values : Object.fromEntries(entries);
    });
  }, [data?.packages]);
  const sites = useQuery({
    queryKey: ['sdk-sites'],
    queryFn: () => call<SdkSiteStatus[]>('sdk_sites_status'),
    enabled: native && tab === 'sites',
  });
  const groups = useMemo(() => {
    const result = new Map<string, SdkPackage[]>();
    for (const p of data?.packages ?? []) {
      if (isPlatform(p) !== (tab === 'platforms') || (hideObsolete && p.obsolete && !p.installed))
        continue;
      if (
        (status === 'installed' && !p.installed) ||
        (status === 'missing' && p.installed) ||
        (status === 'updates' && !updated(p)) ||
        (status === 'selected' && !selected[p.id])
      )
        continue;
      const key = tab === 'platforms' ? apiKey(p) : p.id.split(';')[0];
      const image = p.id.startsWith('system-images;')
        ? t('{0} 系统镜像 {1}', imageFamily(p).title, imageAbi(p))
        : '';
      const searchable =
        `${p.id} ${p.name} ${p.api} ${p.tag} ${p.abi} ${t(toolNames[key] || '')} ${image}`.toLowerCase();
      if (
        !search
          .trim()
          .toLowerCase()
          .split(/\s+/)
          .every((word) => searchable.includes(word))
      )
        continue;
      if (!result.has(key)) result.set(key, []);
      result.get(key)!.push(p);
    }
    return [...result.entries()]
      .sort(([a], [b]) =>
        tab === 'platforms' ? b.localeCompare(a, undefined, { numeric: true }) : a.localeCompare(b),
      )
      .map(([key, packages]) => ({
        kind: 'group' as const,
        key: `${tab}:${key}`,
        packages: packages.sort((a, b) => {
          const order = (p: SdkPackage) =>
            p.id.startsWith('platforms;') ? 0 : p.id.startsWith('sources;') ? 1 : 2;
          return order(a) - order(b) || b.id.localeCompare(a.id, undefined, { numeric: true });
        }),
        title:
          tab === 'platforms'
            ? key === 'canary'
              ? 'Android CANARY Preview'
              : /^\d/.test(key)
                ? `Android · API ${key}`
                : `Android · ${t(key)}`
            : t(toolNames[key] || key),
        children:
          tab === 'platforms'
            ? [
                ...packages.filter((p) => !p.id.startsWith('system-images;')).map(packageNode),
                ...imageNodes(packages, `${tab}:${key}`),
              ]
            : packages.map(packageNode),
      }));
  }, [data?.packages, tab, search, status, hideObsolete, selected, locale]);
  const expansionKey = (key: string) =>
    JSON.stringify([tab, search.trim().toLowerCase(), status, key]);
  const setAllExpanded = (open: boolean) => {
    const entries: [string, boolean][] = [];
    const visit = (nodes: SdkTreeNode[]) =>
      nodes.forEach((node) => {
        if (node.kind !== 'group') return;
        entries.push([expansionKey(node.key), open]);
        visit(node.children);
      });
    visit(groups);
    setExpanded((values) => ({ ...values, ...Object.fromEntries(entries) }));
  };
  const installIds = Object.keys(selected).filter((id) => selected[id] === 'install');
  const removeIds = Object.keys(selected).filter((id) => selected[id] === 'remove');
  const save = async (settings: Settings) => {
    setError('');
    try {
      await refreshCatalog(async () => {
        await call('settings_save', { settings });
        setEditing(null);
        await refresh();
      });
    } catch (error) {
      setError(errorText(error));
    }
  };
  const saveSites = (values: SdkSite[]) =>
    data && void save({ ...data.settings, sdkSites: values });
  return (
    <div className="sdk-manager">
      <div className="sdk-overview">
        <div>
          <HardDrive size={22} />
          <div>
            <b>{data?.settings.sdkRoot ?? t('等待 SDK 环境')}</b>
            <small>
              {data?.packages.filter((p) => p.installed).length ?? 0}
              {t(' 个已安装组件 \u00B7 可用空间')}{' '}
              {data?.freeBytes ? `${(data.freeBytes / 1024 ** 3).toFixed(1)} GiB` : t('读取中')}
            </small>
          </div>
        </div>
        <Button
          disabled={busy || saving}
          onClick={() => void prepare('bootstrap', { purpose: 'usb' })}
        >
          {t('准备真机环境')}
        </Button>
        <Button
          disabled={busy || saving}
          onClick={() => void prepare('bootstrap', { purpose: 'emulator' })}
        >
          {t('准备模拟器环境')}
        </Button>
      </div>
      <div className="sdk-tabs tabs" role="group" aria-label={t('SDK 管理分类')}>
        {(
          [
            ['platforms', 'SDK Platforms'],
            ['tools', 'SDK Tools'],
            ['sites', 'SDK Update Sites'],
          ] as const
        ).map(([value, title]) => (
          <button
            key={value}
            aria-pressed={tab === value}
            className={tab === value ? 'selected' : ''}
            onClick={() => setTab(value)}
          >
            {title}
          </button>
        ))}
      </div>
      {(error || sites.error) && (
        <p className="error-banner" role="alert">
          {t(error || errorText(sites.error))}
        </p>
      )}
      {warnings.length > 0 && (
        <p className="warning-text" role="alert">
          {t('目录已部分更新：')}
          {warnings.map((warning) => t(warning)).join('; ')}
        </p>
      )}
      {tab === 'sites' ? (
        <>
          <div className="sdk-browser-toolbar">
            <p className="hint">
              {t(
                '内置站点提供官方组件；自定义站点支持 Android SDK 仓库 XML（repository / remotePackage 格式）。启用后刷新目录并参与组件安装。',
              )}
            </p>
            <Button
              disabled={!data || saving}
              onClick={() => setEditing({ index: -1, site: { name: '', url: '', enabled: true } })}
            >
              <Plus size={14} />
              {t('添加站点')}
            </Button>
          </div>
          <div className="sdk-sites">
            {sites.data?.map((site) => {
              const index = (data?.settings.sdkSites ?? []).findIndex(
                (value) => value.url === site.url,
              );
              return (
                <article className="sdk-site" key={site.url}>
                  <input
                    aria-label={t('启用 {0}', site.builtin ? t(site.name) : site.name)}
                    type="checkbox"
                    checked={site.enabled}
                    disabled={site.builtin || saving}
                    onChange={(event) =>
                      saveSites(
                        (data?.settings.sdkSites ?? []).map((value) =>
                          value.url === site.url
                            ? { ...value, enabled: event.target.checked }
                            : value,
                        ),
                      )
                    }
                  />
                  <div>
                    <strong>
                      {site.builtin ? t(site.name) : site.name}{' '}
                      {site.builtin && <span className="muted">{t('\u00B7 内置')}</span>}
                    </strong>
                    <code>{site.url}</code>
                    <small className={site.error ? 'warning-text' : 'muted'}>
                      {!site.enabled
                        ? t('已停用')
                        : t(site.error) ||
                          (site.checkedAt
                            ? t(
                                '{0} 个组件 \u00B7 {1}',
                                site.packageCount,
                                new Date(site.checkedAt * 1000).toLocaleString(getLocale()),
                              )
                            : t('尚未刷新'))}
                    </small>
                  </div>
                  {!site.builtin && (
                    <div className="task-actions">
                      <Button
                        disabled={saving}
                        onClick={() =>
                          setEditing({
                            index,
                            site: { name: site.name, url: site.url, enabled: site.enabled },
                          })
                        }
                      >
                        {t('编辑')}
                      </Button>
                      <Button
                        disabled={saving}
                        onClick={() =>
                          saveSites(
                            (data?.settings.sdkSites ?? []).filter(
                              (value) => value.url !== site.url,
                            ),
                          )
                        }
                      >
                        {t('删除')}
                      </Button>
                    </div>
                  )}
                </article>
              );
            })}
            {sites.isPending && native && <p className="hint">{t('正在读取更新站点…')}</p>}
          </div>
        </>
      ) : (
        <>
          <div className="sdk-browser-toolbar">
            <label className="task-search">
              <Search size={15} />
              <input
                aria-label={t('搜索组件')}
                placeholder={
                  tab === 'platforms'
                    ? t('搜索 API、平台、源码、镜像或 ABI')
                    : t('搜索工具名称、版本或组件 ID')
                }
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
              {search && (
                <button aria-label={t('清空组件搜索')} onClick={() => setSearch('')}>
                  <X size={13} />
                </button>
              )}
            </label>
            <select
              aria-label={t('组件状态筛选')}
              value={status}
              onChange={(event) => setStatus(event.target.value)}
            >
              {[
                ['all', t('全部状态')],
                ['installed', t('已安装')],
                ['missing', t('未安装')],
                ['updates', t('有更新')],
                ['selected', t('待变更')],
              ].map(([value, text]) => (
                <option value={value} key={value}>
                  {text}
                </option>
              ))}
            </select>
            <button className="text-button" onClick={() => setAllExpanded(true)}>
              {t('展开全部')}
            </button>
            <button className="text-button" onClick={() => setAllExpanded(false)}>
              {t('收起全部')}
            </button>
          </div>
          <div className="sdk-catalog-options">
            <span>
              {groups.length}
              {t(' 个')}
              {tab === 'platforms' ? t('API 分组') : t('工具系列')} ·{' '}
              {groups.reduce((sum, group) => sum + group.packages.length, 0)}
              {t(' 个组件')}
            </span>
            <label>
              <input
                type="checkbox"
                checked={hideObsolete}
                onChange={(event) => setHideObsolete(event.target.checked)}
              />
              {t('隐藏过时组件（保留已安装项）')}
            </label>
            <label>
              <input
                type="checkbox"
                checked={(data?.settings.sdkChannel ?? 0) > 0}
                disabled={!data || busy || saving}
                onChange={(event) =>
                  data && void save({ ...data.settings, sdkChannel: event.target.checked ? 3 : 0 })
                }
              />
              {t('显示预览组件')}
            </label>
          </div>
          <div className="sdk-tree">
            <div className="sdk-tree-header">
              <span>{t('组件 / 版本层级')}</span>
              <span>API / ABI</span>
              <span>{t('版本')}</span>
              <span>{t('状态')}</span>
              <span>{t('变更')}</span>
            </div>
            {groups.map((group, index) => (
              <SdkTreeBranch
                key={group.key}
                node={group}
                depth={0}
                isOpen={(node, depth) =>
                  expanded[expansionKey(node.key)] ??
                  (Boolean(search.trim()) || status !== 'all' || (depth === 0 && index === 0))
                }
                toggle={(key, open) =>
                  setExpanded((values) => ({ ...values, [expansionKey(key)]: open }))
                }
                selected={selected}
                change={(id, action) => setSelected((values) => ({ ...values, [id]: action }))}
              />
            ))}
            {!groups.length && (
              <div className="empty-state">
                <h3>{t('没有匹配的组件')}</h3>
                <p>{t('调整关键词或状态；尚无远程目录时，先刷新仓库。')}</p>
              </div>
            )}
          </div>
        </>
      )}
      {(installIds.length > 0 || removeIds.length > 0) && (
        <div className="change-bar">
          <div>
            <b>
              {t('待执行变更 \u00B7 安装 / 更新 ')}
              {installIds.length}
              {t(' 项，卸载 ')}
              {removeIds.length}
              {t(' 项')}
            </b>
            <small>{t('选择跨分类保留，查看计划后才会执行。')}</small>
          </div>
          <Button disabled={busy} onClick={() => setSelected({})}>
            {t('清空选择')}
          </Button>
          <Button
            disabled={busy || !removeIds.length}
            onClick={() => void prepare('sdk_remove', { packages: removeIds.join('\n') })}
          >
            {t('确认卸载 ')}
            {removeIds.length}
            {t(' 项')}
          </Button>
          <Button
            primary
            disabled={busy || !installIds.length}
            onClick={() => void prepare('sdk_install', { packages: installIds.join('\n') })}
          >
            {t('确认安装 ')}
            {installIds.length}
            {t(' 项')}
          </Button>
        </div>
      )}
      {editing && (
        <Modal
          title={editing.index < 0 ? t('添加 SDK 更新站点') : t('编辑 SDK 更新站点')}
          onClose={() => setEditing(null)}
        >
          <Field label={t('站点名称')}>
            <input
              value={editing.site.name}
              onChange={(event) =>
                setEditing({ ...editing, site: { ...editing.site, name: event.target.value } })
              }
            />
          </Field>
          <Field
            label={t('仓库 XML 地址')}
            hint={t('使用 HTTPS，例如 https://example.com/android/repository.xml')}
          >
            <input
              value={editing.site.url}
              onChange={(event) =>
                setEditing({ ...editing, site: { ...editing.site, url: event.target.value } })
              }
            />
          </Field>
          {error && (
            <p className="error-banner" role="alert">
              {t(error)}
            </p>
          )}
          <div className="modal-actions">
            <Button onClick={() => setEditing(null)}>{t('取消')}</Button>
            <Button
              primary
              disabled={saving || !editing.site.name.trim() || !editing.site.url.trim()}
              onClick={() => {
                const values = [...(data?.settings.sdkSites ?? [])];
                const site = {
                  ...editing.site,
                  name: editing.site.name.trim(),
                  url: editing.site.url.trim(),
                };
                if (editing.index < 0) values.push(site);
                else values[editing.index] = site;
                saveSites(values);
              }}
            >
              {t('保存并刷新')}
            </Button>
          </div>
        </Modal>
      )}
    </div>
  );
}
function SdkTreeBranch({
  node,
  depth,
  isOpen,
  toggle,
  selected,
  change,
}: {
  node: SdkTreeNode;
  depth: number;
  isOpen: (
    node: Extract<
      SdkTreeNode,
      {
        kind: 'group';
      }
    >,
    depth: number,
  ) => boolean;
  toggle: (key: string, open: boolean) => void;
  selected: Record<string, string>;
  change: (id: string, action: string) => void;
}) {
  if (node.kind === 'package') {
    const p = node.package;
    return (
      <div className="sdk-package">
        <div style={{ paddingLeft: 25 + depth * 20 }}>
          <b>{p.name || p.id}</b>
          <small title={p.sourceUrl || p.id}>
            {p.id}
            {p.obsolete ? t(' \u00B7 已过时') : ''}
            {p.channel ? t(' \u00B7 预览') : ''}
          </small>
        </div>
        <span>
          {p.api || '—'}
          {(p.abi || p.id.startsWith('system-images;')) && <small>{p.abi || t(imageAbi(p))}</small>}
        </span>
        <span>
          {p.installed && !updated(p) ? p.installedVersion || p.version : p.version}
          <small>{updated(p) && t('已装 {0}', p.installedVersion)}</small>
        </span>
        <span className={p.installed ? 'installed' : 'muted'}>
          {updated(p) ? t('有更新') : p.installed ? t('已安装') : t('未安装')}
          {!p.installed && <small>{size(p.size)}</small>}
        </span>
        <select
          aria-label={t('{0} 变更', p.id)}
          value={selected[p.id] ?? ''}
          onChange={(event) => change(p.id, event.target.value)}
        >
          <option value="">{p.installed ? t('保留') : t('不安装')}</option>
          {updated(p) && <option value="install">{t('更新')}</option>}
          <option value={p.installed ? 'remove' : 'install'}>
            {p.installed ? t('卸载') : t('安装')}
          </option>
        </select>
      </div>
    );
  }
  const open = isOpen(node, depth);
  return (
    <section
      className={
        depth === 0
          ? 'sdk-group'
          : `sdk-subgroup ${depth === 1 ? 'sdk-image-family' : 'sdk-image-abi'}`
      }
    >
      <button
        className={`sdk-group-heading ${depth > 0 ? 'sdk-subgroup-heading' : ''}`}
        style={{ paddingLeft: 16 + depth * 20 }}
        aria-label={t(node.title)}
        aria-expanded={open}
        onClick={() => toggle(node.key, !open)}
      >
        {open ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
        <strong>{t(node.title)}</strong>
        {depth === 1 && <small>{t('系统镜像')}</small>}
        <span>
          {node.packages.filter((p) => p.installed).length} / {node.packages.length}
          {t(' 已安装')}
        </span>
        {node.packages.some(updated) && <b className="sdk-update-tag">{t('有更新')}</b>}
      </button>
      {open &&
        node.children.map((child) => (
          <SdkTreeBranch
            key={child.kind === 'group' ? child.key : child.package.id}
            node={child}
            depth={depth + 1}
            isOpen={isOpen}
            toggle={toggle}
            selected={selected}
            change={change}
          />
        ))}
    </section>
  );
}
