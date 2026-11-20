import de from './de.json';
import en from './en.json';
import es from './es.json';
import fr from './fr.json';
import it from './it.json';
import ja from './ja.json';
import ko from './ko.json';
import nl from './nl.json';
import pl from './pl.json';
import ptBR from './pt-BR.json';
import sv from './sv.json';
import zhCN from './zh-CN.json';

export const LANGUAGES = [
  { code: 'en', htmlLang: 'en' },
  { code: 'de', htmlLang: 'de' },
  { code: 'fr', htmlLang: 'fr' },
  { code: 'es', htmlLang: 'es' },
  { code: 'it', htmlLang: 'it' },
  { code: 'pt-BR', htmlLang: 'pt-BR' },
  { code: 'nl', htmlLang: 'nl' },
  { code: 'pl', htmlLang: 'pl' },
  { code: 'sv', htmlLang: 'sv' },
  { code: 'zh-CN', htmlLang: 'zh-CN' },
  { code: 'ja', htmlLang: 'ja' },
  { code: 'ko', htmlLang: 'ko' },
] as const;

export type Language = (typeof LANGUAGES)[number]['code'];
export type MessageKey = keyof typeof en;
export type MessageValues = Record<string, string | number>;
export type Translate = (key: MessageKey, values?: MessageValues) => string;

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

function normalizeTag(tag: string): string {
  return tag.trim().replace(/_/g, '-').toLowerCase();
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

export function htmlLanguage(language: Language): string {
  return (
    LANGUAGES.find((candidate) => candidate.code === language)?.htmlLang ?? 'en'
  );
}

export function createTranslator(language: Language): Translate {
  const catalog = CATALOGS[language] ?? en;
  return (key, values) => {
    const template = catalog[key] ?? en[key] ?? key;
    if (!values) {
      return template;
    }
    return template.replace(/\{(\w+)\}/g, (match, name: string) =>
      name in values ? String(values[name]) : match,
    );
  };
}
