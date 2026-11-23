import manifest from '../../data/releases.json';

export interface Asset {
  name: string;
  size: number;
  url: string;
  download_count?: number;
  content_type?: string;
  key?: string | null;
}

export interface Release {
  tag: string;
  version: string;
  name: string;
  date: string | null;
  published_at: string | null;
  url: string;
  prerelease: boolean;
  draft: boolean;
  body: string;
  assets: Asset[];
  mapped_assets?: Record<string, Asset>;
}

const releases = manifest.releases as Release[];
const stableReleases = releases.filter((release) => !release.prerelease && !release.draft);
const latestWithAssets = stableReleases.find((release) => release.assets.length > 0);
const latestStable = stableReleases[0];

function mapped(release: Release | undefined, key: string): Asset {
  const found = release?.assets.find((asset) => asset.key === key);
  return found ?? { name: '', size: 0, url: '' };
}

/** Newest stable release that actually contains downloadable build files. */
export const LATEST_RELEASE = latestWithAssets ?? latestStable ?? {
  tag: 'No published release',
  version: '',
  date: null,
  url: 'https://github.com/sairajKhilariArch/Saarthi-Studio/releases',
};

/** Newest stable release, even if its workflow has not attached assets yet. */
export const LATEST_STABLE_RELEASE = latestStable ?? LATEST_RELEASE;
export const RELEASES = releases.filter((release) => !release.draft);
export const ALL_RELEASES = releases;
export const ASSETS = {
  linuxDeb: mapped(latestWithAssets, 'linuxDeb'),
  linuxAppImage: mapped(latestWithAssets, 'linuxAppImage'),
  linuxTar: mapped(latestWithAssets, 'linuxTar'),
  windowsMsi: mapped(latestWithAssets, 'windowsMsi'),
  windowsZip: mapped(latestWithAssets, 'windowsZip'),
  macApp: mapped(latestWithAssets, 'macApp'),
  macTar: mapped(latestWithAssets, 'macTar'),
  androidApk: mapped(latestWithAssets, 'androidApk'),
  androidAab: mapped(latestWithAssets, 'androidAab'),
  vsix: mapped(latestWithAssets, 'vsix'),
  sums: mapped(latestWithAssets, 'sums'),
};

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const TOTAL_ASSETS = latestWithAssets?.assets.length ?? 0;

/** True when the newest published package predates the canonical executable rename. */
export const LEGACY_EXECUTABLE_NAMES = latestWithAssets?.assets.some((asset) => /^(free-ai-studio|sarthi-studio)[_-]/i.test(asset.name)) ?? false;
export const EXECUTABLE_NAME = LEGACY_EXECUTABLE_NAMES ? 'free-ai-studio' : 'SAARTHI-STUDIO';
export const MAC_APP_NAME = LEGACY_EXECUTABLE_NAMES ? 'Sarthi Studio.app' : 'SAARTHI-STUDIO.app';
