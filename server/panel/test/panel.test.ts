// SPDX-License-Identifier: MIT OR Apache-2.0
// The panel's unit tests: config files, port pools, the idle stop, the Access check, the router.
//   cd server/panel && node --test test/*.test.ts
import assert from 'node:assert/strict';
import { createSign, generateKeyPairSync } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer as httpServer } from 'node:http';
import { createServer as tcpServer, connect, type AddressInfo } from 'node:net';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { checkToken, keysFromJwks } from '../src/access.ts';
import { type Config, loadConfig, parsePool, takePorts } from '../src/config.ts';
import type { Docker } from '../src/docker.ts';
import { checkPreset, entryList, type Pack, type Preset, presetFromCfg, readPack, serverCfg } from '../src/preset.ts';
import { api, idFromName } from '../public/form.js';
import { listing, router } from '../src/router.ts';
import { Idle, type Running, Servers } from '../src/servers.ts';

const kit = new URL('../../', import.meta.url); // server/
const fixed = (name: string, file: string) => readFileSync(new URL(`${name}/${file}`, kit), 'utf8');
const noComments = (text: string) => text.split('\n').filter((l) => !l.startsWith(';')).join('\n');

// The Spa server's choices, typed out the way the page sends them.
const spa = (): Preset => ({
  id: 'spa-x', name: 'rustyAC Spa', track: 'spa', layout: '',
  cars: [
    { model: 'ks_ferrari_f2004', count: 6, skins: ['00_official', '01_ferrari_1', '02_ferrari_2'] },
    { model: 'bmw_z4_gt3', count: 6, skins: ['0_bmw_motorsport_2013_black', 'bmw_motorsport_2013_white', 'marc_vds_2013', 'scuderia_ecosse', 'walkenhorst_motorsport', 'Vita4one'] },
  ],
  practice: { minutes: 20, open: 1 }, qualify: { minutes: 10, open: 1 }, race: { laps: 5, minutes: 0, waitSeconds: 60, open: 1 },
  time: '14:00',
  weather: { graphics: '3_clear', ambient: 22, road: 8, variationAmbient: 1, variationRoad: 1, windMin: 0, windMax: 8, windDirection: 30, windVariation: 15 },
  dynamicTrack: { start: 96, randomness: 1, transfer: 80, lapGain: 30 },
  password: '', maxPlayers: 12, pickup: true, idleMinutes: 30, rules: {},
});
const pack: Pack = {
  cars: [
    { model: 'bmw_z4_gt3', name: 'BMW Z4 GT3', skins: ['0_bmw_motorsport_2013_black', 'Vita4one', 'bmw_motorsport_2013_white', 'marc_vds_2013', 'scuderia_ecosse', 'walkenhorst_motorsport', 'fanatec'] },
    { model: 'ks_ferrari_f2004', name: 'Ferrari F2004', skins: ['00_official', '01_ferrari_1', '02_ferrari_2'] },
  ],
  tracks: [{ track: 'ks_laguna_seca', layout: '', name: 'Laguna Seca', pitboxes: 24 }, { track: 'spa', layout: '', name: 'Spa', pitboxes: 24 }],
  weather: [{ id: '3_clear', name: 'Clear' }],
};

test('a preset with the Spa server choices writes its server_cfg.ini and entry_list.ini', () => {
  assert.deepEqual(checkPreset(spa(), pack), []);
  assert.equal(serverCfg(spa(), { game: 9600, http: 8081 }), noComments(fixed('spa', 'server_cfg.ini')));
  assert.equal(entryList(spa()), noComments(fixed('spa', 'entry_list.ini')));
});

