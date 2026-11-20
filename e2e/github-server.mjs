import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const port = Number(process.env.ELESDEE_GITHUB_PORT ?? 4396);
const runDir = process.env.ELESDEE_GITHUB_RUN_DIR;
const token = 'e2e-github-token';
const modeFile = path.join(runDir, 'github-mode.txt');
const demoClone = `file://${path.join(runDir, 'github', 'demo.git')}`;

function mode() {
  try {
    return fs.readFileSync(modeFile, 'utf8').trim();
  } catch {
    return 'success';
  }
}

function send(response, status, body) {
  const payload = JSON.stringify(body);
  response.writeHead(status, {
    'Content-Type': 'application/json',
    'Content-Length': Buffer.byteLength(payload),
  });
  response.end(payload);
}

function authorized(request) {
  return request.headers.authorization === `Bearer ${token}`;
}

const repos = [
  {
    id: 1,
    full_name: 'acme/demo',
    name: 'demo',
    owner: { login: 'acme' },
    private: false,
    clone_url: demoClone,
    html_url: 'https://example.invalid/acme/demo',
    description: 'The demo repository',
    updated_at: '2026-01-01T00:00:00Z',
  },
];

const server = http.createServer((request, response) => {
  const url = new URL(request.url, `http://127.0.0.1:${port}`);

  if (url.pathname === '/health') {
    send(response, 200, { ok: true });
    return;
  }
  if (url.pathname === '/login/device/code') {
    send(response, 200, {
      device_code: 'e2e-device-code',
      user_code: 'TEST-CODE',
      verification_uri: 'https://github.com/login/device',
      expires_in: 900,
      interval: 1,
    });
    return;
  }
  if (url.pathname === '/login/oauth/access_token') {
    if (mode() === 'denied') {
      send(response, 200, { error: 'access_denied' });
      return;
    }
    send(response, 200, {
      access_token: token,
      token_type: 'bearer',
      scope: 'repo',
    });
    return;
  }
  if (url.pathname === '/user') {
    if (!authorized(request)) {
      send(response, 401, { message: 'Bad credentials' });
      return;
    }
    send(response, 200, {
      login: 'octocat',
      name: 'The Octocat',
      avatar_url: null,
    });
    return;
  }
  if (url.pathname === '/user/repos') {
    if (!authorized(request)) {
      send(response, 401, { message: 'Bad credentials' });
      return;
    }
    send(response, 200, repos);
    return;
  }

  send(response, 404, { message: 'Not found' });
});

server.listen(port, '127.0.0.1', () => {
  console.log(`fake GitHub listening on ${port}`);
});
