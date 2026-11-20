#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

const root = new URL('..', import.meta.url).pathname.replace(/\/$/, '');
const args = process.argv.slice(2);

function option(name, fallback = null) {
  const at = args.indexOf(name);
  return at === -1 ? fallback : args[at + 1];
}
function flag(name) {
  return args.includes(name);
}

const type = option('--release-type', 'auto');
const apply = flag('--apply');
const dryRun = flag('--dry-run') || !apply;
const notesVersion = option('--notes');

function read(path) {
  return readFileSync(join(root, path), 'utf8');
}
function write(path, text) {
  writeFileSync(join(root, path), text);
}
function json(path) {
  return JSON.parse(read(path));
}
function writeJson(path, value) {
  write(path, JSON.stringify(value, null, 2) + '\n');
}
function git(...gitArgs) {
  return execFileSync('git', gitArgs, {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe']
  }).trim();
}

function notesFor(version) {
  if (!existsSync(join(root, 'CHANGELOG.md'))) return '';
  const text = read('CHANGELOG.md');
  const start = text.indexOf(`## ${version}`);
  if (start === -1) return '';
  const next = text.indexOf('\n## ', start + 3);
  return text.slice(start, next === -1 ? undefined : next).trim();
}

if (notesVersion) {
  process.stdout.write(notesFor(notesVersion) + '\n');
  process.exit(0);
}

const pkg = json('package.json');
const lastTag = (() => {
  try {
    return git('describe', '--tags', '--abbrev=0', '--match', '*.*.*');
  } catch {
    return null;
  }
})();
const range = lastTag ? `${lastTag}..HEAD` : 'HEAD';
const baseVersion = lastTag || pkg.version || '0.0.0';

const log = git('log', range, '--format=%H%x1f%s%x1f%b%x1e');
const commits = log
  .split('\x1e')
  .map((record) => record.trim())
  .filter(Boolean)
  .map((record) => {
    const [hash, subject, body = ''] = record.split('\x1f');
    const match = subject.match(/^([a-z]+)(\([^)]*\))?(!)?: (.+)$/);
    return {
      hash,
      subject,
      body,
      type: match ? match[1] : 'other',
      scope: match && match[2] ? match[2].slice(1, -1) : null,
      breaking: Boolean(match && match[3]) || /BREAKING[ -]CHANGE:/.test(body),
      description: match ? match[4] : subject
    };
  });

const breaking = commits.filter((c) => c.breaking);
const added = commits.filter((c) => c.type === 'feat' && !c.breaking);
const fixed = commits.filter((c) => c.type === 'fix' && !c.breaking);
const changed = commits.filter((c) => ['perf', 'refactor'].includes(c.type) && !c.breaking);

function bumpVersion(current, releaseType) {
  const [major, minor, patch] = current.split('.').map((n) => Number(n));
  if (releaseType === 'major') return `${major + 1}.0.0`;
  if (releaseType === 'minor') return `${major}.${minor + 1}.0`;
  if (releaseType === 'patch') return `${major}.${minor}.${patch + 1}`;
  if (breaking.length) return major === 0 ? `0.${minor + 1}.0` : `${major + 1}.0.0`;
  if (added.length) return `0.${minor + 1}.0`;
  return `0.${minor}.${patch + 1}`;
}

const version = bumpVersion(baseVersion, type);
const date = new Date().toISOString().slice(0, 10);

function group(title, list) {
  if (!list.length) return '';
  const lines = list.map((c) => {
    const scope = c.scope ? `**${c.scope}:** ` : '';
    return `- ${scope}${c.description} (${c.hash.slice(0, 7)})`;
  });
  return `### ${title}\n\n${lines.join('\n')}\n\n`;
}

const section =
  `## ${version} - ${date}\n\n` +
  group('Breaking changes', breaking) +
  group('Added', added) +
  group('Fixed', fixed) +
  group('Changed', changed);

if (dryRun) {
  console.log(`Next version: ${version} (from ${baseVersion}, ${commits.length} commits)`);
  console.log(section);
  process.exit(0);
}

if (git('status', '--porcelain')) {
  console.error('The working tree is dirty. Commit or stash your changes before preparing a release.');
  process.exit(1);
}

writeJson('package.json', { ...pkg, version });
writeJson('app/package.json', { ...json('app/package.json'), version });
writeJson('src-tauri/tauri.conf.json', { ...json('src-tauri/tauri.conf.json'), version });

for (const path of ['core/Cargo.toml', 'src-tauri/Cargo.toml']) {
  const text = read(path).replace(/^version = "[^"]+"/m, `version = "${version}"`);
  write(path, text);
}

function bumpLock(path, names) {
  const lock = json(path);
  let changed = false;
  for (const entry of lock.packages || []) {
    if (names.includes(entry.name) && entry.version) {
      entry.version = version;
      changed = true;
    }
  }
  if (changed) writeJson(path, lock);
}

if (existsSync(join(root, 'core/Cargo.lock'))) bumpLock('core/Cargo.lock', ['elesdee-core']);
if (existsSync(join(root, 'src-tauri/Cargo.lock'))) bumpLock('src-tauri/Cargo.lock', ['elesdee-desktop']);

const lockPath = 'package-lock.json';
if (existsSync(join(root, lockPath))) {
  const lock = json(lockPath);
  if (lock.version) lock.version = version;
  if (lock.packages && lock.packages['']) lock.packages[''].version = version;
  if (lock.packages && lock.packages['app']) lock.packages['app'].version = version;
  writeJson(lockPath, lock);
}

const changelogPath = 'CHANGELOG.md';
const existing = existsSync(join(root, changelogPath)) ? read(changelogPath) : '# Changelog\n\n';
const header = existing.startsWith('# Changelog') ? '# Changelog\n\n' : '';
const body = existing.slice(header.length).replace(/^\n+/, '');
write(changelogPath, `${header}${section}\n${body}`.trimEnd() + '\n');

if (process.env.GITHUB_OUTPUT) {
  const delimiter = `EOF_${Date.now()}`;
  writeFileSync(
    process.env.GITHUB_OUTPUT,
    `version=${version}\nchangelog<<${delimiter}\n${section}\n${delimiter}\n`,
    { flag: 'a' }
  );
}

console.log(`Prepared ${version}`);
console.log(section);
