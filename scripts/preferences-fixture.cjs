// Shared SQLite preference IPC double for browser UI fixtures. Keep persistence
// across page reloads, without using the deprecated product localStorage keys.
const fs = require('node:fs');
const path = require('node:path');
const bindings = fs.readFileSync(path.join(__dirname, '../src/bindings.ts'), 'utf8');
const defaults = JSON.parse(
  bindings.match(/export const defaultUserPreferences: UserPreferences = (\{[\s\S]*?\});/)[1],
);

module.exports = async function installPreferencesFixture(page) {
  await page.addInitScript((defaults) => {
    const key = 'android-devkit.fixture.preferences';
    let internal = window.__TAURI_INTERNALS__;
    const wrap = (value) => {
      if (!value || value.__preferenceFixture) return value;
      value.__preferenceFixture = true;
      const original = value.invoke.bind(value);
      value.invoke = async (command, args) => {
        if (command === 'preferences_load' || command === 'preferences_patch') {
          if (window.__failPreferences === 'load' && command === 'preferences_load')
            throw { message: 'Injected preferences read failure' };
          if (window.__failPreferences === 'save' && command === 'preferences_patch')
            throw { message: 'Injected preferences write failure' };
          const stored = JSON.parse(localStorage.getItem(key) || '{}');
          if (command === 'preferences_load') {
            for (const [field, value] of Object.entries(args?.legacy || {})) {
              if (!(field in stored)) stored[field] = value;
            }
          } else {
            if (window.__preferenceDelay)
              await new Promise((resolve) => setTimeout(resolve, window.__preferenceDelay));
            Object.assign(stored, args.patch);
          }
          localStorage.setItem(key, JSON.stringify(stored));
          window.__preferences = { ...defaults, ...stored };
          return window.__preferences;
        }
        return original(command, args);
      };
      return value;
    };
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      get: () => internal,
      set: (value) => {
        internal = wrap(value);
      },
    });
    internal = wrap(internal);
  }, defaults);
};
