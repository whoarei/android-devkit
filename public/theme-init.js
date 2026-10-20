// Apply the saved palette before the application or stylesheets finish loading.
(() => {
  let theme = 'light';
  try {
    if (localStorage.getItem('android-devkit.theme') === 'dark') theme = 'dark';
  } catch {}
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  const color = theme === 'dark' ? '#111814' : '#f7f9f5';
  document.documentElement.style.backgroundColor = color;
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', color);
})();