test('a copy of each fixed server writes that server\'s files again', () => {
  for (const [name, game, http] of [['spa', 9600, 8081], ['laguna_seca', 9610, 8082]] as const) {
    const p = presetFromCfg(`${name}-x`, fixed(name, 'server_cfg.ini'), fixed(name, 'entry_list.ini'));
    assert.deepEqual(checkPreset(p, pack), [], name);
    assert.equal(serverCfg(p, { game, http }), noComments(fixed(name, 'server_cfg.ini')), name);
    assert.equal(entryList(p), noComments(fixed(name, 'entry_list.ini')), name);
  }
  assert.deepEqual({ ...presetFromCfg('spa-x', fixed('spa', 'server_cfg.ini'), fixed('spa', 'entry_list.ini')), name: 'rustyAC Spa' }, spa());
});

test('presets with anything the pack lacks or the ini cannot hold are refused', () => {
  const bad: [string, (p: Preset) => void][] = [
    ['id', (p) => { p.id = '../etc'; }],
    ['id', (p) => { p.id = 'Spa X'; }],
    ['name', (p) => { p.name = 'a\nADMIN_PASSWORD=x'; }],
    ['track', (p) => { p.track = '../../etc'; }],
    ['track', (p) => { p.track = 'monza'; }],
    ['car', (p) => { p.cars[0].model = 'ks_mazda_mx5_cup'; }],
    ['skin', (p) => { p.cars[0].skins = ['a/b']; }],
    ['password', (p) => { p.password = 'x\ny'; }],
    ['rules', (p) => { p.rules = { HTTP_PORT: '80' }; }],
    ['rules', (p) => { p.rules = { REGISTER_TO_LOBBY: '1' }; }],
    ['rules', (p) => { p.rules = { FUEL_RATE: '1\n[SERVER]' }; }],
    ['maxPlayers', (p) => { p.maxPlayers = 0; }],
    ['cars', (p) => { p.cars = []; }],
    ['time', (p) => { p.time = '23:00'; }],
    ['idle', (p) => { p.idleMinutes = 0; }],
    ['skin', (p) => { p.cars[0].skins = ['not_its_skin']; }],
    ['random', (p) => { p.cars[0].skins = []; p.cars[0].random = true; }],
    ['pit boxes', (p) => { p.cars[0].count = 19; }],
    ['weather', (p) => { p.weather.graphics = '9_storm'; }],
  ];
  for (const [what, change] of bad) {
    const p = spa();
    change(p);
    assert.notDeepEqual(checkPreset(p, pack), [], `${what}: ${JSON.stringify(p).slice(0, 80)}`);
  }
});

test('skins: in turn, all of them, random, or none (acServer\'s default)', () => {
  const skins = (p: Preset, random?: () => number) => [...entryList(p, random).matchAll(/^SKIN=(.*)$/gm)].map((m) => m[1]);
  const p = spa();
  p.cars = [{ model: 'ks_ferrari_f2004', count: 4, skins: ['00_official', '01_ferrari_1'] }];
  assert.deepEqual(skins(p), ['00_official', '01_ferrari_1', '00_official', '01_ferrari_1']);
  p.cars[0].skins = [...pack.cars[1].skins]; // "all skins" ticks every one
  assert.deepEqual(skins(p), ['00_official', '01_ferrari_1', '02_ferrari_2', '00_official']);
  p.cars[0].random = true;
  const draws = [0.9, 0.1, 0.5, 0.99];
  assert.deepEqual(skins(p, () => draws.shift()!), ['02_ferrari_2', '00_official', '01_ferrari_1', '02_ferrari_2']);
  assert.deepEqual(checkPreset(p, pack), []);
  p.cars[0] = { model: 'ks_ferrari_f2004', count: 2, skins: [] };
  assert.deepEqual(skins(p), ['', '']);
  assert.deepEqual(checkPreset(p, pack), []);
});

test('preset ids from names', () => {
  assert.equal(idFromName('Spa: GT3 night!'), 'spa-gt3-night');
  assert.equal(idFromName('  Ünïcode ✓ '), 'n-code');
  assert.equal(idFromName('!!!'), 'server');
  assert.equal(idFromName('Spa GT3', ['spa-gt3', 'spa-gt3-2']), 'spa-gt3-3');
  const taken = Array.from({ length: 20 }, (_, i) => (i ? `a-very-long-name-for-a-server-${i + 1}` : 'a-very-long-name-for-a-server'));
  assert.match(idFromName('a very long name for a server that goes on and on', taken), /^[a-z0-9][a-z0-9_-]{0,31}$/);
});

