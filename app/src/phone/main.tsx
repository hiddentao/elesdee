import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import PhoneApp from './PhoneApp';
import {
  LanguageProvider,
  PHONE_LANGUAGE_KEY,
} from '../i18n/LanguageProvider';
import '@xterm/xterm/css/xterm.css';
import '../styles/tokens.css';
import '../styles/app.css';
import '../styles/phone.css';

const root = document.getElementById('root');

if (!root) {
  throw new Error('The application root is missing.');
}

createRoot(root).render(
  <StrictMode>
    <LanguageProvider storageKey={PHONE_LANGUAGE_KEY}>
      <PhoneApp />
    </LanguageProvider>
  </StrictMode>,
);
