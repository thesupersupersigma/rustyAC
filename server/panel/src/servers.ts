// SPDX-License-Identifier: MIT OR Apache-2.0
// The on-demand servers: start (configs, then Kunos' acServer and the gateway from the kit's
// images), stop, what they do now, and the stop after so many idle minutes.
import { randomBytes } from 'node:crypto';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { type Config, takePorts } from './config.ts';
import type { Docker } from './docker.ts';
import { type Preset, entryList, serverCfg } from './preset.ts';

export interface Ports { game: number; http: number; gateway: number }
export interface Running {
  id: string; preset: Preset; ports: Ports; started: number; admin: string;
  players: string[] | null; // connected drivers' names (null: the HTTP port did not answer)
  status: string; // the acServer container's state ("running", "exited", ...)
  idle: Idle;
}

// When a server has had nobody on it for its idle minutes.
export class Idle {
  last: number;
  minutes: number;
  constructor(now: number, minutes: number) {
    this.last = now;
    this.minutes = minutes;
  }
  seen(players: number, now: number) {
    if (players > 0) this.last = now;
  }
  due(now: number) {
    return now - this.last >= this.minutes * 60000;
  }
}

const LABEL = 'rustyac.panel';
export const names = (id: string) => ({ ac: `rustyac-x-${id}-acserver`, gw: `rustyac-x-${id}-gateway` });

export class Servers {
  running = new Map<string, Running>();
  config: Config;
  docker: Docker;
  log: (line: string) => void;
  constructor(config: Config, docker: Docker, log: (line: string) => void = console.log) {
    this.config = config;
    this.docker = docker;
    this.log = log;
  }

  runDir = (id: string) => join(this.config.dataDir, 'run', id);

  // The servers a panel before this one started (they run on when the panel restarts).
  async recover() {
    for (const c of await this.docker.list(`${LABEL}.ports`)) {
      const id = c.Labels[`${LABEL}.id`];
      try {
        const saved = JSON.parse(readFileSync(join(this.runDir(id), 'preset.json'), 'utf8'));
        const [game, http, gateway] = c.Labels[`${LABEL}.ports`].split(',').map(Number);
        const started = Number(c.Labels[`${LABEL}.started`]);
        this.running.set(id, { id, preset: saved.preset, admin: saved.admin, ports: { game, http, gateway }, started, players: null, status: c.State, idle: new Idle(Date.now(), saved.preset.idleMinutes) });
        this.log(`${id}: found running since ${new Date(started).toISOString()}`);
      } catch (e) {
        this.log(`${id}: a container without its run folder (${(e as Error).message}): stopped`);
        await this.remove(id);
      }
    }
  }

