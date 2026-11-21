import { AppConfig, DirectoryStreamer, Platform, ShardCode } from '../types/radar';

const SUFFIX_RE = /(?:[\s._-]+(?:yt|youtube|live|ttv|tv|gaming|plays|op)|(?:yt|ttv))$/i;

export function cleanIgn(gameName: string): string {
  return gameName.trim().replace(SUFFIX_RE, '').trim();
}

export function normalizeText(s: string): string {
  return s
    .toLowerCase()
    .split('')
    .filter((ch) => /[a-z0-9]/.test(ch))
    .join('');
}

export function canonicalPlatform(p: string): Platform | null {
  switch (p.trim().toLowerCase()) {
    case 'youtube':
    case 'yt':
      return 'YouTube';
    case 'twitch':
      return 'Twitch';
    case 'kick':
      return 'Kick';
    default:
      return null;
  }
}

export function hostOf(url: string): string | null {
  let rest = '';
  if (url.startsWith('https://')) {
    rest = url.slice(8);
  } else if (url.startsWith('http://')) {
    rest = url.slice(7);
  } else {
    return null;
  }
  const host = rest.split(/[/?#]/)[0]?.toLowerCase() ?? '';
  if (!host || !/^[a-z0-9.-]+$/.test(host)) {
    return null; // Rejects user@host tricks, ports, spaces, etc.
  }
  return host;
}

export function hostAllowed(platform: Platform, host: string): boolean {
  const h = host.replace(/^www\./, '').replace(/^m\./, '');
  switch (platform) {
    case 'YouTube':
      return h === 'youtube.com' || h === 'youtu.be';
    case 'Twitch':
      return h === 'twitch.tv';
    case 'Kick':
      return h === 'kick.com';
    default:
      return false;
  }
}

export function validGithubSegment(s: string): boolean {
  const t = s.trim();
  return t.length > 0 && t.length <= 100 && /^[A-Za-z0-9._-]+$/.test(t);
}

export function validateSubmission(
  rid: string,
  platform: Platform,
  cname: string,
  curl: string
): string | null {
  const hashIdx = rid.indexOf('#');
  if (hashIdx === -1) {
    return 'Riot ID must look like Name#TAG.';
  }
  const name = rid.slice(0, hashIdx).trim();
  const tag = rid.slice(hashIdx + 1);
  if (
    name.length < 3 ||
    name.length > 16 ||
    tag.length < 3 ||
    tag.length > 5 ||
    !/^[a-zA-Z0-9]+$/.test(tag)
  ) {
    return 'Riot ID looks invalid (name 3-16 chars, tag 3-5 letters/digits).';
  }
  if (!cname.trim() || cname.trim().length > 64) {
    return 'Enter a channel display name (max 64 characters).';
  }
  const host = hostOf(curl.trim());
  if (!host) {
    return 'Channel URL must start with https:// and be a plain link.';
  }
  if (!hostAllowed(platform, host)) {
    return `That URL is not a ${platform} link.`;
  }
  return null;
}

export function buildGithubIssueUrl(
  owner: string,
  repo: string,
  entry: DirectoryStreamer
): string {
  const title = `Add Streamer: ${entry.riot_id}`;
  const jsonPayload = JSON.stringify(entry, null, 2);
  const body = `### New Streamer Submission\n\n\`\`\`json\n${jsonPayload}\n\`\`\``;
  return `https://github.com/${encodeURIComponent(owner.trim())}/${encodeURIComponent(
    repo.trim()
  )}/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
}

export function resolveEffectiveShard(
  config: AppConfig,
  detectedShard: ShardCode
): ShardCode {
  if (config.shard_override !== 'auto') {
    return config.shard_override;
  }
  return detectedShard;
}

export function formatRadarTimestamp(date: Date = new Date()): string {
  const day = String(date.getDate()).padStart(2, '0');
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const month = months[date.getMonth()];
  const year = date.getFullYear();
  let hours = date.getHours();
  const ampm = hours >= 12 ? 'PM' : 'AM';
  hours = hours % 12 || 12;
  const mins = String(date.getMinutes()).padStart(2, '0');
  return `${day} ${month} ${year} • ${String(hours).padStart(2, '0')}:${mins} ${ampm}`;
}