test('the page\'s fetch: an Access redirect or login page reloads once, then says so; errors keep their message', async () => {
  const page = (answers: (Partial<Response> | Error)[]) => {
    const store = new Map<string, string>();
    const env = {
      reloads: 0, asked: [] as RequestInit[],
      fetch: async (_: string, init: RequestInit) => {
        env.asked.push(init);
        const a = answers.shift()!;
        if (a instanceof Error) throw a;
        return a;
      },
      sessionStorage: { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => store.set(k, v), removeItem: (k: string) => store.delete(k) },
      location: { reload: () => { env.reloads++; } },
    };
    return env;
  };
  const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
  const redirect = { type: 'opaqueredirect', status: 0, ok: false, redirected: false, headers: new Headers() } as Partial<Response>;
  const login = new Response('<html>Cloudflare Access</html>', { status: 200, headers: { 'Content-Type': 'text/html' } });
  const env = page([redirect, login, json(200, { ok: 1 }), redirect]);
  await assert.rejects(api('/api/state', undefined, env), /login expired: reloading/);
  assert.equal(env.reloads, 1);
  assert.deepEqual([env.asked[0].credentials, env.asked[0].redirect], ['same-origin', 'manual']);
  await assert.rejects(api('/api/state', undefined, env), /did not log in again/); // after the reload: no second one
  assert.equal(env.reloads, 1);
  assert.deepEqual(await api('/api/state', undefined, env), { ok: 1 }); // logged in again
  await assert.rejects(api('/api/state', undefined, env), /reloading/); // a later expiry reloads again
  assert.equal(env.reloads, 2);
  const env2 = page([json(400, { error: 'cars: at least one' }), new TypeError('Failed to fetch')]);
  await assert.rejects(api('/api/presets', { id: 'x' }, env2), (e: Error) => e.message === 'cars: at least one');
  assert.equal(env2.asked[0].method, 'POST');
  await assert.rejects(api('/api/state', undefined, env2), (e: Error) => !e.message.includes('Failed to fetch') && /did not answer/.test(e.message));
});

