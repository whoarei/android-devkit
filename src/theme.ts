import { rememberPreference, usePreferenceValue } from './preferences';
import type { UserPreferences } from './bindings';
import { useSyncExternalStore } from 'react';
import { getAppearanceSignature, getResolvedTheme, subscribeAppearance } from './appearance';

export type Theme = UserPreferences['theme'];
export const useTheme = () => usePreferenceValue('theme');
export const setTheme = (value: Theme) => rememberPreference('theme', value);
export const useResolvedTheme = () => useSyncExternalStore(subscribeAppearance, getResolvedTheme);
export const useAppearance = () =>
  useSyncExternalStore(subscribeAppearance, getAppearanceSignature);
