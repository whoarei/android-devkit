// All durable UI preferences pass through this store. Rust owns the schema,
// defaults and validation; desktop persistence belongs to SQLite, not WebView.
import { invoke, isTauri } from '@tauri-apps/api/core';
import { useState, useSyncExternalStore } from 'react';
import { defaultUserPreferences, type UserPreferences } from './bindings';

type EditableKey = Exclude<keyof UserPreferences, 'window' | 'zoomFactor'>;
type Patch = Partial<Pick<UserPreferences, EditableKey>>;
const native = isTauri();
const previewKey = 'android-devkit.preferences.v1';
const legacyKeys = {
  theme: 'android-devkit.theme',
  locale: 'android-devkit.locale',
  defaultTarget: 'android-devkit.default-target',
  autoCheckUpdates: 'android-devkit.auto-check-update',
} as const;
let preferences: UserPreferences = { ...defaultUserPreferences };
let initialized = false;
let error = '';
let failed: Patch = {};
const requested: Patch = {};
let queue: Promise<unknown> = Promise.resolve();
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
function publish() {
  const { theme, locale } = preferences;
  document.documentElement.lang = locale;
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  const color = theme === 'dark' ? '#111814' : '#f7f9f5';
  document.documentElement.style.backgroundColor = color;
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', color);
  listeners.forEach((listener) => listener());
}
function readLegacy(): Patch {
  const values: Patch = {};
  const theme = localStorage.getItem(legacyKeys.theme);
  const locale = localStorage.getItem(legacyKeys.locale);
  const target = localStorage.getItem(legacyKeys.defaultTarget);
  const autoCheck = localStorage.getItem(legacyKeys.autoCheckUpdates);
  if (theme === 'light' || theme === 'dark') values.theme = theme;
  if (locale === 'zh-CN' || locale === 'en-US') values.locale = locale;
  if (target !== null && (target === '' || /^(avd|device):/.test(target)))
    values.defaultTarget = target;
  if (autoCheck === 'true' || autoCheck === 'false') values.autoCheckUpdates = autoCheck === 'true';
  return values;
}

export async function initializePreferences(): Promise<void> {
  let legacy: Patch = {};
  try {
    legacy = readLegacy();
  } catch {
    /* SQLite remains usable without WebView storage. */
  }
  try {
    if (native) {
      const loaded = await invoke<UserPreferences>('preferences_load', { legacy });
      if (!loaded?.theme) throw Error('Missing preferences response');
      preferences = loaded;
    } else {
      const saved = JSON.parse(localStorage.getItem(previewKey) || '{}');
      preferences = { ...defaultUserPreferences, ...legacy, ...saved };
      localStorage.setItem(previewKey, JSON.stringify(preferences));
    }
    initialized = true;
    error = '';
    // Delete legacy values only after the authoritative store acknowledged them.
    try {
      Object.values(legacyKeys).forEach((key) => localStorage.removeItem(key));
    } catch {
      /* import is idempotent */
    }
  } catch {
    preferences = { ...defaultUserPreferences };
    error = '用户配置读取失败，请重试。';
  }
  publish();
}

export const getPreference = <K extends keyof UserPreferences>(key: K): UserPreferences[K] =>
  preferences[key];
export const usePreferenceValue = <K extends keyof UserPreferences>(key: K): UserPreferences[K] =>
  useSyncExternalStore(
    subscribe,
    () => preferences[key],
    () => defaultUserPreferences[key],
  );
export const usePreferenceError = () =>
  useSyncExternalStore(
    subscribe,
    () => error,
    () => '',
  );

export const usePreferencesReady = () =>
  useSyncExternalStore(
    subscribe,
    () => initialized,
    () => false,
  );

export function savePreferences(patch: Patch): Promise<void> {
  Object.assign(requested, patch);
  const work = async () => {
    try {
      if (!initialized) throw Error('Preferences have not loaded');
      const next = native
        ? await invoke<UserPreferences>('preferences_patch', { patch })
        : { ...preferences, ...patch };
      if (!next?.theme) throw Error('Missing preferences response');
      if (!native) localStorage.setItem(previewKey, JSON.stringify(next));
      preferences = next;
      for (const key of Object.keys(patch) as EditableKey[]) {
        delete failed[key];
        if (requested[key] === patch[key]) delete requested[key];
      }
      if (!Object.keys(failed).length) error = '';
      publish();
    } catch (cause) {
      failed = { ...failed, ...patch };
      error = '用户配置保存失败，本次修改尚未保存。';
      publish();
      throw cause;
    }
  };
  const result = queue.then(work, work);
  queue = result.catch(() => {});
  return result;
}
export const savePreference = <K extends EditableKey>(key: K, value: UserPreferences[K]) =>
  savePreferences({ [key]: value } as Patch);
export function rememberPreference<K extends EditableKey>(key: K, value: UserPreferences[K]) {
  const previous = key in requested ? requested[key] : preferences[key];
  if (previous !== value) void savePreference(key, value).catch(() => {});
}
export function usePreference<K extends EditableKey>(key: K) {
  return [
    usePreferenceValue(key),
    (value: UserPreferences[K]) => rememberPreference(key, value),
  ] as const;
}
// A numeric input can temporarily be blank/out of range while being edited.
export function useNumberPreference(
  key: 'recordingSeconds' | 'logQuotaMib' | 'logMinutes',
  min: number,
  max: number,
) {
  const [draft, setDraft] = useState(() => getPreference(key));
  return [
    draft,
    (value: number) => {
      setDraft(value);
      if (Number.isInteger(value) && value >= min && value <= max) rememberPreference(key, value);
    },
  ] as const;
}
export async function retryPreferences() {
  if (!initialized) await initializePreferences();
  if (initialized && Object.keys(failed).length) await savePreferences({ ...failed });
}
