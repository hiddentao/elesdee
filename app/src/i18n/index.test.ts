import { describe, expect, it } from 'vitest';
import { de } from './de';
import { en, type MessageKey } from './en';
import { es } from './es';
import { fr } from './fr';
import {
  formatNumber,
  htmlLang,
  LANGUAGES,
  resolveLanguage,
  translate,
} from './index';
import { it as itCatalog } from './it';
import { ja } from './ja';
import { ko } from './ko';
import { nl } from './nl';
import { pl } from './pl';
import { ptBR } from './pt-BR';
import { sv } from './sv';
import { zhCN } from './zh-CN';

const catalogs: Record<string, Record<MessageKey, string>> = {
  en,
  de,
  fr,
  es,
  it: itCatalog,
  'pt-BR': ptBR,
  nl,
  pl,
  sv,
  'zh-CN': zhCN,
  ja,
  ko,
};

function placeholders(value: string): string[] {
  return [...value.matchAll(/\{(\w+)\}/g)]
    .map((match) => match[1])
    .sort();
}

describe('language catalogs', () => {
  const englishKeys = Object.keys(en).sort() as MessageKey[];

  it('lists the twelve supported languages', () => {
    expect(LANGUAGES.map((language) => language.code)).toEqual([
      'en',
      'de',
      'fr',
      'es',
      'it',
      'pt-BR',
      'nl',
      'pl',
      'sv',
      'zh-CN',
      'ja',
      'ko',
    ]);
  });

  it.each(Object.keys(catalogs))('%s has every English key', (language) => {
    expect(Object.keys(catalogs[language]).sort()).toEqual(englishKeys);
  });

  it.each(Object.keys(catalogs))(
    '%s keeps every placeholder of the English text',
    (language) => {
      for (const key of englishKeys) {
        expect(placeholders(catalogs[language][key]), key).toEqual(
          placeholders(en[key]),
        );
      }
    },
  );

  it.each(Object.keys(catalogs))('%s has no empty text', (language) => {
    for (const key of englishKeys) {
      expect(catalogs[language][key].trim(), key).not.toBe('');
    }
  });
});

describe('resolveLanguage', () => {
  it('uses a supported language tag', () => {
    expect(resolveLanguage('de-DE')).toBe('de');
    expect(resolveLanguage('fr')).toBe('fr');
    expect(resolveLanguage('es-MX')).toBe('es');
    expect(resolveLanguage('it-CH')).toBe('it');
    expect(resolveLanguage('nl-BE')).toBe('nl');
    expect(resolveLanguage('pl-PL')).toBe('pl');
    expect(resolveLanguage('sv-SE')).toBe('sv');
    expect(resolveLanguage('ja-JP')).toBe('ja');
    expect(resolveLanguage('ko-KR')).toBe('ko');
    expect(resolveLanguage('en-GB')).toBe('en');
  });

  it('falls back from European Portuguese to Brazilian Portuguese', () => {
    expect(resolveLanguage('pt-PT')).toBe('pt-BR');
    expect(resolveLanguage('pt')).toBe('pt-BR');
    expect(resolveLanguage('pt_BR')).toBe('pt-BR');
  });

  it('falls back from Traditional Chinese to Simplified Chinese', () => {
    expect(resolveLanguage('zh-TW')).toBe('zh-CN');
    expect(resolveLanguage('zh-HK')).toBe('zh-CN');
    expect(resolveLanguage('zh-Hant-TW')).toBe('zh-CN');
    expect(resolveLanguage('zh')).toBe('zh-CN');
  });

  it('uses the first supported tag in a list', () => {
    expect(resolveLanguage(['xx-YY', 'it-CH', 'de-DE'])).toBe('it');
  });

  it('falls back to English for an unknown or empty language', () => {
    expect(resolveLanguage('xx-YY')).toBe('en');
    expect(resolveLanguage('')).toBe('en');
    expect(resolveLanguage([])).toBe('en');
    expect(resolveLanguage(null)).toBe('en');
    expect(resolveLanguage(undefined)).toBe('en');
  });
});

describe('translate', () => {
  it('reads the selected catalog', () => {
    expect(translate('de', 'status.closed')).toBe('Geschlossen');
    expect(translate('ja', 'status.closed')).toBe('クローズ済み');
    expect(translate('zh-CN', 'sidebar.sessions')).toBe('会话');
  });

  it('replaces named values', () => {
    expect(
      translate('de', 'sidebar.projectSessionsMany', {
        name: 'web',
        count: 3,
      }),
    ).toBe('web, 3 Sitzungen');
  });

  it('keeps an unknown value token unchanged', () => {
    expect(translate('en', 'status.activeWith')).toBe('Active, {detail}');
  });
});

describe('htmlLang and formatNumber', () => {
  it('maps a language to its html tag', () => {
    expect(htmlLang('pt-BR')).toBe('pt-BR');
    expect(htmlLang('zh-CN')).toBe('zh-CN');
    expect(htmlLang('en')).toBe('en');
  });

  it('formats numbers with the language', () => {
    expect(formatNumber('en', 1234)).toBe('1,234');
    expect(formatNumber('de', 1234)).toBe('1.234');
  });
});
