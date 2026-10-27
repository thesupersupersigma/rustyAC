// What Cloudflare Access does in front of the panel, on 127.0.0.1 only, with a test key pair
// made for this run: the team's public keys at /cdn-cgi/access/certs (port <certs>), and a
// proxy (port <edge>) that adds a signed Cf-Access-Jwt-Assertion to every request it carries
// to the panel (port <panel>). Made-up team domain, AUD and e-mail.
//   node tools/web_checks/task38e/edge.mjs <certs> <edge> <panel>
import { createSign, generateKeyPairSync } from 'node:crypto';
import { createServer, request } from 'node:http';

const [certsPort, edgePort, panelPort] = process.argv.slice(2).map(Number);
const { publicKey, privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const kid = 'test-key-38e';
const part = (o) => Buffer.from(JSON.stringify(o)).toString('base64url');
const token = () => {
  const now = Math.floor(Date.now() / 1000);
  const body = { aud: ['made-up-aud-38e'], email: 'me@example.com', iss: 'https://example-team.cloudflareaccess.com', iat: now, nbf: now, exp: now + 3600, type: 'app' };
  const signed = `${part({ alg: 'RS256', kid, typ: 'JWT' })}.${part(body)}`;
  return `${signed}.${createSign('RSA-SHA256').update(signed).sign(privateKey).toString('base64url')}`;
};

createServer((req, res) => {
  if (req.url !== '/cdn-cgi/access/certs') return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ keys: [{ ...publicKey.export({ format: 'jwk' }), kid, alg: 'RS256', use: 'sig' }] }));
}).listen(certsPort, '127.0.0.1');

createServer((req, res) => {
  const headers = { ...req.headers, 'cf-access-jwt-assertion': token() };
  const up = request({ host: '127.0.0.1', port: panelPort, method: req.method, path: req.url, headers }, (r) => {
    res.writeHead(r.statusCode, r.headers);
    r.pipe(res);
  });
  up.on('error', (e) => res.writeHead(502).end(e.message));
  req.pipe(up);
}).listen(edgePort, '127.0.0.1', () => console.log(`certs on ${certsPort}, edge ${edgePort} -> panel ${panelPort}`));
