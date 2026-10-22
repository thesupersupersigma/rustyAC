// SPDX-License-Identifier: MIT OR Apache-2.0
// rustyAC's server panel: a private page (behind Cloudflare Access) that starts and stops extra
// acServers from presets, and the public router of their WebSockets. See server/README.md.
//   node src/main.ts            (settings from the environment: server/.env.example)
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { join } from 'node:path';
import { checkToken, TeamKeys, tokenKid } from './access.ts';
import { type Config, loadConfig } from './config.ts';
import { Docker } from './docker.ts';
import { checkPreset, NAME, type Preset, presetFromCfg, readPack } from './preset.ts';
import { router } from './router.ts';
import { Servers } from './servers.ts';

const config: Config = loadConfig(process.env);
// a test run on one computer: every address the panel talks to must be this computer's
if (process.env.PANEL_LOCAL_ONLY === '1') {
  const loopback = (host: string) => ['127.0.0.1', '::1', '[::1]', 'localhost'].includes(host);
  for (const [what, host] of [['GATEWAY_BIND', config.bind], ['PANEL_LISTEN', config.listen], ['PANEL_HOST_ADDR', config.hostAddr], ['PANEL_DOCKER_URL', new URL(config.dockerUrl).hostname], ['CF_ACCESS_CERTS_URL', new URL(config.access.certsUrl).hostname]]) {
    if (!loopback(host)) throw new Error(`PANEL_LOCAL_ONLY: ${what} is ${host}, not this computer`);
  }
}
const docker = new Docker(config.dockerUrl);
const servers = new Servers(config, docker);
const teamKeys = new TeamKeys(config.access.certsUrl);
const presetsDir = join(config.dataDir, 'presets');
mkdirSync(presetsDir, { recursive: true });
const presetFile = (id: string) => join(presetsDir, `${id}.json`);
// the page and its own files; nothing else is served, and the page may load nothing from elsewhere
const files = new Map<string, { body: Buffer; type: string }>(([['/', 'index.html', 'text/html'], ['/panel.js', 'panel.js', 'text/javascript'], ['/form.js', 'form.js', 'text/javascript'], ['/favicon.svg', 'favicon.svg', 'image/svg+xml']] as const)
  .map(([path, file, type]) => [path, { body: readFileSync(new URL(`../public/${file}`, import.meta.url)), type: `${type}; charset=utf-8` }]));

const presets = (): Preset[] => readdirSync(presetsDir).filter((f) => f.endsWith('.json')).sort().map((f) => JSON.parse(readFileSync(join(presetsDir, f), 'utf8')));
const fixedServers = () => {
  const dir = join(config.packDir, 'cfg');
  return existsSync(dir) ? readdirSync(dir).filter((d) => NAME.test(d) && existsSync(join(dir, d, 'server_cfg.ini'))) : [];
};

function state() {
  const now = Date.now();
  return {
    pack: readPack(config.packDir), presets: presets(), fixed: fixedServers(), maxServers: config.maxServers,
    running: [...servers.running.values()].map((r) => ({
      id: r.id, name: r.preset.name, track: r.preset.track, layout: r.preset.layout, cars: r.preset.cars.map((c) => c.model),
      ports: r.ports, started: r.started, status: r.status, players: r.players, maxPlayers: r.preset.maxPlayers, admin: r.admin,
      idleMinutes: r.idle.minutes, stopsInSeconds: Math.max(0, Math.round((r.idle.last + r.idle.minutes * 60000 - now) / 1000)),
      browser: `${config.siteUrl}?join=x-${r.id}`,
      desktop: `rustyac.exe --join ${config.gameHost}:${r.ports.game} --car ${r.preset.cars[0].model}${r.preset.password ? ' (and the password)' : ''}`,
    })),
  };
}

const send = (res: ServerResponse, status: number, body: unknown) => {
  res.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
  res.end(JSON.stringify(body));
};
const readBody = (req: IncomingMessage): Promise<unknown> => new Promise((resolve, reject) => {
  let text = '';
  req.setEncoding('utf8');
  req.on('data', (c: string) => {
    text += c;
    if (text.length > 100000) req.destroy(new Error('too large'));
  });
  req.on('end', () => {
    try {
      resolve(text ? JSON.parse(text) : null);
    } catch {
      reject(new Error('the body is no JSON'));
    }
  });
  req.on('error', reject);
});

