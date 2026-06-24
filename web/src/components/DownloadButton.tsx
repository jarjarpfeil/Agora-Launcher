'use client';

import { useState, useEffect } from 'react';
import { GITHUB_API_LATEST_RELEASE, GITHUB_RELEASES_URL } from '@/lib/site';

interface ReleaseAsset {
  name: string;
  browser_download_url: string;
  size: number;
}

interface LatestRelease {
  tag_name: string;
  assets: ReleaseAsset[];
}

type DetectedOS = 'windows' | 'macos' | 'linux' | 'unknown';

function detectOS(): DetectedOS {
  if (typeof navigator === 'undefined') return 'unknown';
  const ua = navigator.userAgent;
  if (ua.includes('Win')) return 'windows';
  if (ua.includes('Mac')) return 'macos';
  if (ua.includes('Linux')) return 'linux';
  return 'unknown';
}

function pickAsset(os: DetectedOS, assets: ReleaseAsset[]): ReleaseAsset | null {
  if (assets.length === 0) return null;
  const findMatch = (pred: (name: string) => boolean) =>
    assets.find((a) => pred(a.name.toLowerCase()));

  if (os === 'windows') {
    return findMatch((n) => n.endsWith('.msi')) || findMatch((n) => n.endsWith('.exe')) || null;
  }
  if (os === 'macos') {
    return findMatch((n) => n.endsWith('.dmg')) || null;
  }
  if (os === 'linux') {
    return findMatch((n) => n.endsWith('.appimage')) || findMatch((n) => n.endsWith('.deb')) || null;
  }
  return null;
}

const OS_INFO: Record<DetectedOS, { label: string; icon: string }> = {
  windows: { label: 'Windows', icon: '🪟' },
  macos: { label: 'macOS', icon: '🍎' },
  linux: { label: 'Linux', icon: '🐧' },
  unknown: { label: '', icon: '⬇' },
};

export function DownloadButton() {
  const [os, setOs] = useState<DetectedOS>('unknown');
  const [asset, setAsset] = useState<ReleaseAsset | null>(null);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setOs(detectOS());
    fetch(GITHUB_API_LATEST_RELEASE)
      .then((res) => {
        if (!res.ok) throw new Error(`${res.status}`);
        return res.json();
      })
      .then((data: LatestRelease) => {
        setAsset(pickAsset(detectOS(), data.assets || []));
        setLoading(false);
      })
      .catch(() => {
        setFailed(true);
        setLoading(false);
      });
  }, []);

  const osInfo = OS_INFO[os];
  const label = loading
    ? 'Download Agora'
    : asset
    ? `Download for ${osInfo.label}`
    : os !== 'unknown'
    ? `Download for ${osInfo.label}`
    : 'Download Agora';
  const href = asset?.browser_download_url || GITHUB_RELEASES_URL;

  return (
    <div className="flex flex-col items-center gap-2">
      <a
        href={href}
        className="rounded-lg bg-white px-5 py-3 font-semibold text-indigo-700 shadow-sm hover:bg-gray-100"
        target={asset ? '_blank' : undefined}
        rel={asset ? 'noopener noreferrer' : undefined}
      >
        {osInfo.icon} {label}
        {asset?.size ? ` · ${(asset.size / 1048576).toFixed(1)} MB` : ''}
      </a>
      <div className="flex flex-col items-center gap-1">
        <a
          href={GITHUB_RELEASES_URL}
          className="text-sm text-indigo-100 hover:text-white hover:underline"
          target="_blank"
          rel="noopener noreferrer"
        >
          All platforms →
        </a>
        <p className="text-xs text-indigo-100/60">
          Ignore releases tagged <code className="text-indigo-100/80">registry-*</code> — download the app file for your OS.
        </p>
      </div>
      {!asset && !loading && (
        <p className="max-w-xs text-center text-xs text-indigo-100/70">
          ⚠️ On the releases page, download the file for your platform (`.msi`, `.dmg`, or `.AppImage`). Ignore releases tagged `registry-*` — those are database updates, not the app itself.
        </p>
      )}
    </div>
  );
}
