/**
 * The newest PUBLISHED release, transcribed from the GitHub API
 * (releases/tags/v3.74.4, fetched 2026-11-20). The source tree can be ahead
 * of this; download pages must link assets that actually exist, so this file
 * is updated when a release is published — never guessed from Cargo.toml.
 */
export const LATEST_RELEASE = {
  tag: 'v3.74.4',
  version: '3.74.4',
  date: '2026-11-20',
  url: 'https://github.com/sairajKhilariArch/Saarthi-Studio/releases/tag/v3.74.4',
} as const;

const BASE =
  'https://github.com/sairajKhilariArch/Saarthi-Studio/releases/download/v3.74.4';

export interface Asset {
  name: string;
  size: number;
  url: string;
}

export const ASSETS = {
  linuxDeb: {
    name: 'SAARTHI-STUDIO_3.74.4-1_amd64.deb',
    size: 4868860,
    url: `${BASE}/SAARTHI-STUDIO_3.74.4-1_amd64.deb`,
  },
  linuxAppImage: {
    name: 'SAARTHI-STUDIO_3.74.4_x86_64.AppImage',
    size: 8686072,
    url: `${BASE}/SAARTHI-STUDIO_3.74.4_x86_64.AppImage`,
  },
  linuxTar: {
    name: 'SAARTHI-STUDIO_3.74.4_linux-x86_64.tar.gz',
    size: 8431433,
    url: `${BASE}/SAARTHI-STUDIO_3.74.4_linux-x86_64.tar.gz`,
  },
  windowsMsi: {
    name: 'SAARTHI-STUDIO_3.74.4_windows-x86_64.msi',
    size: 7294976,
    url: `${BASE}/SAARTHI-STUDIO_3.74.4_windows-x86_64.msi`,
  },
  windowsZip: {
    name: 'SAARTHI-STUDIO_3.74.4_windows-x86_64.zip',
    size: 9047077,
    url: `${BASE}/SAARTHI-STUDIO_3.74.4_windows-x86_64.zip`,
  },
  macApp: {
    name: 'SAARTHI-STUDIO_3.74.4_macos-universal.app.zip',
    size: 15082405,
    url: `${BASE}/SAARTHI-STUDIO_3.74.4_macos-universal.app.zip`,
  },
  macTar: {
    name: 'SAARTHI-STUDIO_3.74.4_macos-universal.tar.gz',
    size: 15077832,
    url: `${BASE}/SAARTHI-STUDIO_3.74.4_macos-universal.tar.gz`,
  },
  androidApk: {
    name: 'SAARTHI-STUDIO-android.apk',
    size: 72960,
    url: `${BASE}/SAARTHI-STUDIO-android.apk`,
  },
  androidAab: {
    name: 'SAARTHI-STUDIO-android.aab',
    size: 125700,
    url: `${BASE}/SAARTHI-STUDIO-android.aab`,
  },
  vsix: {
    name: 'SAARTHI-STUDIO-vscode.vsix',
    size: 342343,
    url: `${BASE}/SAARTHI-STUDIO-vscode.vsix`,
  },
  sums: {
    name: 'SHA256SUMS.txt',
    size: 214,
    url: `${BASE}/SHA256SUMS.txt`,
  },
} as const satisfies Record<string, Asset>;

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const TOTAL_ASSETS = Object.values(ASSETS).filter((a) => a.name).length;
