// SPDX-License-Identifier: MIT OR Apache-2.0
// The panel's settings, all from the environment (server/.env through docker-compose.yml).
export interface Config {
  // bind: the host address the panel's ports and the gateways are published on (GATEWAY_BIND);
  // listen: the panel's own listening address (0.0.0.0 inside its container)
  bind: string; listen: string; panelPort: number; routerPort: number;
  hostAddr: string; // where the panel reaches the servers' HTTP ports and gateways
  dockerUrl: string; // the socket proxy
  dataDir: string; dataHostDir: string; packDir: string; packHostDir: string;
  acserverImage: string; gatewayImage: string; gatewayOrigins: string;
  siteUrl: string; gameHost: string; // for the join links: the page, the address desktops join
  maxServers: number; pollSeconds: number;
  pools: { game: number[]; http: number[]; gateway: number[] };
  access: { teamDomain: string; aud: string; certsUrl: string };
}

// "9620-9629" or "9620,9622,9630-9631"
export function parsePool(text: string): number[] {
  const out: number[] = [];
  for (const part of text.split(',').map((s) => s.trim()).filter(Boolean)) {
    const m = /^(\d+)(?:-(\d+))?$/.exec(part);
    if (!m) throw new Error(`port pool ${text}: ${part} is no port or range`);
    const [a, b] = [Number(m[1]), Number(m[2] ?? m[1])];
    if (a < 1024 || b > 65535 || b < a || b - a > 1000) throw new Error(`port pool ${text}: ${part} is out of 1024-65535`);
    for (let p = a; p <= b; p++) out.push(p);
  }
  return out;
}

// Ports never taken from a pool: the fixed servers' (their defaults from docker-compose.yml)
// and Nextcloud's 8081 / 8082 on tsss-server.
export function reservedPorts(env: Record<string, string | undefined>): number[] {
  const fixed: [string, number][] = [
    ['SPA_GAME_PORT', 9600], ['SPA_HTTP_PORT', 8081], ['SPA_GATEWAY_PORT', 9601],
    ['LAGUNA_GAME_PORT', 9610], ['LAGUNA_HTTP_PORT', 8082], ['LAGUNA_GATEWAY_PORT', 9611],
  ];
  return [...new Set([8081, 8082, ...fixed.map(([k, d]) => Number(env[k] || d))])];
}

export function loadConfig(env: Record<string, string | undefined>): Config {
  const need = (k: string) => {
    const v = env[k];
    if (!v) throw new Error(`${k} is not set (see server/.env.example)`);
    return v;
  };
  const num = (k: string, d: number) => {
    const v = Number(env[k] || d);
    if (!Number.isInteger(v) || v < 1) throw new Error(`${k}: ${env[k]} is no whole number`);
    return v;
  };
  const bind = env.GATEWAY_BIND || '127.0.0.1';
  const teamDomain = need('CF_ACCESS_TEAM_DOMAIN');
  const config: Config = {
    bind, listen: env.PANEL_LISTEN || bind, panelPort: num('PANEL_PORT', 9680), routerPort: num('PANEL_ROUTER_PORT', 9681),
    hostAddr: env.PANEL_HOST_ADDR || bind, dockerUrl: env.PANEL_DOCKER_URL || 'http://docker-proxy:2375',
    dataDir: env.PANEL_DATA || '/data', dataHostDir: need('PANEL_DATA_DIR'), packDir: env.PANEL_PACK || '/pack', packHostDir: need('AC_PACK_DIR'),
    acserverImage: env.PANEL_ACSERVER_IMAGE || 'rustyac-acserver', gatewayImage: env.PANEL_GATEWAY_IMAGE || 'rustyac-gateway',
    gatewayOrigins: env.GATEWAY_ORIGINS || 'https://rustyac.example.com',
    siteUrl: env.PANEL_SITE_URL || `${(env.GATEWAY_ORIGINS || 'https://rustyac.example.com').split(',')[0].trim()}/`,
    gameHost: env.PANEL_GAME_HOST || 'tsss-server',
    maxServers: num('PANEL_MAX_SERVERS', 3), pollSeconds: num('PANEL_POLL_SECONDS', 30),
    pools: {
      game: parsePool(env.PANEL_GAME_PORTS || '9620-9629'),
      http: parsePool(env.PANEL_HTTP_PORTS || '9630-9639'),
      gateway: parsePool(env.PANEL_GATEWAY_PORTS || '9640-9649'),
    },
    access: { teamDomain, aud: need('CF_ACCESS_AUD'), certsUrl: env.CF_ACCESS_CERTS_URL || `https://${teamDomain}/cdn-cgi/access/certs` },
  };
  const all = [...config.pools.game, ...config.pools.http, ...config.pools.gateway, config.panelPort, config.routerPort];
  const twice = all.filter((p, i) => all.indexOf(p) !== i);
  if (twice.length) throw new Error(`ports used twice by the panel's pools and ports: ${[...new Set(twice)].join(', ')}`);
  const clash = all.filter((p) => reservedPorts(env).includes(p));
  if (clash.length) throw new Error(`the panel's ports take ports of the fixed servers or Nextcloud: ${clash.join(', ')}`);
  return config;
}

// The first free port of each pool, or null when a pool is used up.
export function takePorts(pools: Config['pools'], used: { game: number; http: number; gateway: number }[]) {
  const free = (pool: number[], taken: number[]) => pool.find((p) => !taken.includes(p));
  const game = free(pools.game, used.map((u) => u.game));
  const http = free(pools.http, used.map((u) => u.http));
  const gateway = free(pools.gateway, used.map((u) => u.gateway));
  return game && http && gateway ? { game, http, gateway } : null;
}