  async start(preset: Preset): Promise<Running> {
    if (this.running.has(preset.id)) throw new Error(`${preset.id} runs already`);
    if (this.running.size >= this.config.maxServers) throw new Error(`${this.config.maxServers} servers run already (PANEL_MAX_SERVERS)`);
    const ports = takePorts(this.config.pools, [...this.running.values()].map((r) => r.ports));
    if (!ports) throw new Error('no free ports left in the pools');
    const { id } = preset;
    const admin = randomBytes(9).toString('base64url');
    const run: Running = { id, preset, ports, started: Date.now(), admin, players: null, status: 'starting', idle: new Idle(Date.now(), preset.idleMinutes) };
    this.running.set(id, run);
    try {
      const dir = this.runDir(id);
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, 'server_cfg.ini'), serverCfg(preset, ports));
      writeFileSync(join(dir, 'entry_list.ini'), entryList(preset));
      writeFileSync(join(dir, 'preset.json'), JSON.stringify({ preset, admin }), { mode: 0o600 });
      const n = names(id);
      // (the kit's images carry its compose file's project and service labels: without these a
      // panel server shows up in Docker tools as the kit's gateway-spa)
      const compose = (service: string) => ({ 'com.docker.compose.project': 'rustyac-panel', 'com.docker.compose.service': service });
      const labels = { [LABEL]: '1', [`${LABEL}.id`]: id };
      const g = `${ports.game}`;
      await this.docker.remove(n.gw);
      await this.docker.remove(n.ac);
      const ac = await this.docker.create(n.ac, {
        Image: this.config.acserverImage,
        Env: [`AC_SERVER=x-${id}`, 'AC_CFG=/cfg', `AC_ADMIN_PASSWORD=${admin}`, `AC_PASSWORD=${preset.password}`],
        Labels: { ...compose(n.ac), ...labels, [`${LABEL}.ports`]: `${ports.game},${ports.http},${ports.gateway}`, [`${LABEL}.started`]: String(run.started) },
        ExposedPorts: { [`${g}/tcp`]: {}, [`${g}/udp`]: {}, [`${ports.http}/tcp`]: {}, '9601/tcp': {} },
        HostConfig: {
          Binds: [`${this.config.packHostDir}:/pack:ro`, `${join(this.config.dataHostDir, 'run', id).replace(/\\/g, '/')}:/cfg:ro`],
          // the game ports for desktop players (LAN / tailnet), the gateway only on GATEWAY_BIND
          PortBindings: {
            [`${g}/tcp`]: [{ HostPort: g }], [`${g}/udp`]: [{ HostPort: g }], [`${ports.http}/tcp`]: [{ HostPort: `${ports.http}` }],
            '9601/tcp': [{ HostIp: this.config.bind, HostPort: `${ports.gateway}` }],
          },
          RestartPolicy: { Name: 'no' },
        },
      });
      await this.docker.start(ac);
      // the gateway lives in the acServer container's network: the server is 127.0.0.1 to it
      const gw = await this.docker.create(n.gw, {
        Image: this.config.gatewayImage,
        Env: [`GATEWAY_NAME=x-${id}`, 'GATEWAY_LISTEN=0.0.0.0:9601', `GATEWAY_SERVER=127.0.0.1:${g}`, `GATEWAY_ORIGINS=${this.config.gatewayOrigins}`, `GATEWAY_MAX_CLIENTS=${preset.maxPlayers}`],
        Labels: { ...compose(n.gw), ...labels },
        HostConfig: { NetworkMode: `container:${n.ac}`, Init: true, RestartPolicy: { Name: 'no' } },
      });
      await this.docker.start(gw);
      run.status = 'running';
      this.log(`${id}: started, game ${ports.game}, HTTP ${ports.http}, gateway ${ports.gateway}`);
      return run;
    } catch (e) {
      this.log(`${id}: start failed: ${(e as Error).message}`);
      await this.remove(id).catch(() => {});
      throw e;
    }
  }

  async remove(id: string) {
    const n = names(id);
    await this.docker.stop(n.gw).catch(() => {});
    await this.docker.remove(n.gw);
    await this.docker.stop(n.ac).catch(() => {});
    await this.docker.remove(n.ac);
    this.running.delete(id);
    rmSync(this.runDir(id), { recursive: true, force: true });
  }

  async stop(id: string, why: string) {
    if (!this.running.has(id)) throw new Error(`${id} is not running`);
    await this.remove(id);
    this.log(`${id}: stopped (${why})`);
  }

  // Who is on each server (acServer's HTTP port), the containers' states, and the idle stops.
  async poll(now = Date.now()) {
    const states = new Map((await this.docker.list(`${LABEL}.ports`)).map((c) => [c.Labels[`${LABEL}.id`], c.State]));
    for (const run of [...this.running.values()]) {
      if (run.status === 'starting') continue;
      run.status = states.get(run.id) ?? 'gone';
      run.players = await players(this.config.hostAddr, run.ports.http);
      run.idle.seen(run.players?.length ?? 0, now);
      if (run.idle.due(now)) await this.stop(run.id, `nobody on it for ${run.idle.minutes} minutes`).catch((e) => this.log(`${run.id}: ${e.message}`));
    }
  }

  async logs(id: string) {
    const n = names(id);
    return { acserver: await this.docker.logs(n.ac), gateway: await this.docker.logs(n.gw) };
  }
}

// The connected drivers' names from acServer's HTTP port (`/JSON|`, what AC's own lobby asks).
export async function players(host: string, port: number): Promise<string[] | null> {
  try {
    const r = await fetch(`http://${host.includes(':') ? `[${host}]` : host}:${port}/JSON%7C`, { signal: AbortSignal.timeout(3000) });
    const cars = ((await r.json()) as { Cars?: { DriverName: string; IsConnected: boolean }[] }).Cars ?? [];
    return cars.filter((c) => c.IsConnected).map((c) => c.DriverName);
  } catch {
    return null;
  }
}
