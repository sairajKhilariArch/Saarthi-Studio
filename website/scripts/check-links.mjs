import { readdir, readFile, stat } from 'node:fs/promises';
import { join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = fileURLToPath(new URL('.', import.meta.url));
const websiteRoot = resolve(scriptDir, '..');
const dist = resolve(websiteRoot, 'dist');

const baseUrl = process.env.BASE_URL || process.env.SITE_BASE || '/';
const basePrefix = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`;

const errors = [];

async function walk(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(full)));
    else if (entry.isFile()) files.push(full);
  }
  return files;
}

function isLocalReference(reference) {
  if (!reference || reference.startsWith('#') || reference.startsWith('//')) return false;
  return !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(reference);
}

function stripReference(reference) {
  return reference.split('#')[0].split('?')[0];
}

function checkBase(reference) {
  if (basePrefix === '/') return reference.startsWith('/');
  return reference === baseUrl.slice(0, -1) || reference.startsWith(basePrefix);
}

function resolveReference(reference) {
  let rel = reference === basePrefix ? '' : reference.slice(basePrefix.length);
  rel = rel.replace(/^\/+/, '');

  if (!rel) return join(dist, 'index.html');
  if (rel.endsWith('/')) return join(dist, rel, 'index.html');
  return join(dist, rel);
}

function hasAttribute(html, pattern) {
  return pattern.test(html);
}

function extractReferences(html, source) {
  const refs = [];
  const re = /(?<!!)\b(?:href|src)\s*=\s*["']([^"']*)["']/gi;
  let m;
  while ((m = re.exec(html)) !== null) refs.push(m[1]);
  const srcset = html.match(/srcset="([^"]*)"/g);
  if (srcset) {
    for (const candidate of srcset) {
      for (const url of candidate.split(',')) {
        const urlPart = url.trim().split(/\s+/)[0];
        if (urlPart) refs.push(urlPart);
      }
    }
  }
  return refs;
}

async function main() {
  try {
    await stat(dist);
  } catch {
    console.error(`Build output not found at ${dist}. Run "npm run build" first.`);
    process.exit(1);
  }

  const htmlFiles = (await walk(dist)).filter((f) => f.endsWith('.html'));
  let total = 0;

  for (const file of htmlFiles) {
    const page = relative(dist, file).split(sep).join('/');
    const html = await readFile(file, 'utf8');
    const base = basePrefix === '/' ? '' : basePrefix;

    // Absolute canonical / og URLs must be https:// (never localhost).
    if (/http:\/\/localhost/i.test(html)) {
      errors.push(`${page}: contains a localhost URL.`);
    }

    const references = extractReferences(html, page);
    for (const reference of references) {
      if (!isLocalReference(reference)) continue;
      const clean = stripReference(reference);
      if (!clean) continue;

      if (!checkBase(clean)) {
        errors.push(`${page}: "${reference}" is outside the configured base ${basePrefix}.`);
        continue;
      }

      const target = resolveReference(clean);
      try {
        const info = await stat(target);
        if (info.isDirectory()) {
          errors.push(`${page}: "${reference}" resolves to a directory.`);
        }
        total += 1;
      } catch {
        errors.push(`${page}: "${reference}" does not resolve to a built file.`);
      }
    }

    // Same-document anchors must exist on the page.
    const anchorHrefs = [...html.matchAll(/href="#([a-zA-Z0-9_-]+)"/g)].map((m) => m[1]);
    for (const hash of new Set(anchorHrefs)) {
      if (!html.includes(`id="${hash}"`) && !html.includes(`id='${hash}'`)) {
        errors.push(`${page}: anchor "#${hash}" has no matching id on the page.`);
      }
    }
  }

  if (errors.length > 0) {
    console.error(`Link check failed with ${errors.length} issue(s):`);
    for (const error of errors) console.error(`- ${error}`);
    process.exit(1);
  }

  console.log(`Link check passed: ${htmlFiles.length} page(s), ${total} local reference(s) resolved.`);
}

main();
