// SPDX-License-Identifier: MIT OR Apache-2.0
// The panel's unit tests: config files, port pools, the idle stop, the Access check, the router.
//   cd server/panel && node --test test/*.test.ts
import assert from 'node:assert/strict';
import { createSign, generateKeyPairSync } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer as httpServer } from 'node:http';
import { createServer as tcpServer, connect, type AddressInfo } from 'node:net';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { checkToken, keysFromJwks } from '../src/access.ts';
import { type Config, loadConfig, parsePool, takePorts } from '../src/config.ts';
import type { Docker } from '../src/docker.ts';
import { checkPreset, entryList, type Preset, presetFromCfg, serverCfg } from '../src/preset.ts';
import { router } from '../src/router.ts';
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
const pack = { cars: ['bmw_z4_gt3', 'ks_ferrari_f2004'], tracks: [{ track: 'ks_laguna_seca', layout: '' }, { track: 'spa', layout: '' }] };

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
  ];
  for (const [what, change] of bad) {
    const p = spa();
    change(p);
    assert.notDeepEqual(checkPreset(p, pack), [], `${what}: ${JSON.stringify(p).slice(0, 80)}`);
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