async function handle(req: IncomingMessage, res: ServerResponse) {
  // Cloudflare Access first: no valid token, nothing (not even the page)
  const token = req.headers['cf-access-jwt-assertion'] as string | undefined;
  const verdict = checkToken(token, await teamKeys.get(tokenKid(token)), config.access.aud, `https://${config.access.teamDomain}`);
  if ('refused' in verdict) {
    console.log(`refused ${req.method} ${req.url}: ${verdict.refused}`);
    return send(res, 403, { error: 'refused: open the panel through Cloudflare Access' });
  }
  const url = new URL(req.url ?? '/', 'http://panel');
  const file = req.method === 'GET' ? files.get(url.pathname) : undefined;
  if (file) {
    res.writeHead(200, { 'Content-Type': file.type, 'Cache-Control': 'no-store', 'Content-Security-Policy': "default-src 'self'; style-src 'unsafe-inline'" });
    return res.end(file.body);
  }
  if (req.method === 'GET' && url.pathname === '/api/state') return send(res, 200, state());
  let m = /^\/api\/logs\/([^/]+)$/.exec(url.pathname);
  if (req.method === 'GET' && m && NAME.test(m[1])) return send(res, 200, await servers.logs(m[1]));
  if (req.method !== 'POST') return send(res, 404, { error: 'no such page' });
  // changes only from the panel's own page: a JSON body needs a CORS preflight from another
  // site, which is never answered (no Origin check: behind the tunnel the Host is the origin's)
  if (!String(req.headers['content-type']).startsWith('application/json')) {
    return send(res, 403, { error: 'changes come from the panel page only' });
  }
  const body = await readBody(req);
  console.log(`${verdict.email}: ${url.pathname}`);
  if (url.pathname === '/api/presets') {
    const preset = body as Preset;
    const bad = checkPreset(preset, readPack(config.packDir));
    if (bad.length) return send(res, 400, { error: bad.join('; ') });
    writeFileSync(presetFile(preset.id), JSON.stringify(preset, null, 1), { mode: 0o600 });
    return send(res, 200, { ok: true });
  }
  m = /^\/api\/(delete|start|stop|copy)\/([^/]+)$/.exec(url.pathname);
  if (!m || !NAME.test(m[2])) return send(res, 404, { error: 'no such action' });
  const [, action, id] = m;
  if (action === 'copy') {
    if (!fixedServers().includes(id)) return send(res, 404, { error: `no fixed server ${id}` });
    const dir = join(config.packDir, 'cfg', id);
    const copy = presetFromCfg(`${id}-x`.slice(0, 32), readFileSync(join(dir, 'server_cfg.ini'), 'utf8'), readFileSync(join(dir, 'entry_list.ini'), 'utf8'));
    copy.name = `${copy.name} (on demand)`.slice(0, 120);
    writeFileSync(presetFile(copy.id), JSON.stringify(copy, null, 1), { mode: 0o600 });
    return send(res, 200, { ok: true, id: copy.id });
  }
  if (action === 'stop') return servers.stop(id, `stopped by ${verdict.email}`).then(() => send(res, 200, { ok: true }));
  if (!existsSync(presetFile(id))) return send(res, 404, { error: `no preset ${id}` });
  if (action === 'delete') {
    rmSync(presetFile(id));
    return send(res, 200, { ok: true });
  }
  const preset: Preset = JSON.parse(readFileSync(presetFile(id), 'utf8'));
  const bad = checkPreset(preset, readPack(config.packDir));
  if (bad.length) return send(res, 400, { error: bad.join('; ') });
  await servers.start(preset);
  return send(res, 200, { ok: true });
}

const panel = createServer((req, res) => {
  handle(req, res).catch((e) => send(res, 500, { error: (e as Error).message }));
});

await servers.recover();
const tick = () => servers.poll().catch((e) => console.log(`poll: ${(e as Error).message}`));
setInterval(tick, config.pollSeconds * 1000);
await tick();
panel.listen(config.panelPort, config.listen, () => console.log(`panel on ${config.listen}:${config.panelPort}`));
router(() => [...servers.running.values()], config.hostAddr).listen(config.routerPort, config.listen, () => console.log(`router on ${config.listen}:${config.routerPort}`));
