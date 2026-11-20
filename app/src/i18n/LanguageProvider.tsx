import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import {
  htmlLang,
  isStoredLanguage,
  resolveLanguage,
  translate,
  type Language,
  type Translate,
} from './index';

export const DESKTOP_LANGUAGE_KEY = 'elesdee.language';
export const PHONE_LANGUAGE_KEY = 'elesdee.phone-language';

interface LanguageContextValue {
  language: Language;
  setLanguage: (language: Language) => void;
  t: Translate;
}

const englishContext: LanguageContextValue = {
  language: 'en',
  setLanguage: () => undefined,
  t: (key, values) => translate('en', key, values),
};

const LanguageContext = createContext<LanguageContextValue>(englishContext);

function systemLanguages(): readonly string[] {
  if (typeof navigator === 'undefined') {
    return [];
  }

  if (Array.isArray(navigator.languages) && navigator.languages.length > 0) {
    return navigator.languages;
  }

  return typeof navigator.language === 'string' ? [navigator.language] : [];
}

function readStored(storageKey: string): Language | null {
  try {
    const stored = window.localStorage.getItem(storageKey);
    if (!stored) {
      return null;
    }
    return isStoredLanguage(stored) ? stored : resolveLanguage(stored);
  } catch {
    return null;
  }
}

function writeStored(storageKey: string, language: Language): void {
  try {
    window.localStorage.setItem(storageKey, language);
  } catch {
    // A private window or blocked storage keeps the choice for this session.
  }
}

function initialLanguage(storageKey: string): Language {
  return readStored(storageKey) ?? resolveLanguage(systemLanguages());
}

interface LanguageProviderProps {
  storageKey?: string;
  children: ReactNode;
}

export function LanguageProvider({
  storageKey = DESKTOP_LANGUAGE_KEY,
  children,
}: LanguageProviderProps) {
  const [language, setLanguageState] = useState<Language>(() =>
    initialLanguage(storageKey),
  );

  useEffect(() => {
    document.documentElement.lang = htmlLang(language);
  }, [language]);

  const setLanguage = useCallback(
    (next: Language) => {
      setLanguageState(next);
      writeStored(storageKey, next);
    },
    [storageKey],
  );

  const t = useCallback<Translate>(
    (key, values) => translate(language, key, values),
    [language],
  );

  const value = useMemo(
    () => ({ language, setLanguage, t }),
    [language, setLanguage, t],
  );

  return (
    <LanguageContext.Provider value={value}>
      {children}
    </LanguageContext.Provider>
  );
}

export function useLanguage(): LanguageContextValue {
  return useContext(LanguageContext);
}
