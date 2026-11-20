import http from 'node:http';

const permission = {
  id: 'per_e2e',
  sessionID: 'ses_e2e',
  permission: 'bash',
  patterns: ['*'],
  metadata: { command: 'rm -rf build' },
  always: [],
};
let replied = null;

const server = http.createServer((request, response) => {
  response.setHeader('content-type', 'application/json');

  if (request.method === 'GET' && request.url === '/health') {
    response.end('{"ok":true}');
    return;
  }
  if (request.method === 'GET' && request.url === '/permission') {
    response.end(JSON.stringify(replied ? [] : [permission]));
    return;
  }
  if (request.method === 'GET' && request.url === '/state') {
    response.end(JSON.stringify({ replied }));
    return;
  }
  if (request.method === 'POST' && request.url === '/reset') {
    replied = null;
    response.end('{"ok":true}');
    return;
  }
  if (
    request.method === 'POST' &&
    request.url === `/permission/${permission.id}/reply`
  ) {
    let body = '';
    request.on('data', (chunk) => {
      body += chunk;
    });
    request.on('end', () => {
      replied = JSON.parse(body || '{}').reply ?? null;
      response.end('true');
    });
    return;
  }

  response.statusCode = 404;
  response.end('{"error":"not found"}');
});

const port = Number(process.env.ELESDEE_OPENCODE_PORT ?? 4398);

server.listen(port, '127.0.0.1', () => {
  console.log(`fake OpenCode server on 127.0.0.1:${port}`);
});
