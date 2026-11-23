/**
 * The newest PUBLISHED release, transcribed from the GitHub API
 * (releases/tags/v3.83.0, fetched 2026-11-23). The source tree can be ahead
 * of this; download pages must link assets that actually exist, so this file
 * is updated when a release is published — never guessed from Cargo.toml.
 *
 * v3.83.0 shipped Linux, Android and VS Code assets only. The Windows and
 * macOS entries stay empty on purpose: their pages say no build is attached
 * yet instead of linking a file that returns 404.
 */
export const LATEST_RELEASE = {
  tag: 'v3.83.0',
  version: '3.83.0',
  date: '2026-11-22',
  url: 'https://github.com/sairajKhilariArch/Saarthi-Studio/releases/tag/v3.83.0',
} as const;

const BASE =
  'https://github.com/sairajKhilariArch/Saarthi-Studio/releases/download/v3.83.0';

export interface Asset {
  name: string;
  size: number;
  url: string;
}

/** A platform this release has no build for. */
const MISSING: Asset = { name: '', size: 0, url: '' };

export const ASSETS = {
  linuxDeb: {
    name: 'free-ai-studio_3.83.0-1_amd64.deb',
    size: 4959224,
    url: `${BASE}/free-ai-studio_3.83.0-1_amd64.deb`,
  },
  linuxAppImage: {
    name: 'sarthi-studio_3.83.0_x86_64.AppImage',
    size: 8800760,
    url: `${BASE}/sarthi-studio_3.83.0_x86_64.AppImage`,
  },
  linuxTar: {
    name: 'sarthi-studio_3.83.0_linux-x86_64.tar.gz',
    size: 8568309,
    url: `${BASE}/sarthi-studio_3.83.0_linux-x86_64.tar.gz`,
  },
  windowsMsi: MISSING,
  windowsZip: MISSING,
  macApp: MISSING,
  macTar: MISSING,
  androidApk: {
    name: 'free-ai-studio-android.apk',
    size: 73516,
    url: `${BASE}/free-ai-studio-android.apk`,
  },
  androidAab: {
    name: 'free-ai-studio-android.aab',
    size: 126259,
    url: `${BASE}/free-ai-studio-android.aab`,
  },
  vsix: {
    name: 'free-ai-studio-vscode.vsix',
    size: 356050,
    url: `${BASE}/free-ai-studio-vscode.vsix`,
  },
  sums: {
    name: 'SHA256SUMS.txt',
    size: 214,
    url: `${BASE}/SHA256SUMS.txt`,
  },
} as const satisfies Record<string, Asset>;

/**
 * The executable inside the 3.83.0 packages: the deb installs
 * /usr/bin/free-ai-studio and the Linux tarball carries `free-ai-studio`.
 * Change this with the release that renames the binary.
 */
export const EXECUTABLE_NAME = 'free-ai-studio';

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const TOTAL_ASSETS = Object.values(ASSETS).filter((a) => a.name).length;
