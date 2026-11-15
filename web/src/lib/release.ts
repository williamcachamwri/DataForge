import fs from 'node:fs';
import path from 'node:path';

export interface ReleaseInfo {
  version: string;
  tag: string;
  releaseUrl: string;
  releasesPageUrl: string;
  repoUrl: string;
}

export const GITHUB_OWNER = 'williamcachamwri';
export const GITHUB_REPO = 'DataForge';
export const REPO_URL = `https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}`;
export const RELEASES_PAGE_URL = `${REPO_URL}/releases`;

function normalizeTag(raw: string): { version: string; tag: string } | null {
  const cleaned = raw.trim();
  if (!cleaned) return null;
  const tag = cleaned.startsWith('v') ? cleaned : `v${cleaned}`;
  const version = tag.slice(1);
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
    return null;
  }
  return { version, tag };
}

function readCargoFallbackTag(): { version: string; tag: string } {
  const candidates = [
    path.resolve(process.cwd(), '../crates/dataforge-windows-app/Cargo.toml'),
    path.resolve(process.cwd(), 'crates/dataforge-windows-app/Cargo.toml'),
  ];

  for (const candidate of candidates) {
    try {
      const content = fs.readFileSync(candidate, 'utf8');
      const match = content.match(/^version\s*=\s*"([^"]+)"/m);
      if (match?.[1]) {
        const parsed = normalizeTag(match[1]);
        if (parsed) return parsed;
      }
    } catch {
      // Try next candidate
    }
  }

  return { version: '0.1.3', tag: 'v0.1.3' };
}

async function fetchJsonWithTimeout<T>(url: string, timeoutMs: number): Promise<T | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(url, {
      signal: controller.signal,
      headers: {
        Accept: 'application/vnd.github+json',
        'User-Agent': 'DataForge-Web-Build',
      },
    });
    if (!res.ok) return null;
    return (await res.json()) as T;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

export async function getBuildReleaseInfo(): Promise<ReleaseInfo> {
  const fallback = readCargoFallbackTag();

  const latestRelease = await fetchJsonWithTimeout<{ tag_name?: string; html_url?: string }>(
    `https://api.github.com/repos/${GITHUB_OWNER}/${GITHUB_REPO}/releases/latest`,
    2200
  );

  if (latestRelease?.tag_name) {
    const normalized = normalizeTag(latestRelease.tag_name);
    if (normalized) {
      return {
        version: normalized.version,
        tag: normalized.tag,
        releaseUrl:
          latestRelease.html_url || `${REPO_URL}/releases/tag/${encodeURIComponent(normalized.tag)}`,
        releasesPageUrl: RELEASES_PAGE_URL,
        repoUrl: REPO_URL,
      };
    }
  }

  const tags = await fetchJsonWithTimeout<Array<{ name?: string }>>(
    `https://api.github.com/repos/${GITHUB_OWNER}/${GITHUB_REPO}/tags?per_page=1`,
    1800
  );

  if (Array.isArray(tags) && tags[0]?.name) {
    const normalized = normalizeTag(tags[0].name);
    if (normalized) {
      return {
        version: normalized.version,
        tag: normalized.tag,
        releaseUrl: `${REPO_URL}/releases/tag/${encodeURIComponent(normalized.tag)}`,
        releasesPageUrl: RELEASES_PAGE_URL,
        repoUrl: REPO_URL,
      };
    }
  }

  return {
    version: fallback.version,
    tag: fallback.tag,
    releaseUrl: `${REPO_URL}/releases/tag/${encodeURIComponent(fallback.tag)}`,
    releasesPageUrl: RELEASES_PAGE_URL,
    repoUrl: REPO_URL,
  };
}
