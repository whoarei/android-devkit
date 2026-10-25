import { getPreference, rememberPreference, usePreferenceValue } from './preferences';
import english from './locales/en.json';
import nativeEnglish from './locales/en-native.json';

export type Locale = 'zh-CN' | 'en-US';
export const getLocale = () => getPreference('locale');
export const useLocale = () => usePreferenceValue('locale');
export const setLocale = (value: Locale) => rememberPreference('locale', value);

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
  if (getLocale() === 'en-US') {
    const trimmed = source.trim();
    result =
      messages[source] ??
      (messages[trimmed] !== undefined
        ? `${source.slice(0, source.indexOf(trimmed))}${messages[trimmed]}${source.slice(source.indexOf(trimmed) + trimmed.length)}`
        : source);
    if (result === source && values.length === 0 && /[\u3400-\u9fff]/.test(source)) {
      const error = source.match(/^([A-Z][A-Z0-9_]+)：([\s\S]+)$/);
      if (error) return `${error[1]}: ${t(error[2])}`;
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
