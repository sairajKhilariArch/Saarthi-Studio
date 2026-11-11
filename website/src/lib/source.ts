import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

let cached: string | null | undefined;

/**
 * The website documents the source tree, which can be ahead of the newest
 * published release. Read the version from Cargo.toml rather than hardcoding
 * it; fail soft so a website build never depends on a package layout detail.
 */
export function getSourceVersion(): string | null {
  if (cached !== undefined) return cached;

  const candidates = [
    resolve(process.cwd(), '../app/Cargo.toml'),
    resolve(process.cwd(), 'app/Cargo.toml'),
  ];

  for (const candidate of candidates) {
    try {
      const cargo = readFileSync(candidate, 'utf8');
      const match = cargo.match(/^version\s*=\s*"([^"]+)"/m);
      if (match?.[1]) {
        cached = match[1];
        return cached;
      }
    } catch {
      // Try the other layout; report null if neither works.
    }
  }

  cached = null;
  return cached;
}
