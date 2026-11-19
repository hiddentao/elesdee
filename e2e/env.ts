import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const pointer = path.join(os.tmpdir(), 'elesdee-e2e-run-dir');

export function createRunDir(): string {
  if (process.env.ELESDEE_E2E_DIR) {
    return process.env.ELESDEE_E2E_DIR;
  }

  const runDir = fs.mkdtempSync(path.join(os.tmpdir(), 'elesdee-e2e-'));
  process.env.ELESDEE_E2E_DIR = runDir;
  fs.writeFileSync(pointer, runDir);
  return runDir;
}

export function getRunDir(): string {
  if (process.env.ELESDEE_E2E_DIR) {
    return process.env.ELESDEE_E2E_DIR;
  }

  return fs.readFileSync(pointer, 'utf8').trim();
}

export function dataDir(): string {
  return path.join(getRunDir(), 'data');
}

export function repoDir(): string {
  return path.join(getRunDir(), 'repo');
}

export function projectsDir(): string {
  return path.join(getRunDir(), 'projects');
}

export function removeRunDir(): void {
  fs.rmSync(getRunDir(), { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  fs.rmSync(pointer, { force: true });
}
