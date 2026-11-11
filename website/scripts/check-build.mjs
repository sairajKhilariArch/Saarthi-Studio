import { readdir, readFile, stat } from 'node:fs/promises';
import { join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = fileURLToPath(new URL('.', import.meta.url));
const websiteRoot = resolve(scriptDir, '..');
const dist = resolve(websiteRoot, 'dist');

const rawBase = process.env.SITE_BASE || '/';
const basePrefix = rawBase.endsWith('/') ? rawBase : `${rawBase}/`;
const baseRoot = basePrefix.slice(0, -1);

const errors = [];

async function walk(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(full)));
    else files.push(full);
  }

  return files;
}

function isLocalReference(reference) {
  if (!reference || reference.startsWith('#') || reference.startsWith('//')) {
    return false;
  }
  return !/^(?:[a-z][a-z0-9+.-]*:)/i.test(reference);
}

function stripReference(reference) {
  const clean = reference.split('#')[0].split('?')[0];
  try {
    return decodeURI(clean);
  } catch {
    return clean;
  }
}

function checkBase(reference) {
  if (basePrefix === '/') return reference.startsWith('/');
  return reference === baseRoot || reference.startsWith(basePrefix);
}

function resolveReference(reference) {
  let rel =
    basePrefix === '/'
      ? reference.slice(1)
      : reference.slice(basePrefix.length);
  rel = rel.replace(/^\/+/, '');

  if (!rel || rel.endsWith('/')) {
    rel = `${rel}index.html`;
  } else if (!rel.split('/').pop()?.includes('.')) {
    rel = `${rel}/index.html`;
  }

  return resolve(dist, rel);
}

function hasAttribute(html, pattern) {
  return pattern.test(html);
}

try {
  await stat(dist);
} catch {
  console.error(`Build output not found at ${dist}. Run "npm run build" first.`);
  process.exit(1);
}

const files = await walk(dist);
const htmlFiles = files.filter((file) => file.endsWith('.html'));

if (htmlFiles.length === 0) {
  errors.push('No HTML pages were generated.');
}

for (const file of htmlFiles) {
  const page = relative(dist, file).split(sep).join('/');
  const html = await readFile(file, 'utf8');

  if (!/<html[^>]*\blang="en"/i.test(html)) {
    errors.push(`${page}: missing lang="en" on the document.`);
  }
  if (!/<title>[^<]+<\/title>/i.test(html)) {
    errors.push(`${page}: missing a non-empty title.`);
  }
  if (!hasAttribute(html, /<meta\s+name="description"\s+content="[^"]+"/i)) {
    errors.push(`${page}: missing a meta description.`);
  }
  if (!hasAttribute(html, /<link\s+rel="canonical"\s+href="https?:\/\/[^"]+"/i)) {
    errors.push(`${page}: missing an absolute canonical URL.`);
  }
  if (!hasAttribute(html, /<meta\s+property="og:title"\s+content="[^"]+"/i)) {
    errors.push(`${page}: missing og:title.`);
  }
  if ((html.match(/<h1\b/gi) || []).length !== 1) {
    errors.push(`${page}: expected exactly one h1.`);
  }
  if (html.includes('http://localhost')) {
    errors.push(`${page}: contains a localhost URL.`);
  }
  if (/SITE_BASE|SITE_URL/.test(html)) {
    errors.push(`${page}: contains an unresolved build variable.`);
  }

  const references = [
    ...html.matchAll(/\b(?:href|src)="([^"]*)"/g),
  ].map((match) => match[1]);

  for (const match of html.matchAll(/\bsrcset="([^"]*)"/g)) {
    for (const candidate of match[1].split(',')) {
      const url = candidate.trim().split(/\s+/)[0];
      if (url) references.push(url);
    }
  }

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
    } catch {
      errors.push(`${page}: "${reference}" does not resolve to a built file.`);
    }
  }
}

const index = join(dist, 'index.html');
try {
  const html = await readFile(index, 'utf8');
  if (!html.includes('Sarthi Studio')) errors.push('index.html does not name Sarthi Studio.');
} catch {
  errors.push('index.html is missing.');
}

const sitemap = join(dist, 'sitemap-index.xml');
try {
  await stat(sitemap);
} catch {
  errors.push('sitemap-index.xml is missing.');
}

const robots = join(dist, 'robots.txt');
try {
  const text = await readFile(robots, 'utf8');
  if (!/^Sitemap:\s+https?:\/\//m.test(text)) {
    errors.push('robots.txt does not declare an absolute sitemap URL.');
  }
} catch {
  errors.push('robots.txt is missing.');
}

if (errors.length > 0) {
  console.error(`Build validation failed with ${errors.length} issue(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

console.log(
  `Build validation passed: ${htmlFiles.length} HTML page(s), base "${basePrefix}", local references resolved.`,
);
