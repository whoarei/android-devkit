import { useSyncExternalStore } from 'react';

export type Theme = 'light' | 'dark';
const storageKey = 'android-devkit.theme';
const read = (): Theme => {
  try {
    return localStorage.getItem(storageKey) === 'dark' ? 'dark' : 'light';
  } catch {
    return 'light';
  }
};
let theme = read();
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
const apply = () => {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  const color = theme === 'dark' ? '#111814' : '#f7f9f5';
  document.documentElement.style.backgroundColor = color;
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', color);
};
export const useTheme = () =>
  useSyncExternalStore(
    subscribe,
    () => theme,
    () => 'light' as Theme,
  );
export function setTheme(value: Theme) {
  if ((value !== 'light' && value !== 'dark') || value === theme) return;
  theme = value;
  try {
    localStorage.setItem(storageKey, value);
  } catch {
    /* Keep the preference for this session. */
  }
  apply();
  listeners.forEach((listener) => listener());
}
apply();
