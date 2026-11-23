import test from 'node:test';
import assert from 'node:assert/strict';
import { buildManifest, classifyAsset, normalizeRelease } from '../scripts/sync-releases.mjs';

test('classifies the supported release asset formats', () => {
  const cases = [
    ['SAARTHI-STUDIO_3.84.0-1_amd64.deb', 'linuxDeb'],
    ['SAARTHI-STUDIO_3.84.0_x86_64.AppImage', 'linuxAppImage'],
    ['SAARTHI-STUDIO_3.84.0_linux-x86_64.tar.gz', 'linuxTar'],
    ['SAARTHI-STUDIO_3.84.0_windows-x86_64.msi', 'windowsMsi'],
    ['SAARTHI-STUDIO_3.84.0_windows-x86_64.zip', 'windowsZip'],
    ['SAARTHI-STUDIO_3.84.0_macos-universal.app.zip', 'macApp'],
    ['SAARTHI-STUDIO_3.84.0_macos-universal.tar.gz', 'macTar'],
    ['SAARTHI-STUDIO-android.apk', 'androidApk'],
    ['SAARTHI-STUDIO-android.aab', 'androidAab'],
    ['SAARTHI-STUDIO-vscode.vsix', 'vsix'],
    ['SHA256SUMS.txt', 'sums'],
  ];
  for (const [name, expected] of cases) assert.equal(classifyAsset(name), expected, name);
  assert.equal(classifyAsset('checksums.json'), null);
});

test('normalizes release metadata and preserves unrecognized assets', () => {
  const release = normalizeRelease({
    tag_name: 'v3.84.0',
    name: 'SAARTHI-STUDIO 3.84.0',
    published_at: '2026-11-22T09:17:31Z',
    html_url: 'https://example.test/release',
    assets: [
      { name: 'SAARTHI-STUDIO_3.84.0_linux-x86_64.tar.gz', size: 1234, browser_download_url: 'https://example.test/app.tar.gz', download_count: 4 },
      { name: 'extra.json', size: 10, browser_download_url: 'https://example.test/extra.json' },
    ],
  });
  assert.equal(release.version, '3.84.0');
  assert.equal(release.assets.length, 2);
  assert.equal(release.assets[0].key, 'linuxTar');
  assert.equal(release.assets[1].key, null);
  assert.equal(release.assets[0].download_count, 4);
});

test('manifest selects newest stable and newest stable release with assets', async () => {
  const make = (tag, published_at, prerelease, assets) => ({
    tag_name: tag,
    published_at,
    prerelease,
    draft: false,
    html_url: `https://example.test/${tag}`,
    assets,
  });
  const manifest = await buildManifest([
    make('v3.8.0', '2026-11-22T09:00:00Z', false, [{ name: 'app.apk', size: 2, browser_download_url: 'https://example.test/app.apk' }]),
    make('v4.0.0-rc.1', '2026-11-23T10:00:00Z', true, [{ name: 'preview.zip', size: 1, browser_download_url: 'https://example.test/preview.zip' }]),
    make('v3.9.0', '2026-11-23T09:00:00Z', false, []),
  ], '2026-11-23T12:00:00Z');
  assert.equal(manifest.latest_stable_tag, 'v3.9.0');
  assert.equal(manifest.latest_with_assets_tag, 'v3.8.0');
  assert.equal(manifest.releases.length, 3);
});
