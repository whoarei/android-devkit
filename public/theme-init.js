// Paint the last acknowledged appearance before CSS/React. This cache is never
// imported as a preference: the native SQLite response always takes precedence.
(() => {
  let mode = 'system';
  let palette = 'default';
  try {
    const cached = JSON.parse(localStorage.getItem('android-devkit.appearance.v1') || 'null');
    const preview = JSON.parse(localStorage.getItem('android-devkit.preferences.v1') || 'null');
    const saved = cached || preview;
    const candidate = saved?.theme || localStorage.getItem('android-devkit.theme');
    if (['light', 'dark', 'system'].includes(candidate)) mode = candidate;
    if (
      [
        'default',
        'blue',
        'green',
        'yellow',
        'pink',
        'orange',
        'purple',
        'black',
        'custom',
      ].includes(saved?.colorTheme)
    )
      palette = saved.colorTheme;
  } catch {
    /* A cache is optional. */
  }
  const theme =
    mode === 'system'
      ? matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
      : mode;
  document.documentElement.dataset.theme = theme;
  document.documentElement.dataset.themeMode = mode;
  document.documentElement.dataset.colorTheme = palette;
  document.documentElement.style.colorScheme = theme;
  const color =
    palette === 'black'
      ? theme === 'dark'
        ? '#0c0c0f'
        : '#f5f5f6'
      : theme === 'dark'
        ? '#0a101c'
        : '#f1f5fb';
  document.documentElement.style.backgroundColor = color;
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', color);
})();
