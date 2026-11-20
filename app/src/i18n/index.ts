import { de } from './de';
import { en, type MessageKey } from './en';
import { es } from './es';
import { fr } from './fr';
import { it } from './it';
import { ja } from './ja';
import { ko } from './ko';
import { nl } from './nl';
import { pl } from './pl';
import { ptBR } from './pt-BR';
import { sv } from './sv';
import { zhCN } from './zh-CN';

export const LANGUAGES = [
  { code: 'en', label: 'English', htmlLang: 'en' },
  { code: 'de', label: 'Deutsch', htmlLang: 'de' },
  { code: 'fr', label: 'Français', htmlLang: 'fr' },
  { code: 'es', label: 'Español', htmlLang: 'es' },
  { code: 'it', label: 'Italiano', htmlLang: 'it' },
  { code: 'pt-BR', label: 'Português do Brasil', htmlLang: 'pt-BR' },
  { code: 'nl', label: 'Nederlands', htmlLang: 'nl' },
  { code: 'pl', label: 'Polski', htmlLang: 'pl' },
  { code: 'sv', label: 'Svenska', htmlLang: 'sv' },
  { code: 'zh-CN', label: '简体中文', htmlLang: 'zh-CN' },
  { code: 'ja', label: '日本語', htmlLang: 'ja' },
  { code: 'ko', label: '한국어', htmlLang: 'ko' },
] as const;

export type Language = (typeof LANGUAGES)[number]['code'];

const CATALOGS: Record<Language, Record<MessageKey, string>> = {
  en,
  de,
  fr,
  es,
  it,
  'pt-BR': ptBR,
  nl,
  pl,
  sv,
  'zh-CN': zhCN,
  ja,
  ko,
};

const REGIONAL_FALLBACKS: Record<string, Language> = {
  pt: 'pt-BR',
  'pt-pt': 'pt-BR',
  zh: 'zh-CN',
  'zh-tw': 'zh-CN',
  'zh-hk': 'zh-CN',
  'zh-mo': 'zh-CN',
  'zh-hant': 'zh-CN',
};

export type MessageValues = Record<string, string | number>;
export type Translate = (key: MessageKey, values?: MessageValues) => string;

function normalizeTag(tag: string): string {
  return tag.trim().replace(/_/g, '-').toLowerCase();
}

function isLanguage(value: string): value is Language {
  return LANGUAGES.some((language) => language.code.toLowerCase() === value);
}

function findLanguage(value: string): Language | null {
  const match = LANGUAGES.find(
    (language) => language.code.toLowerCase() === value,
  );
  return match ? match.code : null;
}

/**
 * Picks the first supported language from the given tags. European
 * Portuguese resolves to Brazilian Portuguese, Traditional Chinese to
 * Simplified Chinese, and an unknown language resolves to English.
 */
export function resolveLanguage(
  candidates: string | readonly string[] | null | undefined,
): Language {
  const list =
    candidates === null || candidates === undefined
      ? []
      : typeof candidates === 'string'
        ? [candidates]
        : candidates;

  for (const raw of list) {
    const tag = normalizeTag(raw);
    if (!tag) {
      continue;
    }

    const exact = findLanguage(tag);
    if (exact) {
      return exact;
    }

    const regional = REGIONAL_FALLBACKS[tag];
    if (regional) {
      return regional;
    }

    const primary = tag.split('-')[0];
    const primaryFallback = REGIONAL_FALLBACKS[primary];
    if (primaryFallback) {
      return primaryFallback;
    }

    const byPrimary = LANGUAGES.find(
      (language) => language.code.toLowerCase().split('-')[0] === primary,
    );
    if (byPrimary) {
      return byPrimary.code;
    }
  }

  return 'en';
}

export function isStoredLanguage(value: string | null): value is Language {
  return value !== null && isLanguage(value.toLowerCase());
}

export function htmlLang(language: Language): string {
  return (
    LANGUAGES.find((item) => item.code === language)?.htmlLang ?? 'en'
  );
}

export function translate(
  language: Language,
  key: MessageKey,
  values?: MessageValues,
): string {
  const catalog = CATALOGS[language] ?? en;
  const template = catalog[key] ?? en[key] ?? key;

  if (!values) {
    return template;
  }

  return template.replace(/\{(\w+)\}/g, (match, name: string) => {
    const value = values[name];
    return value === undefined ? match : String(value);
  });
}

export function formatNumber(language: Language, value: number): string {
  try {
    return new Intl.NumberFormat(htmlLang(language)).format(value);
  } catch {
    return String(value);
  }
}

export function formatDateTime(
  language: Language,
  value: string | null | undefined,
): string | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  try {
    return new Intl.DateTimeFormat(htmlLang(language), {
      dateStyle: 'short',
      timeStyle: 'short',
    }).format(date);
  } catch {
    return null;
  }
}

export const englishT: Translate = (key, values) =>
  translate('en', key, values);

export type { MessageKey };
