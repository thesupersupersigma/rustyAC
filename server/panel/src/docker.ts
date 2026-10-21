// SPDX-License-Identifier: MIT OR Apache-2.0
// The few Docker Engine API calls the panel makes, through the socket proxy (plain HTTP).
// No API version in the paths: the daemon's own is taken (Docker 29 refuses versions below 1.44).
import { request } from 'node:http';

export class Docker {
  url: URL;
  constructor(url: string) {
    this.url = new URL(url);
  }

  call(method: string, path: string, body?: unknown): Promise<{ status: number; data: Buffer }> {
    const payload = body === undefined ? undefined : Buffer.from(JSON.stringify(body));
    return new Promise((resolve, reject) => {
      const req = request({
        host: this.url.hostname, port: this.url.port || 2375, method, path, timeout: 30000,
        headers: payload ? { 'Content-Type': 'application/json', 'Content-Length': payload.length } : {},
      }, (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (c: Buffer) => chunks.push(c));
        res.on('end', () => resolve({ status: res.statusCode ?? 0, data: Buffer.concat(chunks) }));
      });
      req.on('timeout', () => req.destroy(new Error(`docker ${method} ${path}: no answer`)));
      req.on('error', reject);
      req.end(payload);
    });
  }

  async ok(method: string, path: string, body?: unknown, also: number[] = []) {
    const r = await this.call(method, path, body);
    if (r.status >= 300 && !also.includes(r.status)) throw new Error(`docker ${method} ${path.split('?')[0]}: ${r.status} ${r.data.toString().slice(0, 300)}`);
    return r.data.length ? JSON.parse(r.data.toString()) : null;
  }

  list(label: string): Promise<{ Id: string; Names: string[]; State: string; Labels: Record<string, string> }[]> {
    return this.ok('GET', `/containers/json?all=1&filters=${encodeURIComponent(JSON.stringify({ label: [label] }))}`);
  }
  async create(name: string, spec: unknown): Promise<string> {
    return (await this.ok('POST', `/containers/create?name=${encodeURIComponent(name)}`, spec)).Id;
  }
  start = (id: string) => this.ok('POST', `/containers/${id}/start`, undefined, [304]);
  stop = (id: string) => this.ok('POST', `/containers/${id}/stop?t=5`, undefined, [304, 404]);
  remove = (id: string) => this.ok('DELETE', `/containers/${id}?force=1`, undefined, [404]);

  // the last lines of a container's output (the stream without a TTY: 8-byte frame headers)
  async logs(id: string, tail = 20): Promise<string[]> {
    const r = await this.call('GET', `/containers/${id}/logs?stdout=1&stderr=1&tail=${tail}`);
    if (r.status !== 200) return [];
    const parts: Buffer[] = [];
    for (let at = 0; at + 8 <= r.data.length;) {
      const size = r.data.readUInt32BE(at + 4);
      parts.push(r.data.subarray(at + 8, at + 8 + size));
      at += 8 + size;
    }
    return Buffer.concat(parts).toString().split(/\r?\n/).filter(Boolean).slice(-tail);
  }
}
