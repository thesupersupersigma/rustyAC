// SPDX-License-Identifier: GPL-3.0-or-later
//
// The site in front of the page as rustyac.com is laid out (Task 39): the page under /play/app/
// (carried to web/serve.py) and the server panel's router at /net/x/*, on one address, 127.0.0.1
// only. The router is a file (its servers.json; a WebSocket to /net/x/<id> is refused) or the
// port of a running panel's router (everything under /net/x/ carried to it, WebSockets too).
//   node tools/proofs/ondemand_site.mjs <port> <serve.py port> <servers.json | router port>
import { readFileSync } from 'node:fs';
import { createServer, request } from 'node:http';
import { connect } from 'node:net';

const [port, page, router] = process.argv.slice(2);
const live = /^\d+$/.test(router);
const carry = (req, res, to, path) => {
  const up = request({ host: '127.0.0.1', port: Number(to), method: req.method, path, headers: req.headers }, (r) => {
    res.writeHead(r.statusCode, r.headers);
    r.pipe(res);
  });
  up.on('error', (e) => res.writeHead(502).end(e.message));
  req.pipe(up);
};
const server = createServer((req, res) => {
  if (req.url.startsWith('/net/x/') && live) return carry(req, res, router, req.url);
  if (req.url.split('?')[0] === '/net/x/servers.json') {
    res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-cache' });
    return res.end(readFileSync(router));
  }
  if (!req.url.startsWith('/play/app/')) return res.writeHead(404).end();
  carry(req, res, page, req.url.slice('/play/app'.length));
});
server.on('upgrade', (req, socket, head) => {
  if (!live || !req.url.startsWith('/net/x/')) return socket.end('HTTP/1.1 404 Not Found\r\n\r\n');
  const up = connect(Number(router), '127.0.0.1', () => {
    up.write(`${req.method} ${req.url} HTTP/1.1\r\n${Object.entries(req.headers).map(([k, v]) => `${k}: ${v}`).join('\r\n')}\r\n\r\n`);
    up.write(head);
    up.pipe(socket).pipe(up);
  });
  up.on('error', () => socket.destroy());
  socket.on('error', () => up.destroy());
});
server.listen(Number(port), '127.0.0.1', () => console.log(`site on ${port}: /play/app/ -> ${page}, /net/x/ -> ${router}`));