test('make_server_pack.py --all: the index of a made-up install, and the panel reads it', () => {
  const tmp = mkdtempSync(join(fileURLToPath(new URL('.', import.meta.url)), 'tmp-'));
  try {
    const ac = join(tmp, 'ac');
    const put = (rel: string, text = 'x') => {
      mkdirSync(join(ac, rel, '..'), { recursive: true });
      writeFileSync(join(ac, rel), text);
    };
    put('server/acServer');
    put('server/system/data/surfaces.ini');
    for (const car of ['ks_ferrari_f2004', 'bmw_z4_gt3']) put(`content/cars/${car}/data.acd`);
    for (const track of ['spa', 'ks_laguna_seca']) put(`content/tracks/${track}/data/surfaces.ini`);
    put('content/cars/test_car/data.acd');
    put('content/cars/test_car/ui/ui_car.json', '{\n\t"name": "Test Car GT",\n\t"brand": "x",\n}'); // not valid JSON, as many are
    for (const skin of ['red', 'blue']) mkdirSync(join(ac, 'content/cars/test_car/skins', skin), { recursive: true });
    put('content/cars/dlc_car/ui/ui_car.json', '{"name": "Not Owned"}');
    put('content/tracks/test_track/gp/data/surfaces.ini');
    put('content/tracks/test_track/gp/data/drs_zones.ini');
    put('content/tracks/test_track/models_gp.ini');
    put('content/tracks/test_track/ui/gp/ui_track.json', '{"name": "Test Track - GP", "pitboxes": "8"}');
    put('content/tracks/test_track/short/data/surfaces.ini');
    put('content/tracks/test_track/ui/short/ui_track.json', '{"name": "Test Track - Short", "pitboxes": 6}');
    put('content/weather/3_clear/weather.ini');
    put('content/weather/1_heavy_fog/weather.ini');
    const out = join(tmp, 'pack');
    const python = process.env.PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3');
    const script = fileURLToPath(new URL('make_server_pack.py', kit));
    execFileSync(python, [script, '--ac', ac, '--out', out, '--all', '--no-check'], { stdio: 'pipe' });
    const p = readPack(out);
    assert.deepEqual(p.cars.find((c) => c.model === 'test_car'), { model: 'test_car', name: 'Test Car GT', skins: ['blue', 'red'] });
    assert.equal(p.cars.length, 3);
    assert.deepEqual(p.tracks.filter((t) => t.track === 'test_track'), [
      { track: 'test_track', layout: 'gp', name: 'Test Track - GP', pitboxes: 8 },
      { track: 'test_track', layout: 'short', name: 'Test Track - Short', pitboxes: 6 },
    ]);
    assert.deepEqual(p.weather, [{ id: '1_heavy_fog', name: 'Heavy fog' }, { id: '3_clear', name: 'Clear' }]);
    const index = JSON.parse(readFileSync(join(out, 'index.json'), 'utf8'));
    assert.deepEqual(index.skipped.map((s: { id: string }) => s.id), ['dlc_car']);
    for (const f of ['content/tracks/test_track/gp/data/drs_zones.ini', 'content/tracks/test_track/models_gp.ini', 'content/weather/3_clear/weather.ini', 'cfg/spa/server_cfg.ini']) {
      assert.ok(readFileSync(join(out, f)), f);
    }
    // --car adds one to the pack that is there
    put('content/cars/late_car/data.acd');
    execFileSync(python, [script, '--ac', ac, '--out', out, '--car', 'late_car', '--no-check'], { stdio: 'pipe' });
    assert.deepEqual(readPack(out).cars.map((c) => c.model).sort(), ['bmw_z4_gt3', 'ks_ferrari_f2004', 'late_car', 'test_car']);
    assert.equal(readPack(out).tracks.length, 4);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

const env = { CF_ACCESS_TEAM_DOMAIN: 'example-team.cloudflareaccess.com', CF_ACCESS_AUD: 'made-up-aud', PANEL_DATA_DIR: '/srv/panel', AC_PACK_DIR: '/srv/pack' };

test('port pools: ranges, the first free ports, and never the fixed servers\' or Nextcloud\'s', () => {
  assert.deepEqual(parsePool('9620-9622, 9630'), [9620, 9621, 9622, 9630]);
  assert.throws(() => parsePool('80'));
  assert.throws(() => parsePool('9620-x'));
  const pools = { game: [1, 2], http: [11, 12], gateway: [21, 22] };
  assert.deepEqual(takePorts(pools, []), { game: 1, http: 11, gateway: 21 });
  assert.deepEqual(takePorts(pools, [{ game: 1, http: 11, gateway: 21 }]), { game: 2, http: 12, gateway: 22 });
  assert.deepEqual(takePorts(pools, [{ game: 2, http: 11, gateway: 22 }]), { game: 1, http: 12, gateway: 21 });
  assert.equal(takePorts(pools, [{ game: 1, http: 11, gateway: 21 }, { game: 2, http: 12, gateway: 22 }]), null);
  assert.equal(loadConfig(env).pools.game.length, 10);
  assert.throws(() => loadConfig({ ...env, PANEL_HTTP_PORTS: '8080-8085' }), /8081, 8082/);
  assert.throws(() => loadConfig({ ...env, PANEL_GAME_PORTS: '9600' }), /9600/);
  assert.throws(() => loadConfig({ ...env, LAGUNA_GAME_PORT: '9625' }), /9625/);
  assert.throws(() => loadConfig({ ...env, PANEL_GATEWAY_PORTS: '9620' }), /twice/);
  assert.throws(() => loadConfig({ ...env, CF_ACCESS_AUD: '' }), /CF_ACCESS_AUD/);
});

test('the idle timer counts from the last time anybody was on', () => {
  const idle = new Idle(0, 30);
  assert.equal(idle.due(29 * 60000), false);
  idle.seen(1, 20 * 60000);
  assert.equal(idle.due(49 * 60000), false);
  idle.seen(0, 49 * 60000);
  assert.equal(idle.due(50 * 60000), true);
});

test('the poll stops a server nobody was on for its idle minutes, and keeps one with players', async () => {
  // acServer's HTTP port of "busy" (one driver connected, one slot free); "empty" has none that answers
  const http = httpServer((req, res) => {
    assert.equal(req.url, '/JSON%7C');
    res.end(JSON.stringify({ Cars: [{ DriverName: 'a', IsConnected: true }, { DriverName: '', IsConnected: false }] }));
  }).listen(0, '127.0.0.1');
  await new Promise((r) => http.once('listening', r));
  const calls: string[] = [];
  const docker = {
    list: async () => ['busy', 'empty'].map((id) => ({ Labels: { 'rustyac.panel.id': id }, State: 'running' })),
    stop: async (n: string) => calls.push(`stop ${n}`),
    remove: async (n: string) => calls.push(`remove ${n}`),
  } as unknown as Docker;
  const data = mkdtempSync(join(fileURLToPath(new URL('.', import.meta.url)), 'tmp-'));
  try {
    const servers = new Servers({ ...loadConfig(env), hostAddr: '127.0.0.1', dataDir: data } as Config, docker, () => {});
    const run = (id: string, port: number): Running => ({ id, preset: { ...spa(), id }, ports: { game: 1, http: port, gateway: 2 }, started: 0, admin: '', players: null, status: 'running', idle: new Idle(0, 30) });
    servers.running.set('busy', run('busy', (http.address() as AddressInfo).port));
    servers.running.set('empty', run('empty', 1));
    await servers.poll(29 * 60000);
    assert.deepEqual(servers.running.get('busy')?.players, ['a']);
    assert.equal(servers.running.get('empty')?.players, null);
    assert.deepEqual(calls, []);
    await servers.poll(31 * 60000);
    assert.deepEqual([...servers.running.keys()], ['busy']);
    assert.deepEqual(calls, ['stop rustyac-x-empty-gateway', 'remove rustyac-x-empty-gateway', 'stop rustyac-x-empty-acserver', 'remove rustyac-x-empty-acserver']);
  } finally {
    http.close();
    rmSync(data, { recursive: true, force: true });
  }
});

test('Access tokens: only a valid one for this application passes', () => {
  const team = generateKeyPairSync('rsa', { modulusLength: 2048 });
  const other = generateKeyPairSync('rsa', { modulusLength: 2048 });
  const keys = keysFromJwks({ keys: [{ ...team.publicKey.export({ format: 'jwk' }), kid: 'k1' }] });
  const now = 1_800_000_000;
  const issuer = 'https://example-team.cloudflareaccess.com';
  const token = (body: object, key = team.privateKey, kid = 'k1', alg = 'RS256') => {
    const part = (o: object) => Buffer.from(JSON.stringify(o)).toString('base64url');
    const signed = `${part({ alg, kid })}.${part(body)}`;
    return `${signed}.${createSign('RSA-SHA256').update(signed).sign(key).toString('base64url')}`;
  };
  const good = { aud: ['made-up-aud'], iss: issuer, email: 'me@example.com', exp: now + 600, iat: now - 10, nbf: now - 10 };
  const check = (t: string | undefined) => checkToken(t, keys, 'made-up-aud', issuer, now);
  assert.deepEqual(check(token(good)), { email: 'me@example.com' });
  assert.match((check(token({ ...good, exp: now - 1 })) as { refused: string }).refused, /expired/);
  assert.match((check(token({ ...good, aud: ['another-aud'] })) as { refused: string }).refused, /AUD/);
  assert.match((check(undefined) as { refused: string }).refused, /no Cf-Access/);
  assert.match((check(token(good, other.privateKey)) as { refused: string }).refused, /signature/);
  assert.match((check(token(good, other.privateKey, 'k2')) as { refused: string }).refused, /not the team/);
  assert.match((check(token({ ...good, iss: 'https://evil.cloudflareaccess.com' })) as { refused: string }).refused, /issuer/);
  assert.match((check(token(good, team.privateKey, 'k1', 'none')) as { refused: string }).refused, /algorithm/);
  assert.match((check('a.b') as { refused: string }).refused, /not a JWT/);
  const t = token(good);
  assert.match((check(`${t.slice(0, t.indexOf('.') + 5)}x${t.slice(t.indexOf('.') + 6)}`) as { refused: string }).refused, /signature|JWT/);
});

test('a running server with a password is listed with one', () => {
  const r = (id: string, password: string): Running => ({ id, preset: { ...spa(), password }, ports: { game: 1, http: 2, gateway: 3 }, started: 0, admin: 'secret', players: [], status: 'running', idle: new Idle(0, 30) });
  const list = listing([r('open', ''), r('locked', 'made-up-pw')]);
  assert.deepEqual(list.servers.map((s) => [s.id, s.password]), [['x-open', false], ['x-locked', true]]);
  assert.ok(!JSON.stringify(list).includes('made-up-pw'));
});

test('the router lists running servers and carries only their WebSockets', async () => {
  // a stand-in gateway: answers the upgrade, then echoes
  const seen: string[] = [];
  const gw = tcpServer((s) => {
    s.once('data', (d) => {
      seen.push(d.toString().split('\r\n')[0]);
      s.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n');
      s.pipe(s);
    });
  }).listen(0, '127.0.0.1');
  await new Promise((r) => gw.once('listening', r));
  const running: Running[] = [
    { id: 'one', preset: spa(), ports: { game: 1, http: 2, gateway: (gw.address() as AddressInfo).port }, started: 0, admin: 'secret', players: ['a'], status: 'running', idle: new Idle(0, 30) },
    { id: 'two', preset: { ...spa(), password: 'pw' }, ports: { game: 1, http: 2, gateway: 1 }, started: 0, admin: 'secret', players: null, status: 'starting', idle: new Idle(0, 30) },
  ];
  const r = router(() => running, '127.0.0.1').listen(0, '127.0.0.1');
  await new Promise((done) => r.once('listening', done));
  const port = (r.address() as AddressInfo).port;
  try {
    const list = await (await fetch(`http://127.0.0.1:${port}/net/x/servers.json`)).json();
    assert.deepEqual(list, { servers: [{ id: 'x-one', name: 'rustyAC Spa', url: '/net/x/one', cars: ['ks_ferrari_f2004', 'bmw_z4_gt3'], track: 'spa', layout: '', players: 1, maxPlayers: 12, password: false }] });
    assert.ok(!JSON.stringify(list).includes('secret'));
    const upgrade = (path: string) => new Promise<string>((done) => {
      const s = connect(port, '127.0.0.1', () => s.write(`GET ${path} HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n`));
      let got = '';
      s.on('data', (d) => {
        got += d;
        if (got.includes('\r\n\r\n')) {
          if (got.startsWith('HTTP/1.1 101')) s.write('ping');
          if (!got.startsWith('HTTP/1.1 101') || got.endsWith('ping')) s.destroy();
        }
      });
      s.on('close', () => done(got));
    });
    for (const path of ['/net/x/three', '/net/x/two', '/net/spa', '/net/x/../spa', '/net/x/one/../two']) assert.match(await upgrade(path), /^HTTP\/1\.1 404/, path);
    assert.match(await upgrade('/net/x/one'), /^HTTP\/1\.1 101[^]*ping$/);
    assert.deepEqual(seen, ['GET /net/x/one HTTP/1.1']);
    assert.equal((await fetch(`http://127.0.0.1:${port}/net/x/two`)).status, 404);
  } finally {
    r.close();
    gw.close();
  }
});
