#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const DOC_PATTERNS = [
  /^core\/src\//,
  /^app\/src\//,
  /^src-tauri\/src\//,
  /^scripts\//,
  /^\.github\/workflows\//
];

const TRANSLATION_PATTERNS = [
  /^core\/src\//,
  /^app\/src\//,
  /^src-tauri\/src\//
];

const MEANINGLESS = new Set(['', '-', 'n/a', 'na', 'none', 'no', 'checked', 'ok', 'yes']);

function trailerValue(message, key) {
  const matches = [...message.matchAll(new RegExp(`^${key}:[ \\t]*(.*)$`, 'gim'))];
  if (!matches.length) return null;
  return matches[matches.length - 1][1].trim();
}

function isMeaningful(value) {
  if (value == null) return false;
  const normalized = value.trim().toLowerCase();
  return normalized.length >= 8 && !MEANINGLESS.has(normalized);
}

function isExempt(message) {
  return /^Merge /m.test(message) || /^chore\(release\):/m.test(message);
}

export function missingAcks(message, files) {
  if (isExempt(message)) return [];
  const errors = [];
  const docFiles = files.filter((file) => DOC_PATTERNS.some((pattern) => pattern.test(file)));
  const translationFiles = files.filter((file) =>
    TRANSLATION_PATTERNS.some((pattern) => pattern.test(file))
  );

  if (docFiles.length && !isMeaningful(trailerValue(message, 'Docs-Checked'))) {
    errors.push(
      `This commit touches code or workflows (${summarize(docFiles)}) but has no meaningful Docs-Checked: trailer. ` +
        'Say which docs you updated or verified, or why none apply, for example:\n' +
        '  Docs-Checked: updated .dev/SPEC.md and .dev/api.md for the resize endpoint\n' +
        '  Docs-Checked: verified AGENTS.md and .dev/hooks.md still match; no other doc surface changed'
    );
  }

  if (translationFiles.length && !isMeaningful(trailerValue(message, 'Translations-Checked'))) {
    errors.push(
      `This commit changes user-facing code (${summarize(translationFiles)}) but has no meaningful Translations-Checked: trailer. ` +
        'Say which language catalogs you updated or verified, or why none apply, for example:\n' +
        '  Translations-Checked: updated the English, French and German catalogs for the session menu\n' +
        '  Translations-Checked: no user-facing strings changed; existing catalogs still match'
    );
  }

  return errors;
}

function summarize(files) {
  const shown = files.slice(0, 5).join(', ');
  return files.length > 5 ? `${shown} and ${files.length - 5} more` : shown;
}

function main() {
  const file = process.argv[2];
  if (!file) {
    console.error('Usage: check-commit-acks.mjs <commit-message-file>');
    process.exit(2);
  }
  const message = readFileSync(file, 'utf8');
  let staged = '';
  try {
    staged = execFileSync('git', ['diff', '--cached', '--name-only', '--diff-filter=ACMR'], {
      encoding: 'utf8'
    });
  } catch {
    // Outside a Git checkout or before the index exists, there is nothing to check.
    process.exit(0);
  }
  const files = staged.split('\n').map((line) => line.trim()).filter(Boolean);
  const errors = missingAcks(message, files);
  if (errors.length) {
    console.error('\nCommit rejected:\n\n' + errors.map((error) => '- ' + error).join('\n\n') + '\n');
    process.exit(1);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) main();
