// A stand-in for the Docker Engine API behind the socket proxy, on 127.0.0.1 only: the panel's
// container calls become local processes. "rustyac-acserver" = the copy of Kunos' acServer.exe
// in re/scratch/task38e/acserver/ in a folder laid out as server/entrypoint.sh lays it out;
// "rustyac-gateway" = target/release/rustyac-gateway.exe on the port the acServer container
// publishes for 9601. Every call is written to calls.log (method, path, the create bodies).
//   node tools/web_checks/task38e/fake_docker.mjs <port> <run folder>
import { spawn } from 'node:child_process';
import { appendFileSync, cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join, resolve } from 'node:path';

const [port, runDir] = [Number(process.argv[2]), resolve(process.argv[3])];
const repo = resolve(import.meta.dirname, '../../..');
mkdirSync(runDir, { recursive: true });
const log = (line) => appendFileSync(join(runDir, 'calls.log'), `${new Date().toISOString()} ${line}\n`);
const containers = new Map(); // name -> { Id, Name, spec, State, proc, out: Buffer[] }
let next = 1;

// "C:/x/y:/pack:ro" -> ["C:/x/y", "/pack"]
const bindOf = (spec, inside) => spec.HostConfig.Binds.map((b) => /^(.*):(\/[^:]*)(?::(ro|rw))?$/.exec(b)).find((m) => m && m[2] === inside)?.[1];
const envOf = (spec) => Object.fromEntries(spec.Env.map((e) => [e.slice(0, e.indexOf('=')), e.slice(e.indexOf('=') + 1)]));
const find = (key) => containers.get(key) || [...containers.values()].find((c) => c.Id === key);

function start(c) {
  const env = envOf(c.spec);
  let proc;
  if (c.spec.Image === 'rustyac-acserver') {
    // server/entrypoint.sh's layout, with the passwords filled in the same way
    const pack = bindOf(c.spec, '/pack');
    const cfg = env.AC_CFG === '/cfg' ? bindOf(c.spec, '/cfg') : join(pack, 'cfg', env.AC_SERVER);
    const work = join(runDir, 'work', c.Name);
    rmSync(work, { recursive: true, force: true });
    mkdirSync(join(work, 'cfg'), { recursive: true });
    for (const d of ['content', 'system']) cpSync(join(pack, d), join(work, d), { recursive: true });
    cpSync(join(repo, 're/scratch/task38e/acserver/acServer.exe'), join(work, 'acServer.exe'));
    writeFileSync(join(work, 'blacklist.txt'), '');
    const text = readFileSync(join(cfg, 'server_cfg.ini'), 'utf8').replace('@ADMIN_PASSWORD@', env.AC_ADMIN_PASSWORD).replace(/^PASSWORD=.*$/m, `PASSWORD=${env.AC_PASSWORD}`);
    if (!/^REGISTER_TO_LOBBY=0$/m.test(text)) throw new Error('the server would register to the lobby');
    writeFileSync(join(work, 'cfg', 'server_cfg.ini'), text);
    cpSync(join(cfg, 'entry_list.ini'), join(work, 'cfg', 'entry_list.ini'));
    proc = spawn(join(work, 'acServer.exe'), [], { cwd: work });
  } else if (c.spec.Image === 'rustyac-gateway') {
    // the gateway shares the acServer container's network: its 9601 is what that one publishes
    const ac = find(c.spec.HostConfig.NetworkMode.replace(/^container:/, ''));
    const b = ac.spec.HostConfig.PortBindings['9601/tcp'][0];
    proc = spawn(join(repo, 'target/release/rustyac-gateway.exe'), [], { env: { ...process.env, ...env, GATEWAY_LISTEN: `${b.HostIp}:${b.HostPort}` } });
  } else {
    throw new Error(`no such image ${c.spec.Image}`);
  }
  // Docker's log stream without a TTY: 8-byte headers (stream, size) before each chunk
  const frame = (stream) => (d) => {
    const h = Buffer.alloc(8);
    h[0] = stream;
    h.writeUInt32BE(d.length, 4);
    c.out.push(h, d);
  };
  proc.stdout.on('data', frame(1));
  proc.stderr.on('data', frame(2));
  proc.on('exit', (code) => {
    log(`${c.Name} exited ${code}`);
    if (c.proc === proc) c.State = 'exited';
  });
  c.proc = proc;
  c.State = 'running';
}
function stop(c) {
  if (c.proc && c.State === 'running') c.proc.kill();
  c.State = 'exited';
}

const send = (res, status, body) => {
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(body === undefined ? '' : JSON.stringify(body));
};

createServer((req, res) => {
  let text = '';
  req.on('data', (d) => (text += d));
  req.on('end', () => {
    const url = new URL(req.url, 'http://docker');
    log(`${req.method} ${req.url}${text ? ` ${text}` : ''}`);
    try {
      if (req.method === 'GET' && url.pathname === '/containers/json') {
        const label = JSON.parse(url.searchParams.get('filters') || '{}').label?.[0];
        return send(res, 200, [...containers.values()].filter((c) => !label || label in (c.spec.Labels || {})).map((c) => ({ Id: c.Id, Names: [`/${c.Name}`], State: c.State, Labels: c.spec.Labels || {} })));
      }
      if (req.method === 'POST' && url.pathname === '/containers/create') {
        const name = url.searchParams.get('name');
        if (containers.has(name)) return send(res, 409, { message: `Conflict. The container name "/${name}" is already in use` });
        const Id = `fake${next++}`;
        containers.set(name, { Id, Name: name, spec: JSON.parse(text), State: 'created', out: [] });
        return send(res, 201, { Id, Warnings: [] });
      }
      const m = /^\/containers\/([^/]+)(?:\/(start|stop|logs))?$/.exec(url.pathname);
      const c = m && find(m[1]);
      if (!c) return send(res, 404, { message: `No such container: ${m ? m[1] : url.pathname}` });
      if (req.method === 'POST' && m[2] === 'start') {
        if (c.State === 'running') return send(res, 304);
        start(c);
        return send(res, 204);
      }
      if (req.method === 'POST' && m[2] === 'stop') {
        if (c.State !== 'running') return send(res, 304);
        stop(c);
        return send(res, 204);
      }
      if (req.method === 'GET' && m[2] === 'logs') {
        const all = Buffer.concat(c.out);
        res.writeHead(200, { 'Content-Type': 'application/vnd.docker.raw-stream' });
        return res.end(all);
      }
      if (req.method === 'DELETE' && !m[2]) {
        stop(c);
        containers.delete(c.Name);
        return send(res, 204);
      }
      send(res, 404, { message: 'not in the stand-in' });
    } catch (e) {
      log(`error ${e.message}`);
      send(res, 500, { message: e.message });
    }
  });
}).listen(port, '127.0.0.1', () => console.log(`fake docker on 127.0.0.1:${port}`));
process.on('SIGTERM', () => {
  for (const c of containers.values()) stop(c);
  process.exit(0);
});
