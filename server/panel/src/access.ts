// SPDX-License-Identifier: MIT OR Apache-2.0
// Cloudflare Access: every panel request carries the Cf-Access-Jwt-Assertion header, a token
// Access signed (RS256) for this application. It is checked against the team's public keys,
// the application's AUD, its issuer and its expiry; anything else is refused.
import { createPublicKey, verify, type JsonWebKey, type KeyObject } from 'node:crypto';

export type Keys = Map<string, KeyObject>;

export function keysFromJwks(jwks: { keys?: (JsonWebKey & { kid?: string })[] }): Keys {
  const out: Keys = new Map();
  for (const k of jwks.keys ?? []) if (k.kid && k.kty === 'RSA') out.set(k.kid, createPublicKey({ key: k, format: 'jwk' }));
  return out;
}

// The verdict on a token: the e-mail it was given to, or why it is refused.
export function checkToken(token: string | undefined, keys: Keys, aud: string, issuer: string, now = Date.now() / 1000): { email: string } | { refused: string } {
  if (!token) return { refused: 'no Cf-Access-Jwt-Assertion' };
  const parts = token.split('.');
  if (parts.length !== 3) return { refused: 'not a JWT' };
  let head, body;
  try {
    head = JSON.parse(Buffer.from(parts[0], 'base64url').toString());
    body = JSON.parse(Buffer.from(parts[1], 'base64url').toString());
  } catch {
    return { refused: 'not a JWT' };
  }
  if (head.alg !== 'RS256') return { refused: `algorithm ${head.alg}` };
  const key = keys.get(head.kid);
  if (!key) return { refused: 'signed with a key that is not the team\'s' };
  if (!verify('RSA-SHA256', Buffer.from(`${parts[0]}.${parts[1]}`), key, Buffer.from(parts[2], 'base64url'))) return { refused: 'bad signature' };
  if (!(Array.isArray(body.aud) ? body.aud : [body.aud]).includes(aud)) return { refused: 'another application\'s token (AUD)' };
  if (body.iss !== issuer) return { refused: `issuer ${body.iss}` };
  if (typeof body.exp !== 'number' || body.exp <= now) return { refused: 'expired' };
  if (typeof body.nbf === 'number' && body.nbf > now + 60) return { refused: 'not valid yet' };
  return { email: String(body.email ?? body.sub ?? '') };
}

// The team's keys, fetched when a token names a key not known yet (Access rotates them),
// at most once a minute.
export class TeamKeys {
  keys: Keys = new Map();
  fetched = 0;
  url: string;
  constructor(url: string) {
    this.url = url;
  }
  async get(kid: string | undefined): Promise<Keys> {
    if ((kid && this.keys.has(kid)) || Date.now() - this.fetched < 60000) return this.keys;
    this.fetched = Date.now();
    try {
      const r = await fetch(this.url, { signal: AbortSignal.timeout(5000) });
      if (r.ok) this.keys = keysFromJwks(await r.json());
    } catch (e) {
      console.log(`access: the team's keys could not be fetched (${(e as Error).message})`);
    }
    return this.keys;
  }
}

export const tokenKid = (token: string | undefined) => {
  try {
    return JSON.parse(Buffer.from(String(token).split('.')[0], 'base64url').toString()).kid as string;
  } catch {
    return undefined;
  }
};
