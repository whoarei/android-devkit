import { rememberPreference, usePreferenceValue } from './preferences';
import type { UserPreferences } from './bindings';

export type Theme = UserPreferences['theme'];
export const useTheme = () => usePreferenceValue('theme');
export const setTheme = (value: Theme) => rememberPreference('theme', value);
