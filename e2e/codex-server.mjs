import http from 'node:http';
import { WebSocketServer } from 'ws';

let replied = null;
let connected = 0;

const server = http.createServer((request, response) => {
  response.setHeader('content-type', 'application/json');
  if (request.url === '/state') {
    response.end(JSON.stringify({ replied, connected }));
    return;
  }
  if (request.url === '/reset') {
    replied = null;
    response.end('{"ok":true}');
    return;
  }
  if (request.url === '/health') {
    response.end('{"ok":true}');
    return;
  }
  response.statusCode = 404;
  response.end('{"error":"not found"}');
});

const sockets = new WebSocketServer({ server });

sockets.on('connection', (socket) => {
  connected += 1;
  socket.on('message', (raw) => {
    let message;
    try {
      message = JSON.parse(String(raw));
    } catch {
      return;
    }
    if (message.method === 'initialize') {
      socket.send(
        JSON.stringify({
          jsonrpc: '2.0',
          id: message.id,
          result: {
            userAgent: 'elesdee-e2e',
            codexHome: '/tmp',
            platformFamily: 'unix',
            platformOs: 'macos',
          },
        }),
      );
      setTimeout(() => {
        if (socket.readyState !== 1) {
          return;
        }
        socket.send(
          JSON.stringify({
            jsonrpc: '2.0',
            id: 42,
            method: 'item/commandExecution/requestApproval',
            params: {
              command: 'rm -rf release',
              itemId: 'item-1',
              threadId: 'thread-1',
              turnId: 'turn-1',
              startedAtMs: Date.now(),
            },
          }),
        );
      }, 300);
      return;
    }
    if (message.id === 42 && message.result) {
      replied = message.result.decision ?? null;
    }
  });
});

server.listen(4397, '127.0.0.1', () => {
  console.log('fake Codex app server on 127.0.0.1:4397');
});
