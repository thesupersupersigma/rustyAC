// SPDX-License-Identifier: MIT OR Apache-2.0
// The public router behind one tunnel route (rustyac.com/net/x/*): the list of the on-demand
// servers that run, and each one's WebSocket (net/x/<id>) carried to that server's gateway.
// Only servers this panel started are known to it; every other path is refused.
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { connect, type Socket } from 'node:net';
import type { Running } from './servers.ts';

export interface Listed { id: string; name: string; url: string; cars: string[]; track: string; layout: string; players: number; maxPlayers: number; password: boolean }

export const listing = (servers: Running[]): { servers: Listed[] } => ({
  servers: servers.filter((r) => r.status === 'running').map((r) => ({
    id: `x-${r.id}`, name: r.preset.name, url: `/net/x/${r.id}`, cars: r.preset.cars.map((c) => c.model),
    track: r.preset.track, layout: r.preset.layout, players: r.players?.length ?? 0, maxPlayers: r.preset.maxPlayers, password: !!r.preset.password,
  })),
});

export function router(servers: () => Running[], gatewayHost: string) {
  const find = (url: string | undefined) => {
    const m = /^\/net\/x\/([a-z0-9_-]{1,32})\/?(?:\?.*)?$/.exec(url ?? '');
    return m ? servers().find((r) => r.id === m[1] && r.status === 'running') : undefined;
  };
  const server = createServer((req: IncomingMessage, res: ServerResponse) => {
    if (req.method === 'GET' && /^\/net\/x\/servers\.json(\?.*)?$/.test(req.url ?? '')) {
      // a public, read-only list (the page may be on another address in a test: any origin reads it)
      res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store', 'Access-Control-Allow-Origin': '*' });
      res.end(JSON.stringify(listing(servers())));
      return;
    }
    res.writeHead(find(req.url) ? 426 : 404, { 'Content-Type': 'text/plain' });
    res.end(find(req.url) ? 'a WebSocket path\n' : 'no such server\n');
  });
  server.on('upgrade', (req: IncomingMessage, socket: Socket, head: Buffer) => {
    const run = find(req.url);
    if (!run) {
      socket.end('HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n');
      return;
    }
    // the request goes on as it came (the gateway answers the handshake and checks the Origin)
    const upstream = connect(run.ports.gateway, gatewayHost, () => {
      const lines = [`${req.method} ${req.url} HTTP/${req.httpVersion}`];
      for (let i = 0; i < req.rawHeaders.length; i += 2) lines.push(`${req.rawHeaders[i]}: ${req.rawHeaders[i + 1]}`);
      upstream.write(`${lines.join('\r\n')}\r\n\r\n`);
      if (head.length) upstream.write(head);
      upstream.pipe(socket);
      socket.pipe(upstream);
    });
    const close = () => {
      upstream.destroy();
      socket.destroy();
    };
    upstream.on('error', () => {
      if (!socket.destroyed && upstream.bytesRead === 0) socket.end('HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n');
      close();
    });
    upstream.on('close', close);
    socket.on('error', close);
    socket.on('close', close);
  });
  return server;
}
