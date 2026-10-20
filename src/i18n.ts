import { useSyncExternalStore } from 'react';
import english from './locales/en.json';
import nativeEnglish from './locales/en-native.json';

export type Locale = 'zh-CN' | 'en-US';
const storageKey = 'android-devkit.locale';
const readLocale = (): Locale => {
  try {
    return localStorage.getItem(storageKey) === 'en-US' ? 'en-US' : 'zh-CN';
  } catch {
    return 'zh-CN';
  }
};
let locale = readLocale();
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
export const getLocale = () => locale;
export const useLocale = () => useSyncExternalStore(subscribe, getLocale, () => 'zh-CN' as Locale);
export function setLocale(value: Locale) {
  if (value !== 'zh-CN' && value !== 'en-US') return;
  if (value === locale) return;
  locale = value;
  try {
    localStorage.setItem(storageKey, value);
  } catch {
    /* The current session remains usable without storage. */
  }
  document.documentElement.lang = value;
  listeners.forEach((listener) => listener());
}
document.documentElement.lang = locale;

const messages: Record<string, string> = { ...nativeEnglish, ...english };
const patterns = Object.entries(messages)
  .filter(([source]) => /\{\d+\}/.test(source))
  .sort(([a], [b]) => b.replace(/\{\d+\}/g, '').length - a.replace(/\{\d+\}/g, '').length)
  .map(([source, target]) => {
    const indices: number[] = [];
    const escaped = source
      .split(/(\{\d+\})/)
      .map((part) => {
        const index = part.match(/^\{(\d+)\}$/);
        if (index) {
          indices.push(Number(index[1]));
          return '([\\s\\S]*?)';
        }
        return part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      })
      .join('');
    return { regex: new RegExp(`^${escaped}$`), target, indices };
  });
export function t(source: string, ...values: unknown[]): string {
  let result = source;
  if (locale === 'en-US') {
    const trimmed = source.trim();
    result =
      messages[source] ??
      (messages[trimmed] !== undefined
        ? `${source.slice(0, source.indexOf(trimmed))}${messages[trimmed]}${source.slice(source.indexOf(trimmed) + trimmed.length)}`
        : source);
    if (result === source && values.length === 0 && /[\u3400-\u9fff]/.test(source)) {
      for (const pattern of patterns) {
        const match = source.match(pattern.regex);
        if (match)
          return pattern.target.replace(/\{(\d+)\}/g, (placeholder, index) => {
            const position = pattern.indices.indexOf(Number(index));
            return position < 0 ? placeholder : match[position + 1];
          });
      }
    }
  }
  return result.replace(/\{(\d+)\}/g, (placeholder, index) =>
    Number(index) < values.length ? String(values[Number(index)] ?? '') : placeholder,
  );
}
