#!/usr/bin/env node
/**
 * Sync published GitHub Releases into website/data/releases.json.
 * Uses only Node built-ins; never replaces the last good manifest on API errors.
 */
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

const OWNER = process.env.GITHUB_REPOSITORY?.split('/')[0] || 'sairajKhilariArch';
const REPO = process.env.GITHUB_REPOSITORY?.split('/')[1] || 'Saarthi-Studio';
const API = `https://api.github.com/repos/${OWNER}/${REPO}/releases`;
const OUTPUT = resolve('data/releases.json');
const TOKEN = process.env.GITHUB_TOKEN;

export function classifyAsset(name) {
  const n = name.toLowerCase();
  if (/\.deb$/.test(n)) return 'linuxDeb';
  if (/\.appimage$/.test(n)) return 'linuxAppImage';
  if (/linux.*\.tar\.gz$/.test(n) || /linux-x86_64.*\.tar\.gz$/.test(n)) return 'linuxTar';
  if (/\.msi$/.test(n)) return 'windowsMsi';
  if (/windows.*\.zip$/.test(n) || /windows-x86_64.*\.zip$/.test(n)) return 'windowsZip';
  if (/\.app\.zip$/.test(n) || /macos.*\.zip$/.test(n)) return 'macApp';
  if (/macos.*\.tar\.gz$/.test(n)) return 'macTar';
  if (/\.apk$/.test(n)) return 'androidApk';
  if (/\.aab$/.test(n)) return 'androidAab';
  if (/\.vsix$/.test(n)) return 'vsix';
  if (/^sha256sums\.txt$/i.test(name)) return 'sums';
  return null;
}

export function normalizeRelease(raw) {
  if (!raw || typeof raw.tag_name !== 'string' || !Array.isArray(raw.assets)) {
    throw new TypeError('GitHub returned a malformed release record');
  }
  const assets = raw.assets.map((asset) => ({
    name: String(asset.name || ''),
    size: Number.isFinite(asset.size) ? asset.size : 0,
    url: String(asset.browser_download_url || ''),
    download_count: Number.isFinite(asset.download_count) ? asset.download_count : 0,
    content_type: String(asset.content_type || ''),
    key: classifyAsset(String(asset.name || '')),
  }));
  const mapped = {};
  for (const asset of assets) {
    if (asset.key && !mapped[asset.key]) mapped[asset.key] = asset;
  }
  return {
    tag: raw.tag_name,
    version: raw.tag_name.replace(/^v/, ''),
    name: raw.name || raw.tag_name,
    date: raw.published_at ? raw.published_at.slice(0, 10) : null,
    published_at: raw.published_at || null,
    url: raw.html_url || '',
    prerelease: Boolean(raw.prerelease),
    draft: Boolean(raw.draft),
    body: typeof raw.body === 'string' ? raw.body : '',
    assets,
    mapped_assets: mapped,
  };
}

async function fetchAllReleases() {
  const headers = { Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'SAARTHI-STUDIO-release-sync' };
  if (TOKEN) headers.Authorization = `Bearer ${TOKEN}`;
  const releases = [];
  for (let page = 1; page <= 20; page += 1) {
    const response = await fetch(`${API}?per_page=100&page=${page}`, { headers, signal: AbortSignal.timeout(30000) });
    if (!response.ok) throw new Error(`GitHub Releases API returned HTTP ${response.status}`);
    const batch = await response.json();
    if (!Array.isArray(batch)) throw new TypeError('GitHub Releases API did not return an array');
    releases.push(...batch);
    if (batch.length < 100) break;
  }
  return releases;
}

export async function buildManifest(rawReleases, generatedAt = new Date().toISOString()) {
  const releases = rawReleases.map(normalizeRelease).sort((a, b) => (b.published_at || '').localeCompare(a.published_at || ''));
  const stable = releases.filter((r) => !r.prerelease && !r.draft);
  const latestStable = stable[0] || null;
  const latestWithAssets = stable.find((r) => r.assets.length > 0) || null;
  return {
    schema_version: 1,
    generated_at: generatedAt,
    repository: `${OWNER}/${REPO}`,
    latest_stable_tag: latestStable?.tag || null,
    latest_with_assets_tag: latestWithAssets?.tag || null,
    releases,
  };
}

async function main() {
  const raw = await fetchAllReleases();
  if (!raw.length) throw new Error('GitHub returned no releases; refusing to erase the existing manifest');

  let previous = null;
  try {
    previous = JSON.parse(await readFile(OUTPUT, 'utf8'));
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }

  const manifest = await buildManifest(raw, previous?.generated_at || new Date().toISOString());
  if (previous && JSON.stringify(previous) === JSON.stringify(manifest)) {
    console.log(`No release metadata changes; manifest remains at ${manifest.generated_at}`);
    return;
  }

  manifest.generated_at = new Date().toISOString();
  await mkdir(dirname(OUTPUT), { recursive: true });
  const temp = `${OUTPUT}.tmp`;
  await writeFile(temp, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
  await rename(temp, OUTPUT);
  console.log(`Synced ${manifest.releases.length} releases; latest with assets: ${manifest.latest_with_assets_tag || 'none'}`);
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(new URL(import.meta.url).pathname)) {
  main().catch((error) => { console.error(error); process.exitCode = 1; });
}
