import fs from 'node:fs';
import path from 'node:path';
import { chromium, defineConfig } from '@playwright/test';
import { createRunDir } from './e2e/env';

const runDir = createRunDir();
const corePort = Number(process.env.ELESDEE_E2E_CORE_PORT ?? 4399);
const webPort = Number(process.env.ELESDEE_E2E_WEB_PORT ?? 5199);
const opencodePort = Number(process.env.ELESDEE_E2E_OPENCODE_PORT ?? 4398);
const codexPort = Number(process.env.ELESDEE_E2E_CODEX_PORT ?? 4397);
const chromePath = process.env.ELESDEE_E2E_CHROME ?? chromium.executablePath();
const agentStub = path.join(runDir, 'agent-stub.sh');

fs.writeFileSync(
  agentStub,
  '#!/bin/sh\nfor arg in "$@"; do printf \'arg:%s\\n\' "$arg"; done\nprintf \'env-anthropic-base:%s\\n\' "${ANTHROPIC_BASE_URL:-}"\nprintf \'env-anthropic-token:%s\\n\' "${ANTHROPIC_AUTH_TOKEN:-}"\nprintf \'env-anthropic-key:%s\\n\' "${ANTHROPIC_API_KEY:-}"\nprintf \'env-openai-key:%s\\n\' "${OPENAI_API_KEY:-}"\nprintf \'env-openai-base:%s\\n\' "${OPENAI_BASE_URL:-}"\nprintf \'env-opencode-config:%s\\n\' "${OPENCODE_CONFIG:-}"\nprintf \'env-e2e-value:%s\\n\' "${E2E_VALUE:-}"\nwhile IFS= read -r line; do printf \'input:%s\\n\' "$line"; done\n',
);
fs.chmodSync(agentStub, 0o755);

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
      command: `ELESDEE_PORT=${corePort} ELESDEE_DATA_DIR=${JSON.stringify(path.join(runDir, 'data'))} ELESDEE_AGENT_BIN=${JSON.stringify(agentStub)} ELESDEE_MEMORY_SECRETS=1 ELESDEE_OPENCODE_PORT=${opencodePort} ELESDEE_CODEX_WS=ws://127.0.0.1:${codexPort} CHROME=${JSON.stringify(chromePath)} cargo run --manifest-path core/Cargo.toml`,
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
      command: `ELESDEE_API_PORT=${corePort} npm --workspace app run dev -- --port ${webPort} --strictPort --host 127.0.0.1`,
      url: `http://127.0.0.1:${webPort}`,
      timeout: 120_000,
      reuseExistingServer: false,
      stdout: 'pipe',
      stderr: 'pipe',
    },
  ],
});
