import React from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App } from './App';
import { initializePreferences } from './preferences';
import './theme.css';
import './style.css';
import './compact.css';

// Suppress the WebView's default menu throughout the application.
document.addEventListener('contextmenu', (event) => event.preventDefault(), { capture: true });

const client = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});
void initializePreferences().then(() =>
  createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <QueryClientProvider client={client}>
        <App />
      </QueryClientProvider>
    </React.StrictMode>,
  ),
);
