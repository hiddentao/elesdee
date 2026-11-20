import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { dataDir, getRunDir, projectsDir, repoDir } from './env';

export default async function globalSetup(): Promise<void> {
  fs.mkdirSync(dataDir(), { recursive: true });
  fs.mkdirSync(projectsDir(), { recursive: true });

  const repo = repoDir();
  fs.mkdirSync(repo, { recursive: true });

  const git = (...args: string[]) =>
    execFileSync('git', args, { cwd: repo, stdio: 'pipe' });

  git('init', '-q', '-b', 'main');
  git('config', 'user.name', 'Elesdee E2E');
  git('config', 'user.email', 'e2e@example.com');
  fs.writeFileSync(path.join(repo, 'README.md'), 'elesdee e2e\n');
  git('add', '.');
  git('commit', '-qm', 'Initial commit');

  git('checkout', '-qb', 'feature');
  fs.writeFileSync(path.join(repo, 'feature.txt'), 'feature branch\n');
  git('add', '.');
  git('commit', '-qm', 'Feature commit');
  git('checkout', '-q', 'main');

  const github = path.join(getRunDir(), 'github');
  fs.mkdirSync(github, { recursive: true });
  const source = path.join(github, 'demo-source');
  fs.mkdirSync(source, { recursive: true });
  const demoGit = (...args: string[]) =>
    execFileSync('git', args, { cwd: source, stdio: 'pipe' });
  demoGit('init', '-q', '-b', 'main');
  demoGit('config', 'user.name', 'Elesdee E2E');
  demoGit('config', 'user.email', 'e2e@example.com');
  fs.writeFileSync(path.join(source, 'README.md'), 'github e2e\n');
  demoGit('add', '.');
  demoGit('commit', '-qm', 'Initial commit');
  execFileSync(
    'git',
    ['clone', '--quiet', '--bare', source, path.join(github, 'demo.git')],
    { stdio: 'pipe' },
  );
  execFileSync(
    'git',
    ['clone', '--quiet', '--bare', source, path.join(github, 'launch.git')],
    { stdio: 'pipe' },
  );
}
