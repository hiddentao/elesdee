import fs from 'node:fs';
import path from 'node:path';
import { chromium, defineConfig } from '@playwright/test';
import { createRunDir } from './e2e/env';

const runDir = createRunDir();
const corePort = Number(process.env.ELESDEE_E2E_CORE_PORT ?? 4399);
const webPort = Number(process.env.ELESDEE_E2E_WEB_PORT ?? 5199);
const opencodePort = Number(process.env.ELESDEE_E2E_OPENCODE_PORT ?? 4398);
const codexPort = Number(process.env.ELESDEE_E2E_CODEX_PORT ?? 4397);
const githubPort = Number(process.env.ELESDEE_E2E_GITHUB_PORT ?? 4396);
const githubMode = path.join(runDir, 'github-mode.txt');
const chromePath = process.env.ELESDEE_E2E_CHROME ?? chromium.executablePath();
const agentStub = path.join(runDir, 'agent-stub.sh');
const signInStub = path.join(runDir, 'sign-in-stub.sh');
const signInControl = path.join(runDir, 'sign-in-control.txt');
const tailscaleBin = path.join(runDir, 'tailscale-bin');
const tailscaleCalls = path.join(runDir, 'tailscale-calls.txt');

fs.mkdirSync(tailscaleBin, { recursive: true });
fs.writeFileSync(tailscaleCalls, '');
fs.writeFileSync(
  path.join(tailscaleBin, 'tailscale'),
  `#!/bin/sh
if [ "$1 $2" = "status --json" ]; then
  printf '{"BackendState":"Running","Self":{"DNSName":"elesdee-e2e.tailnet.ts.net.","TailscaleIPs":["100.101.102.103","fd7a:115c:a1e0::1"]}}'
  exit 0
fi
if [ "$1" = "funnel" ]; then
  printf '%s\\n' "$*" >> '${tailscaleCalls}'
  if [ "$2" = "--bg" ]; then
    exit 0
  fi
  if [ "$2" = "--https=443" ] && [ "$4" = "off" ]; then
    exit 0
  fi
  # The foreground form never returns, so a regression waits for the client
  # timeout instead of passing.
  sleep 30
fi
exit 1
`,
);
fs.chmodSync(path.join(tailscaleBin, 'tailscale'), 0o755);
const corePath = `${tailscaleBin}${path.delimiter}${process.env.PATH ?? ''}`;

fs.writeFileSync(
  agentStub,
  '#!/bin/sh\nfor arg in "$@"; do printf \'arg:%s\\n\' "$arg"; done\nprintf \'env-anthropic-base:%s\\n\' "${ANTHROPIC_BASE_URL:-}"\nprintf \'env-anthropic-token:%s\\n\' "${ANTHROPIC_AUTH_TOKEN:-}"\nprintf \'env-anthropic-key:%s\\n\' "${ANTHROPIC_API_KEY:-}"\nprintf \'env-openai-key:%s\\n\' "${OPENAI_API_KEY:-}"\nprintf \'env-openai-base:%s\\n\' "${OPENAI_BASE_URL:-}"\nprintf \'env-opencode-config:%s\\n\' "${OPENCODE_CONFIG:-}"\nprintf \'env-e2e-value:%s\\n\' "${E2E_VALUE:-}"\nwhile IFS= read -r line; do printf \'input:%s\\n\' "$line"; done\n',
);
fs.chmodSync(agentStub, 0o755);

fs.writeFileSync(
  signInStub,
  `#!/bin/sh
mode=$(cat "$SIGNIN_CONTROL" 2>/dev/null || echo success)
case "$1 $2" in
  "auth status"|"login status")
    printf '{"loggedIn":true,"authMethod":"claude.ai"}'
    exit 0
    ;;
esac
printf 'Open the sign-in page: https://example.com/signin\\n'
if [ "$mode" = "refuse" ]; then
  exit 1
fi
if [ "$mode" = "wait" ]; then
  sleep 60
fi
exit 0
`,
);
fs.chmodSync(signInStub, 0o755);
fs.writeFileSync(signInControl, 'success');
fs.writeFileSync(githubMode, 'success');

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  timeout: 180_000,
  expect: { timeout: 15_000 },
  reporter: 'list',
  use: {
    baseURL: `http://127.0.0.1:${webPort}`,
    permissions: ['clipboard-read', 'clipboard-write'],
    viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { browserName: 'chromium' },
    },
  ],
  globalSetup: './e2e/global-setup.ts',
  globalTeardown: './e2e/global-teardown.ts',
  webServer: [
    {
      command: `PATH=${JSON.stringify(corePath)} ELESDEE_PORT=${corePort} ELESDEE_DEV=1 ELESDEE_DATA_DIR=${JSON.stringify(path.join(runDir, 'data'))} ELESDEE_AGENT_BIN=${JSON.stringify(agentStub)} ELESDEE_SIGNIN_BIN=${JSON.stringify(signInStub)} SIGNIN_CONTROL=${JSON.stringify(signInControl)} ELESDEE_SIGNIN_TIMEOUT_SECS=20 ELESDEE_MEMORY_SECRETS=1 ELESDEE_PAUSE_GRACE_MS=50 ELESDEE_OPENCODE_PORT=${opencodePort} ELESDEE_CODEX_WS=ws://127.0.0.1:${codexPort} ELESDEE_GITHUB_CLIENT_ID=test-client ELESDEE_GITHUB_API_BASE=http://127.0.0.1:${githubPort} ELESDEE_GITHUB_LOGIN_BASE=http://127.0.0.1:${githubPort} ELESDEE_GITHUB_DEFAULT_PARENT=${JSON.stringify(path.join(runDir, 'github-clones'))} CHROME=${JSON.stringify(chromePath)} cargo run --manifest-path core/Cargo.toml`,
      url: `http://127.0.0.1:${corePort}/api/health`,
      timeout: 300_000,
      reuseExistingServer: false,
      stdout: 'pipe',
      stderr: 'pipe',
    },
    {
      command: `ELESDEE_OPENCODE_PORT=${opencodePort} node e2e/opencode-server.mjs`,
      url: `http://127.0.0.1:${opencodePort}/health`,
      timeout: 30_000,
      reuseExistingServer: false,
      stdout: 'pipe',
      stderr: 'pipe',
    },
    {
      command: `ELESDEE_CODEX_PORT=${codexPort} node e2e/codex-server.mjs`,
      url: `http://127.0.0.1:${codexPort}/health`,
      timeout: 30_000,
      reuseExistingServer: false,
      stdout: 'pipe',
      stderr: 'pipe',
    },
    {
      command: `ELESDEE_GITHUB_PORT=${githubPort} ELESDEE_GITHUB_RUN_DIR=${JSON.stringify(runDir)} node e2e/github-server.mjs`,
      url: `http://127.0.0.1:${githubPort}/health`,
      timeout: 30_000,
      reuseExistingServer: false,
      stdout: 'pipe',
      stderr: 'pipe',
    },
    {
      command: `ELESDEE_API_PORT=${corePort} npm --workspace app run dev -- --port ${webPort} --strictPort --host 127.0.0.1`,
      url: `http://127.0.0.1:${webPort}`,
      timeout: 120_000,
      reuseExistingServer: false,
      stdout: 'pipe',
      stderr: 'pipe',
    },
  ],
});
