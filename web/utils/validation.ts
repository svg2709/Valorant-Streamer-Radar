import {
  AppConfig,
  COMMUNITY_REPO_NAME,
  COMMUNITY_REPO_OWNER,
  ConfidenceTier,
  DateRangeFilter,
  DirectoryStreamer,
  Platform,
  SERVER_REGIONS,
  ServerRegionCode,
  ServerRegionInfo,
  StatusFilter,
  StreamerEntry,
  getServerRegionInfo,
} from '../types/radar';

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

export function compareSemver(a: string, b: string): number {
  const parse = (v: string) =>
    v
      .trim()
      .replace(/^v/i, '')
      .split('-')[0]
      .split('.')
      .map((n) => parseInt(n, 10) || 0);
  const pa = parse(a);
  const pb = parse(b);
  const len = Math.max(pa.length, pb.length, 3);
  for (let i = 0; i < len; i++) {
    const va = pa[i] ?? 0;
    const vb = pb[i] ?? 0;
    if (va > vb) return 1;
    if (va < vb) return -1;
  }
  return 0;
}

export function slugifyStreamerId(riotId: string, platform: Platform): string {
  const slug = riotId
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  return `${slug || 'streamer'}-${platform.toLowerCase()}`;
}

export function parseAliasesCsv(raw: string, primaryRiotId: string): string[] {
  const primaryLower = primaryRiotId.trim().toLowerCase();
  const seen = new Set<string>();
  const out: string[] = [];
  for (const part of raw.split(',')) {
    const trimmed = part.trim();
    const low = trimmed.toLowerCase();
    if (!trimmed || low === primaryLower || seen.has(low)) continue;
    seen.add(low);
    out.push(trimmed);
  }
  return out;
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

export function buildGithubIssueUrl(entry: DirectoryStreamer): string {
  const title = `Add Streamer: ${entry.riot_id}`;
  const cleanPayload: Record<string, unknown> = {
    id: entry.id || slugifyStreamerId(entry.riot_id, entry.platform),
    display_name: entry.display_name || entry.channel_name,
    riot_id: entry.riot_id,
    aliases: entry.aliases ?? [],
    platform: entry.platform,
    channel_name: entry.channel_name,
    channel_url: entry.channel_url,
    verified: false,
    source: 'community-submission',
  };
  if (entry.server_region?.trim()) {
    cleanPayload.server_region = entry.server_region.trim();
  }
  const jsonPayload = JSON.stringify(cleanPayload, null, 2);
  const body = `### New Streamer Submission\n\nMaintainers review and verify submissions before merging into the central \`streamers.json\` database.\n\n\`\`\`json\n${jsonPayload}\n\`\`\``;
  return `https://github.com/${COMMUNITY_REPO_OWNER}/${COMMUNITY_REPO_NAME}/issues/new?labels=streamer-database&title=${encodeURIComponent(
    title
  )}&body=${encodeURIComponent(body)}`;
}

export function resolveEffectiveRegion(
  config: AppConfig,
  detectedRegion: ServerRegionCode
): ServerRegionInfo {
  if (config.server_override !== 'auto') {
    const found = SERVER_REGIONS.find((r) => r.code === config.server_override);
    if (found) return found;
  }
  return getServerRegionInfo(detectedRegion);
}

export function resolveEntryTier(entry: StreamerEntry): ConfidenceTier {
  if (entry.confidence_tier) return entry.confidence_tier;
  if (entry.source === 'Directory' || entry.source === 'Chat') return 'Confirmed';
  if ((entry.confidence ?? 0) >= 85) return 'Confirmed';
  if ((entry.confidence ?? 0) >= 65) return 'Probable';
  return 'Unverified';
}

export interface HistoryFilters {
  searchQuery: string;
  teamFilter: 'All' | 'Ally' | 'Enemy';
  matchTypeFilter: string;
  serverFilter: string;
  dateRangeFilter: DateRangeFilter;
  statusFilter: StatusFilter;
}

export function matchesHistoryFilters(
  entry: StreamerEntry,
  filters: HistoryFilters,
  nowMs: number = Date.now()
): boolean {
  if (
    filters.teamFilter !== 'All' &&
    entry.team_relation.toLowerCase() !== filters.teamFilter.toLowerCase()
  ) {
    return false;
  }

  if (
    filters.matchTypeFilter !== 'All' &&
    entry.queue_name.toLowerCase() !== filters.matchTypeFilter.toLowerCase()
  ) {
    return false;
  }

  if (filters.serverFilter !== 'All') {
    const entryServer = (entry.server_region || '').trim().toLowerCase();
    if (entryServer !== filters.serverFilter.trim().toLowerCase()) {
      return false;
    }
  }

  if (filters.statusFilter !== 'all') {
    if (filters.statusFilter === 'Chat') {
      if (entry.source !== 'Chat' && !entry.chat_message) return false;
    } else if (resolveEntryTier(entry) !== filters.statusFilter) {
      return false;
    }
  }

  if (filters.dateRangeFilter !== 'all') {
    const parsedMs = entry.detected_at_iso ? Date.parse(entry.detected_at_iso) : NaN;
    if (!Number.isNaN(parsedMs)) {
      const ageMs = nowMs - parsedMs;
      if (filters.dateRangeFilter === 'today') {
        const entryDate = new Date(parsedMs);
        const nowDate = new Date(nowMs);
        const sameDay =
          entryDate.getFullYear() === nowDate.getFullYear() &&
          entryDate.getMonth() === nowDate.getMonth() &&
          entryDate.getDate() === nowDate.getDate();
        if (!sameDay) return false;
      } else if (filters.dateRangeFilter === '7d' && ageMs > 7 * 86_400_000) {
        return false;
      } else if (filters.dateRangeFilter === '30d' && ageMs > 30 * 86_400_000) {
        return false;
      }
    }
  }

  const q = filters.searchQuery.trim().toLowerCase();
  if (!q) return true;
  return (
    entry.riot_id.toLowerCase().includes(q) ||
    entry.platform.toLowerCase().includes(q) ||
    entry.channel.toLowerCase().includes(q) ||
    entry.title.toLowerCase().includes(q) ||
    (entry.chat_message?.toLowerCase().includes(q) ?? false) ||
    (entry.server_region?.toLowerCase().includes(q) ?? false) ||
    (entry.match_reason?.toLowerCase().includes(q) ?? false) ||
    entry.map_name.toLowerCase().includes(q) ||
    entry.agent_name.toLowerCase().includes(q) ||
    entry.queue_name.toLowerCase().includes(q)
  );
}

export function formatViewers(count: number): string {
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M watching`;
  if (count >= 1_000) return `${(count / 1_000).toFixed(1)}K watching`;
  return `${count} watching`;
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

export interface ChatStreamPlugExtraction {
  platform: Platform;
  url: string;
  channel: string;
  matchType: 'direct-url' | 'shorthand' | 'live-mention';
  rawMessage: string;
}

const CHAT_STOP_WORDS = new Set([
  'live', 'stream', 'streaming', 'valorant', 'val', 'gaming', 'channel',
  'link', 'here', 'now', 'guys', 'bro', 'pls', 'please', 'check', 'watch',
  'sub', 'follow', 'search', 'find', 'look', 'youtube', 'twitch', 'kick',
  'ttv', 'com', 'www', 'http', 'https', 'results', 'shorts', 'playlist',
  'videos', 'featured', 'about', 'streams',
]);

function handleFromStreamUrl(platform: Platform, url: string): string | null {
  const clean = url.split(/[?#]/)[0].replace(/\/+$/, '');
  const rest = clean.replace(/^https?:\/\//i, '');
  const parts = rest.split('/').slice(1);
  const first = parts[0] ?? '';
  const second = parts[1] ?? '';
  if (platform === 'YouTube') {
    if (first.startsWith('@') && first.length >= 3) return first.slice(1);
    if (['c', 'user', 'channel'].includes(first.toLowerCase()) && second) return second;
    return null;
  }
  const slug = first.replace(/^@/, '');
  if (slug.length >= 3 && !CHAT_STOP_WORDS.has(slug.toLowerCase())) {
    return slug;
  }
  return null;
}

export function extractChatStreamPlug(
  body: string,
  senderRiotId?: string
): ChatStreamPlugExtraction | null {
  const trimmed = body.trim();
  if (!trimmed) return null;

  const senderIgn = senderRiotId ? senderRiotId.split('#')[0] : 'Player';
  const cleanedSender = cleanIgn(senderIgn) || senderIgn;

  // 1. Direct YouTube / Twitch / Kick URL (with or without https://)
  const urlMatches = trimmed.match(
    /\b(?:https?:\/\/)?(?:www\.|m\.)?(?:youtube\.com|youtu\.be|twitch\.tv|kick\.com)\/[^\s<>"')\]]+/gi
  );
  if (urlMatches) {
    for (const raw of urlMatches) {
      const stripped = raw.replace(/[.,!?;:)\]]+$/, '');
      const fullUrl = /^https?:\/\//i.test(stripped) ? stripped : `https://${stripped}`;
      const host = hostOf(fullUrl);
      if (!host) continue;
      let platform: Platform | null = null;
      if (hostAllowed('YouTube', host)) platform = 'YouTube';
      else if (hostAllowed('Twitch', host)) platform = 'Twitch';
      else if (hostAllowed('Kick', host)) platform = 'Kick';
      if (!platform) continue;

      return {
        platform,
        url: fullUrl,
        channel: handleFromStreamUrl(platform, fullUrl) ?? cleanedSender,
        matchType: 'direct-url',
        rawMessage: trimmed,
      };
    }
  }

  // 2. Shorthand ("ttv/vexfps", "yt/@RohanGaming", "kick/kairo")
  const shorthands: [RegExp, Platform][] = [
    [/\b(?:yt|youtube)\s*(?:\/|:|@|-)\s*@?([a-z0-9._-]{3,30})\b/i, 'YouTube'],
    [/\b(?:ttv|twitch)\s*(?:\/|:|@|-)\s*([a-z0-9_]{3,25})\b/i, 'Twitch'],
    [/\bkick\s*(?:\/|:|@|-)\s*([a-z0-9_-]{3,25})\b/i, 'Kick'],
  ];
  for (const [re, platform] of shorthands) {
    const m = trimmed.match(re);
    if (m?.[1]) {
      const handle = m[1].replace(/^[.-]+|[.-]+$/g, '');
      if (handle.length >= 3 && !CHAT_STOP_WORDS.has(handle.toLowerCase())) {
        const url =
          platform === 'YouTube'
            ? `https://www.youtube.com/@${handle}`
            : platform === 'Twitch'
            ? `https://www.twitch.tv/${handle}`
            : `https://kick.com/${handle}`;
        return {
          platform,
          url,
          channel: handle,
          matchType: 'shorthand',
          rawMessage: trimmed,
        };
      }
    }
  }

  // 3. Live self-promo intent ("im live on yt", "streaming on twitch @handle")
  const hasLiveIntent =
    /\b(?:live|streaming|stream|watch|tune\s+in|sub|subscribe|follow|check\s+(?:out\s+)?my|going\s+live|im\s+live|i'm\s+live)\b/i.test(
      trimmed
    );
  if (hasLiveIntent) {
    let platform: Platform | null = null;
    if (/\b(?:yt|youtube)\b/i.test(trimmed)) platform = 'YouTube';
    else if (/\b(?:ttv|twitch)\b/i.test(trimmed)) platform = 'Twitch';
    else if (/\bkick\b/i.test(trimmed)) platform = 'Kick';

    if (platform) {
      const atMatch = trimmed.match(/(?:^|\s)@([a-z0-9._-]{3,30})\b/i);
      if (atMatch?.[1] && !CHAT_STOP_WORDS.has(atMatch[1].toLowerCase())) {
        const handle = atMatch[1].replace(/^[.-]+|[.-]+$/g, '');
        const url =
          platform === 'YouTube'
            ? `https://www.youtube.com/@${handle}`
            : platform === 'Twitch'
            ? `https://www.twitch.tv/${handle}`
            : `https://kick.com/${handle}`;
        return {
          platform,
          url,
          channel: handle,
          matchType: 'shorthand',
          rawMessage: trimmed,
        };
      }

      const searchUrl =
        platform === 'YouTube'
          ? `https://www.youtube.com/results?search_query=${encodeURIComponent(
              `${cleanedSender} Valorant`
            )}&sp=EgJAAQ%3D%3D`
          : platform === 'Twitch'
          ? `https://www.twitch.tv/search?term=${encodeURIComponent(cleanedSender)}`
          : `https://kick.com/search?query=${encodeURIComponent(cleanedSender)}`;
      return {
        platform,
        url: searchUrl,
        channel: cleanedSender,
        matchType: 'live-mention',
        rawMessage: trimmed,
      };
    }
  }

  return null;
}
