import './i18n';
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';
import { ThemeProvider } from '@/components/theme/theme-provider';
import { AdvancedModeProvider } from '@/components/AdvancedModeContext';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ThemeProvider>
      <AdvancedModeProvider>
        <App />
      </AdvancedModeProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
