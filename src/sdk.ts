import { t } from './i18n';
import type { SdkPackage } from './bindings';
export type SdkTreeNode =
  | {
      kind: 'package';
      package: SdkPackage;
    }
  | {
      kind: 'group';
      key: string;
      title: string;
      packages: SdkPackage[];
      children: SdkTreeNode[];
    };
export const packageNode = (p: SdkPackage): SdkTreeNode => ({ kind: 'package', package: p });
export function imageFamily(p: SdkPackage) {
  // The canonical ID retains image variants when metadata uses a generic tag (e.g. 16 KB).
  const tag = p.id.split(';')[2] || p.tag || 'unknown';
  if (/^google_apis_playstore(?:_|$)/.test(tag))
    return { key: 'play', title: 'Google Play', order: 2 };
  if (/^google_apis(?:_|$)/.test(tag)) return { key: 'apis', title: 'Google APIs', order: 1 };
  if (tag === 'default' || /^aosp(?:_|$)/.test(tag))
    return { key: 'aosp', title: 'AOSP', order: 0 };
  return { key: `other:${tag}`, title: t('其他镜像 \u00B7 {0}', tag), order: 3 };
}
export const imageAbi = (p: SdkPackage) => p.id.split(';')[3] || p.abi || '未标注 ABI';
export function imageNodes(packages: SdkPackage[], parent: string): SdkTreeNode[] {
  const families = new Map<
    string,
    {
      title: string;
      order: number;
      packages: SdkPackage[];
    }
  >();
  for (const p of packages.filter((p) => p.id.startsWith('system-images;'))) {
    const family = imageFamily(p);
    if (!families.has(family.key)) families.set(family.key, { ...family, packages: [] });
    families.get(family.key)!.packages.push(p);
  }
  return [...families.entries()]
    .sort((a, b) => a[1].order - b[1].order || a[0].localeCompare(b[0]))
    .map(([key, family]) => {
      const abis = new Map<string, SdkPackage[]>();
      for (const p of family.packages) {
        const abi = imageAbi(p);
        if (!abis.has(abi)) abis.set(abi, []);
        abis.get(abi)!.push(p);
      }
      const path = `${parent}:image:${key}`;
      const order = ['x86_64', 'arm64-v8a', 'x86', 'armeabi-v7a'];
      return {
        kind: 'group' as const,
        key: path,
        title: family.title,
        packages: family.packages,
        children: [...abis.entries()]
          .sort(([a], [b]) => {
            const rank = (abi: string) => (order.includes(abi) ? order.indexOf(abi) : order.length);
            return rank(a) - rank(b) || a.localeCompare(b);
          })
          .map(([abi, packages]) => ({
            kind: 'group' as const,
            key: `${path}:abi:${abi}`,
            title: abi,
            packages,
            children: packages.map(packageNode),
          })),
      };
    });
}
export function hasSdkUpdate(p: SdkPackage): boolean {
  if (!p.installed || !p.installedVersion) return false;
  const revision = (value: string) => {
    const [release, preview] = value.split('-rc');
    const parts = release.split('.').map(Number);
    return [parts[0] || 0, parts[1] || 0, parts[2] || 0, preview ? 0 : 1, Number(preview) || 0];
  };
  const remote = revision(p.version);
  const local = revision(p.installedVersion);
  const index = remote.findIndex((value, index) => value !== local[index]);
  return index >= 0 && remote[index] > local[index];
}
