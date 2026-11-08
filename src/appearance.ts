import type { UserPreferences } from './bindings';

export type Appearance = Pick<UserPreferences, 'theme' | 'colorTheme' | 'customColor'>;
export type ResolvedTheme = 'light' | 'dark';
export const colorThemes = [
  { id: 'default', name: '默认', color: '#000000' },
  { id: 'blue', name: '蓝色', color: '#3563f5' },
  { id: 'green', name: '绿色', color: '#00b8a0' },
  { id: 'yellow', name: '黄色', color: '#ffcc33' },
  { id: 'pink', name: '粉色', color: '#f55ca6' },
  { id: 'orange', name: '橙色', color: '#ff7b61' },
  { id: 'purple', name: '紫色', color: '#a34dff' },
  { id: 'black', name: '黑色', color: '#000000' },
  { id: 'custom', name: '自定义', color: '#35b8bd' },
] as const;

export const validColor = (color: string) => /^#[\da-f]{6}$/i.test(color);
const channels = (hex: string) =>
  [1, 3, 5].map((index) => parseInt(hex.slice(index, index + 2), 16));
export function mixColor(first: string, second: string, amount: number) {
  const end = channels(second);
  return (
    '#' +
    channels(first)
      .map((value, index) =>
        Math.round(value + (end[index] - value) * amount)
          .toString(16)
          .padStart(2, '0'),
      )
      .join('')
  );
}
function luminance(color: string) {
  return channels(color)
    .map((value) => {
      const channel = value / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    })
    .reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
}
export function contrastRatio(first: string, second: string) {
  const a = luminance(first),
    b = luminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}
function readableColor(color: string, background: string, dark: boolean) {
  for (let step = 0; step <= 100; step++) {
    const candidate = mixColor(color, dark ? '#ffffff' : '#0a101c', step / 100);
    if (contrastRatio(candidate, background) >= 4.7) return candidate;
  }
  return dark ? '#ffffff' : '#0a101c';
}
export function appearanceTokens(preferences: Appearance, resolved: ResolvedTheme) {
  const dark = resolved === 'dark';
  const monochrome = preferences.colorTheme === 'black';
  const base =
    preferences.colorTheme === 'custom' && validColor(preferences.customColor)
      ? preferences.customColor.toLowerCase()
      : (colorThemes.find((theme) => theme.id === preferences.colorTheme)?.color ?? '#000000');
  const page = monochrome ? (dark ? '#0c0c0f' : '#f5f5f6') : dark ? '#0a101c' : '#f1f5fb';
  const surface = monochrome ? (dark ? '#1b1b20' : '#ffffff') : dark ? '#121e30' : '#ffffff';
  const raised = monochrome ? (dark ? '#292930' : '#ededf0') : dark ? '#19283e' : '#eaf0f9';
  // Selected surfaces are brighter than the raised surface in dark mode.
  // Use their brightest possible fill as the contrast floor for all accents.
  const accent = readableColor(base, dark ? mixColor(surface, '#ffffff', 0.14) : raised, dark);
  const fill = preferences.colorTheme === 'default' ? (dark ? '#e3e8ef' : '#172b46') : base;
  const onAccent =
    contrastRatio(fill, '#ffffff') >= 4.5
      ? '#ffffff'
      : contrastRatio(fill, '#101522') >= 4.5
        ? '#101522'
        : '#000000';
  return {
    '--page': page,
    '--surface': surface,
    '--surface-raised': raised,
    '--surface-muted': mixColor(surface, page, 0.5),
    '--surface-subtle': mixColor(surface, page, 0.2),
    '--sidebar': monochrome ? (dark ? '#121215' : '#fafafa') : dark ? '#0d1524' : '#f9fbff',
    '--titlebar': monochrome ? (dark ? '#121215' : '#fafafa') : dark ? '#0d1524' : '#f9fbff',
    '--line': monochrome ? (dark ? '#3b3b44' : '#d7d7dd') : dark ? '#2b3b52' : '#d7e0ee',
    '--text': dark ? '#edf4ff' : '#172b46',
    '--text-strong': dark ? '#f4f7ff' : '#14263f',
    '--text-body': dark ? '#dce6f5' : '#2b405b',
    '--text-secondary': dark ? '#b5c4da' : '#536883',
    '--muted': dark ? '#a7b7cf' : '#536883',
    '--accent': accent,
    '--accent-color': base,
    '--accent-fill': fill,
    '--on-accent': onAccent,
    '--accent-hover': mixColor(fill, onAccent === '#ffffff' ? '#000000' : '#ffffff', 0.09),
    '--focus': accent,
    '--icon': accent,
    '--border-strong': accent,
    '--soft': mixColor(surface, accent, dark ? 0.1 : 0.055),
    '--hover': mixColor(surface, accent, dark ? 0.12 : 0.07),
    '--selected': mixColor(surface, accent, dark ? 0.14 : 0.08),
    '--selected-text': accent,
    '--screen-from': mixColor(surface, accent, 0.22),
    '--screen-to': surface,
    '--screen-outline': accent,
    '--ambient': mixColor(page, accent, dark ? 0.085 : 0.055),
    '--terminal-background': dark ? '#091222' : '#102037',
    '--terminal-foreground': '#d7e5f4',
    '--terminal-cursor': readableColor(base, '#102037', true),
    '--terminal-selection': '#7299c460',
  };
}

const media = window.matchMedia('(prefers-color-scheme: dark)');
let current: Appearance = { theme: 'system', colorTheme: 'default', customColor: '#35b8bd' };
let signature = '';
let resolvedTheme: ResolvedTheme = media.matches ? 'dark' : 'light';
const listeners = new Set<() => void>();
export const getAppearanceSignature = () => signature;
export const getResolvedTheme = () => resolvedTheme;
export const subscribeAppearance = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
export function applyAppearance(preferences: Appearance) {
  current = preferences;
  resolvedTheme =
    preferences.theme === 'system' ? (media.matches ? 'dark' : 'light') : preferences.theme;
  const next = `${preferences.theme}:${resolvedTheme}:${preferences.colorTheme}:${preferences.customColor}`;
  if (next === signature) return;
  const root = document.documentElement;
  root.dataset.theme = resolvedTheme;
  root.dataset.themeMode = preferences.theme;
  root.dataset.colorTheme = preferences.colorTheme;
  root.style.colorScheme = resolvedTheme;
  const tokens = appearanceTokens(preferences, resolvedTheme);
  for (const [name, color] of Object.entries(tokens)) root.style.setProperty(name, color);
  root.style.backgroundColor = tokens['--page'];
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', tokens['--page']);
  signature = next;
  listeners.forEach((listener) => listener());
}
media.addEventListener('change', () => {
  if (current.theme === 'system') applyAppearance(current);
});
