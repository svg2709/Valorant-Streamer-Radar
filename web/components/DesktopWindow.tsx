import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  AlertTriangle,
  ArrowUpCircle,
  Check,
  CheckCircle2,
  ChevronDown,
  Copy,
  Download,
  ExternalLink,
  Filter,
  Globe,
  Info,
  KeyRound,
  MessageSquareQuote,
  Minus,
  Pencil,
  Plus,
  Radio,
  RefreshCw,
  RotateCcw,
  Search,
  ShieldCheck,
  Trash2,
  Users,
  X,
  XCircle,
} from 'lucide-react';
import {
  APP_VERSION,
  AppConfig,
  COMMUNITY_REPO_NAME,
  COMMUNITY_REPO_OWNER,
  ConfidenceTier,
  DateRangeFilter,
  DesktopTab,
  DirectoryStreamer,
  LastMatchSummary,
  Platform,
  RecommendedStreamer,
  SERVER_REGIONS,
  ServerRegionCode,
  ServerRegionOverride,
  StatusFilter,
  StreamerEntry,
  getServerRegionInfo,
} from '../types/radar';
import { REGIONAL_RECOMMENDATIONS } from '../data/mockMatches';
import {
  cleanIgn,
  compareSemver,
  formatViewers,
  hostAllowed,
  hostOf,
  matchesHistoryFilters,
  resolveEffectiveRegion,
  resolveEntryTier,
} from '../utils/validation';
import { RadarIcon } from './RadarIcon';

interface DesktopWindowProps {
  visible: boolean;
  activeTab: DesktopTab;
  onChangeTab: (tab: DesktopTab) => void;
  status: string;
  isScanning: boolean;
  isSyncingDirectory: boolean;
  valorantRunning: boolean;
  detectedRegion: ServerRegionCode;
  config: AppConfig;
  history: StreamerEntry[];
  lastScannedMatch: LastMatchSummary | null;
  customDirectory: DirectoryStreamer[];
  communityDirectory: DirectoryStreamer[];
  toastMessage: string | null;
  onTriggerManualScan: () => void;
  onOpenAddModal: (editEntry?: DirectoryStreamer) => void;
  onOpenAddFromRecommendation: (rec: RecommendedStreamer) => void;
  onDeleteCustomEntry: (riotId: string, platform: Platform) => void;
  onSyncDirectory: () => void;
  onClearHistory: () => void;
  onCopyLink: (url: string) => void;
  onWatchStream: (entry: StreamerEntry) => void;
  onSaveConfig: (nextConfig: AppConfig) => void;
  onToggleRecommendationsImmediate: (enabled: boolean) => void;
  onHideToTray: () => void;
  onRestoreFromTray: () => void;
}

type UpdateState =
  | { status: 'idle' }
  | { status: 'checking' }
  | { status: 'up_to_date'; message: string; checkedTag: string }
  | {
      status: 'available';
      tagName: string;
      releaseName: string;
      changelog: string;
      downloadUrl: string;
      htmlUrl: string;
      sizeLabel: string;
      publishedAt: string;
      buildSha?: string;
      shaVerified: boolean;
    }
  | { status: 'error'; message: string };

function platformColor(platform: Platform): string {
  switch (platform) {
    case 'Twitch':
      return '#A770FF';
    case 'Kick':
      return '#53FC18';
    default:
      return '#FF5CA8';
  }
}

function tierBadgeStyle(tier: ConfidenceTier): {
  color: string;
  borderColor: string;
  backgroundColor: string;
} {
  if (tier === 'Confirmed') {
    return {
      color: '#22C55E',
      borderColor: '#22C55E66',
      backgroundColor: '#22C55E18',
    };
  }
  if (tier === 'Probable') {
    return {
      color: '#FB923C',
      borderColor: '#FB923C66',
      backgroundColor: '#FB923C18',
    };
  }
  return {
    color: '#98A2B8',
    borderColor: '#64748B66',
    backgroundColor: '#64748B1A',
  };
}

const QUICK_SERVER_CHIPS: { code: ServerRegionOverride; label: string }[] = [
  { code: 'auto', label: 'Auto' },
  { code: 'ap-mumbai', label: 'Mumbai' },
  { code: 'ap-tokyo', label: 'Tokyo' },
  { code: 'ap-manila', label: 'Manila' },
  { code: 'ap-singapore', label: 'Singapore' },
  { code: 'na-austin', label: 'Austin' },
  { code: 'na-us-east', label: 'US East' },
  { code: 'eu-frankfurt', label: 'Frankfurt' },
  { code: 'eu-london', label: 'London' },
  { code: 'kr-seoul', label: 'Seoul' },
];

const RECS_CACHE_TTL_MS = 5 * 60 * 1000;

interface ToggleSwitchProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  ariaLabel: string;
  showStateText?: boolean;
}

const ToggleSwitch: React.FC<ToggleSwitchProps> = ({
  checked,
  onChange,
  ariaLabel,
  showStateText = true,
}) => (
  <div className="inline-flex shrink-0 items-center gap-2">
    {showStateText && (
      <span
        className={`w-6 text-right font-mono text-[10.5px] font-bold tracking-wider ${
          checked ? 'text-[#FFA6D2]' : 'text-[#64748B]'
        }`}
      >
        {checked ? 'ON' : 'OFF'}
      </span>
    )}
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full border transition-colors duration-150 focus:outline-none focus:ring-2 focus:ring-[#FF5CA8]/50 ${
        checked
          ? 'border-[#FFA6D2]/80 bg-[#FF5CA8]'
          : 'border-[#273144] bg-[#1D2636] hover:border-[#FF5CA8]/50'
      }`}
    >
      <span
        className={`pointer-events-none inline-block h-[18px] w-[18px] transform rounded-full bg-[#FFF5FA] shadow transition-transform duration-150 ${
          checked ? 'translate-x-[22px]' : 'translate-x-[2px]'
        }`}
      />
    </button>
  </div>
);

interface ToggleSettingRowProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  title: string;
  description: React.ReactNode;
}

const ToggleSettingRow: React.FC<ToggleSettingRowProps> = ({
  checked,
  onChange,
  title,
  description,
}) => (
  <div
    className={`flex items-center justify-between gap-4 rounded-md border px-3.5 py-2.5 transition-colors ${
      checked
        ? 'border-[#FF5CA8]/35 bg-[#090D14]'
        : 'border-[#273144] bg-[#090D14]'
    }`}
  >
    <div className="min-w-0">
      <p className="text-xs font-semibold text-[#F3EFF4]">{title}</p>
      <p className="mt-0.5 text-[11px] leading-relaxed text-[#98A2B8]">
        {description}
      </p>
    </div>
    <ToggleSwitch checked={checked} onChange={onChange} ariaLabel={title} />
  </div>
);

export const DesktopWindow: React.FC<DesktopWindowProps> = ({
  visible,
  activeTab,
  onChangeTab,
  status,
  isScanning,
  isSyncingDirectory,
  valorantRunning,
  detectedRegion,
  config,
  history,
  lastScannedMatch,
  customDirectory,
  communityDirectory,
  toastMessage,
  onTriggerManualScan,
  onOpenAddModal,
  onOpenAddFromRecommendation,
  onDeleteCustomEntry,
  onSyncDirectory,
  onClearHistory,
  onCopyLink,
  onWatchStream,
  onSaveConfig,
  onToggleRecommendationsImmediate,
  onHideToTray,
  onRestoreFromTray,
}) => {
  // History multi-criteria filter state
  const [searchQuery, setSearchQuery] = useState('');
  const [teamFilter, setTeamFilter] = useState<'All' | 'Ally' | 'Enemy'>('All');
  const [matchTypeFilter, setMatchTypeFilter] = useState<string>('All');
  const [serverFilter, setServerFilter] = useState<string>('All');
  const [dateRangeFilter, setDateRangeFilter] = useState<DateRangeFilter>('all');
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');

  // Directory state
  const [dirSource, setDirSource] = useState<'custom' | 'community'>('custom');
  const [dirSearch, setDirSearch] = useState('');
  const [testIgn, setTestIgn] = useState('Desktop');
  const [testUrl, setTestUrl] = useState('https://evil.example.com@youtube.com/@fake');
  const [testPlatform, setTestPlatform] = useState<Platform>('YouTube');

  // Discover state (cached 5 min TTL, zero fetches when OFF)
  const [discoverPlatform, setDiscoverPlatform] = useState<'All' | 'Twitch' | 'YouTube'>('All');
  const [isRefreshingRecs, setIsRefreshingRecs] = useState(false);
  const [recsLastFetchedMs, setRecsLastFetchedMs] = useState<number | null>(null);
  const [recsUpdatedAt, setRecsUpdatedAt] = useState<string>('Not fetched yet');

  // Settings draft state
  const [draft, setDraft] = useState<AppConfig>(config);
  const [showSecrets, setShowSecrets] = useState(false);
  const [showApiGuide, setShowApiGuide] = useState(false);
  const [ytKeyStatus, setYtKeyStatus] = useState<{ kind: 'ok' | 'err'; msg: string } | null>(null);
  const [twitchKeyStatus, setTwitchKeyStatus] = useState<{ kind: 'ok' | 'err'; msg: string } | null>(null);

  // App Updater state (auto-hides banner when up_to_date, never treats error as up_to_date)
  const [updateState, setUpdateState] = useState<UpdateState>({ status: 'idle' });
  const [showChangelog, setShowChangelog] = useState(true);
  const startupCheckedRef = useRef(false);

  const handleCheckForUpdates = useCallback(async (silent = false) => {
    if (!silent) {
      setUpdateState({ status: 'checking' });
    }
    try {
      const res = await fetch(
        `https://api.github.com/repos/${COMMUNITY_REPO_OWNER}/${COMMUNITY_REPO_NAME}/releases/latest`,
        { headers: { Accept: 'application/vnd.github+json' } }
      );
      if (res.status === 404) {
        if (!silent) {
          setUpdateState({
            status: 'error',
            message: 'Release information is currently unavailable (no published GitHub release found).',
          });
        }
        return;
      }
      if (res.status === 403 || res.status === 429) {
        if (!silent) {
          setUpdateState({
            status: 'error',
            message: 'GitHub API rate limit reached. Open Releases directly or try again shortly.',
          });
        }
        return;
      }
      if (!res.ok) {
        if (!silent) {
          setUpdateState({
            status: 'error',
            message: `GitHub Releases check failed (HTTP ${res.status}).`,
          });
        }
        return;
      }
      const data = await res.json();
      const latestTag: string = String(data.tag_name || '').trim();
      if (!latestTag) {
        if (!silent) {
          setUpdateState({
            status: 'error',
            message: 'Latest GitHub release response did not include a version tag.',
          });
        }
        return;
      }

      // Compare installed version against latest GitHub release
      if (compareSemver(latestTag, APP_VERSION) <= 0) {
        // Automatically clears any prior update notification without manual dismissal
        setUpdateState({
          status: 'up_to_date',
          checkedTag: latestTag,
          message: `You are up to date (installed v${APP_VERSION}, latest release ${latestTag}).`,
        });
        return;
      }

      const assets: Array<{ name?: string; browser_download_url?: string; size?: number; digest?: string }> =
        Array.isArray(data.assets) ? data.assets : [];
      const exe = assets.find((a) => (a.name || '').toLowerCase().endsWith('.exe'));
      const shaAsset = assets.find((a) => (a.name || '').toLowerCase().endsWith('.sha256'));
      const bytes = Number(exe?.size || 0);
      const sizeLabel =
        bytes >= 1024 * 1024
          ? `${(bytes / (1024 * 1024)).toFixed(1)} MB`
          : bytes >= 1024
          ? `${Math.round(bytes / 1024)} KB`
          : '';
      const rawBody = typeof data.body === 'string' ? data.body.trim() : '';
      const shaMatch = rawBody.match(/commit\s+`?([0-9a-f]{7,40})`?/i);
      const htmlUrl =
        data.html_url ||
        `https://github.com/${COMMUNITY_REPO_OWNER}/${COMMUNITY_REPO_NAME}/releases`;

      setUpdateState({
        status: 'available',
        tagName: latestTag,
        releaseName: data.name || latestTag,
        changelog:
          rawBody ||
          `Release ${latestTag} is available on GitHub Releases. Open the release page to view full notes.`,
        downloadUrl: exe?.browser_download_url || htmlUrl,
        htmlUrl,
        sizeLabel,
        publishedAt: typeof data.published_at === 'string' ? data.published_at.slice(0, 10) : '',
        buildSha: shaMatch ? shaMatch[1].slice(0, 7) : undefined,
        shaVerified: Boolean(shaAsset || exe?.digest),
      });
      setShowChangelog(true);
    } catch {
      if (!silent) {
        setUpdateState({
          status: 'error',
          message: 'Could not reach GitHub API. Check your internet connection or open Releases.',
        });
      }
    }
  }, []);

  // Silent startup update check (runs once; no duplicate notifications)
  useEffect(() => {
    if (!startupCheckedRef.current) {
      startupCheckedRef.current = true;
      void handleCheckForUpdates(true);
    }
  }, [handleCheckForUpdates]);

  useEffect(() => {
    setDraft(config);
  }, [config]);

  const effectiveRegion = resolveEffectiveRegion(config, detectedRegion);
  const detectedRegionInfo = getServerRegionInfo(detectedRegion);

  const handleRefreshRecs = useCallback(
    (force = false) => {
      if (!config.enable_recommendations && !force) return;
      const now = Date.now();
      if (!force && recsLastFetchedMs && now - recsLastFetchedMs < RECS_CACHE_TTL_MS) {
        return;
      }
      setIsRefreshingRecs(true);
      window.setTimeout(() => {
        setIsRefreshingRecs(false);
        const fetchedAt = Date.now();
        setRecsLastFetchedMs(fetchedAt);
        setRecsUpdatedAt(
          new Date(fetchedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
        );
      }, 450);
    },
    [config.enable_recommendations, recsLastFetchedMs]
  );

  // Only populate recommendations when explicitly enabled and Discover tab is viewed
  useEffect(() => {
    if (activeTab === 'discover' && config.enable_recommendations && !recsLastFetchedMs) {
      handleRefreshRecs(true);
    }
  }, [activeTab, config.enable_recommendations, recsLastFetchedMs, handleRefreshRecs]);

  const handleTestYoutubeKey = () => {
    const key = draft.youtube_api_key.trim();
    if (!key) {
      setYtKeyStatus({
        kind: 'err',
        msg: 'Paste a YouTube Data API v3 key first (or leave blank to run keyless).',
      });
      return;
    }
    if (!/^AIza[0-9A-Za-z_-]{20,}$/.test(key)) {
      setYtKeyStatus({
        kind: 'err',
        msg: 'Key format looks unusual — Google Cloud YouTube keys normally start with "AIza".',
      });
      return;
    }
    setYtKeyStatus({
      kind: 'ok',
      msg: 'Key format looks valid (in the Windows app, Test Key runs a live Google API v3 check).',
    });
  };

  const handleTestTwitchKeys = () => {
    const cid = draft.twitch_client_id.trim();
    const sec = draft.twitch_client_secret.trim();
    if (!cid || !sec) {
      setTwitchKeyStatus({
        kind: 'err',
        msg: 'Enter both Twitch Client ID and Client Secret first.',
      });
      return;
    }
    if (cid.length < 16 || sec.length < 16) {
      setTwitchKeyStatus({
        kind: 'err',
        msg: 'Twitch Client ID / Secret look too short (expected 30-character alphanumeric tokens).',
      });
      return;
    }
    setTwitchKeyStatus({
      kind: 'ok',
      msg: 'Credentials format looks valid (in the Windows app, Test Twitch verifies live OAuth).',
    });
  };

  const availableMatchTypes = useMemo(() => {
    const base = new Set(['Competitive', 'Unrated', 'Swiftplay', 'Spike Rush', 'Deathmatch', 'Premier']);
    for (const e of history) {
      if (e.queue_name) base.add(e.queue_name);
    }
    return ['All', ...Array.from(base)];
  }, [history]);

  const availableServers = useMemo(() => {
    const base = new Set(['Mumbai', 'Tokyo', 'Manila', 'Austin', 'Singapore', 'Frankfurt', 'London', 'Seoul']);
    for (const e of history) {
      if (e.server_region) base.add(e.server_region);
    }
    return ['All', ...Array.from(base)];
  }, [history]);

  const hasActiveFilters =
    searchQuery.trim() !== '' ||
    teamFilter !== 'All' ||
    matchTypeFilter !== 'All' ||
    serverFilter !== 'All' ||
    dateRangeFilter !== 'all' ||
    statusFilter !== 'all';

  const resetHistoryFilters = () => {
    setSearchQuery('');
    setTeamFilter('All');
    setMatchTypeFilter('All');
    setServerFilter('All');
    setDateRangeFilter('all');
    setStatusFilter('all');
  };

  const filteredHistory = useMemo(
    () =>
      history.filter((e) =>
        matchesHistoryFilters(e, {
          searchQuery,
          teamFilter,
          matchTypeFilter,
          serverFilter,
          dateRangeFilter,
          statusFilter,
        })
      ),
    [history, searchQuery, teamFilter, matchTypeFilter, serverFilter, dateRangeFilter, statusFilter]
  );

  // Split into Latest Match vs Previous History (Phase 5)
  const latestMatchId = lastScannedMatch?.match_id ?? history[0]?.match_id ?? null;

  const latestMatchEntries = useMemo(
    () =>
      latestMatchId
        ? filteredHistory.filter((e) => e.match_id === latestMatchId)
        : [],
    [filteredHistory, latestMatchId]
  );

  const previousHistoryEntries = useMemo(
    () =>
      latestMatchId
        ? filteredHistory.filter((e) => e.match_id !== latestMatchId)
        : filteredHistory,
    [filteredHistory, latestMatchId]
  );

  const filteredDirectory = useMemo(() => {
    const list = dirSource === 'custom' ? customDirectory : communityDirectory;
    const q = dirSearch.trim().toLowerCase();
    if (!q) return list;
    return list.filter(
      (d) =>
        d.riot_id.toLowerCase().includes(q) ||
        (d.aliases?.some((a) => a.toLowerCase().includes(q)) ?? false) ||
        d.channel_name.toLowerCase().includes(q) ||
        d.platform.toLowerCase().includes(q) ||
        (d.server_region?.toLowerCase().includes(q) ?? false)
    );
  }, [dirSource, customDirectory, communityDirectory, dirSearch]);

  const visibleRecommendations = useMemo(() => {
    if (!config.enable_recommendations) return [];
    const regional = REGIONAL_RECOMMENDATIONS.filter(
      (r) => r.server_region === effectiveRegion.code
    );
    const base =
      regional.length > 0
        ? regional
        : REGIONAL_RECOMMENDATIONS.filter(
            (r) => getServerRegionInfo(r.server_region).apiRegion === effectiveRegion.apiRegion
          );
    const pool = base.length > 0 ? base : REGIONAL_RECOMMENDATIONS.slice(0, 6);
    return pool.filter(
      (r) => discoverPlatform === 'All' || r.platform === discoverPlatform
    );
  }, [config.enable_recommendations, effectiveRegion, discoverPlatform]);

  const cleanedIgnResult = cleanIgn(testIgn);
  const testedHost = hostOf(testUrl.trim());
  const testedHostOk = testedHost ? hostAllowed(testPlatform, testedHost) : false;

  const handleSaveSettings = () => {
    onSaveConfig({
      ...draft,
      youtube_api_key: draft.youtube_api_key.trim(),
      twitch_client_id: draft.twitch_client_id.trim(),
      twitch_client_secret: draft.twitch_client_secret.trim(),
    });
  };

  const renderHistoryCard = (entry: StreamerEntry) => {
    const pColor = platformColor(entry.platform);
    const tier = resolveEntryTier(entry);
    const tierStyle = tierBadgeStyle(tier);
    const metaItems = [
      entry.map_name,
      entry.agent_name,
      entry.queue_name,
      entry.timestamp,
    ].filter(Boolean);

    return (
      <article
        key={`${entry.match_id}-${entry.riot_id}-${entry.platform}`}
        className={`rounded-lg border bg-[#141B27] px-4 py-3.5 ${
          entry.chat_message
            ? 'border-[#FF5CA8]/45'
            : tier === 'Unverified'
            ? 'border-[#273144]/80'
            : 'border-[#273144]'
        }`}
      >
        <div className="flex flex-col justify-between gap-3 sm:flex-row sm:items-center">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span className="font-mono text-[15px] font-bold text-[#FFF5FA]">
                {entry.riot_id}
              </span>
              <span
                className="rounded border px-1.5 py-0.5 text-[10.5px] font-semibold"
                style={{
                  color: pColor,
                  borderColor: `${pColor}66`,
                  backgroundColor: `${pColor}1A`,
                }}
              >
                {entry.platform}
              </span>
              <span className="text-xs font-semibold" style={{ color: pColor }}>
                {entry.channel}
              </span>
              {entry.team_relation && (
                <>
                  <span className="text-[#273144]" aria-hidden="true">
                    ·
                  </span>
                  <span
                    className="text-xs font-semibold"
                    style={{
                      color: entry.team_relation === 'Ally' ? '#2DD4BF' : '#FB923C',
                    }}
                  >
                    {entry.team_relation}
                  </span>
                </>
              )}
              <span
                className="rounded border px-1.5 py-0.5 font-mono text-[10px] font-semibold tabular-nums"
                style={tierStyle}
              >
                {tier}
                {entry.confidence ? ` · ${entry.confidence}%` : ''}
              </span>
              {entry.server_region && (
                <span className="rounded border border-[#273144] bg-[#1D2636] px-1.5 py-0.5 text-[10px] font-medium text-[#2DD4BF]">
                  {entry.server_region}
                </span>
              )}
              {entry.chat_message && (
                <span className="inline-flex items-center gap-1 rounded border border-[#FFA6D2]/60 bg-[#FF5CA8]/15 px-1.5 py-0.5 text-[10.5px] font-semibold text-[#FFF5FA]">
                  <MessageSquareQuote className="h-3 w-3 text-[#FFA6D2]" />
                  In-Game Chat
                </span>
              )}
            </div>

            <p className="mt-1 truncate text-[13px] text-[#F3EFF4]">{entry.title}</p>

            {(entry.server_match_status || entry.match_reason) && (
              <p className="mt-0.5 text-[11px] text-[#98A2B8]">
                {entry.server_match_status && (
                  <span className="font-medium text-[#CBD5E1]">
                    {entry.server_match_status}
                  </span>
                )}
                {entry.server_match_status && entry.match_reason ? ' — ' : ''}
                {entry.match_reason}
              </p>
            )}

            {entry.chat_message && (
              <div className="mt-1.5 rounded border border-[#FF5CA8]/35 bg-[#090D14] px-2.5 py-1.5 text-[11.5px] text-[#FFA6D2]">
                <span className="font-semibold text-[#FFF5FA]">Chat:</span>{' '}
                &ldquo;{entry.chat_message}&rdquo;
              </div>
            )}

            <p className="mt-1.5 font-mono text-[11px] text-[#98A2B8] tabular-nums">
              {metaItems.join('  ·  ')}
            </p>
          </div>

          <div className="flex shrink-0 items-center gap-2">
            <button
              type="button"
              onClick={() => onCopyLink(entry.url)}
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[#273144] bg-[#1D2636] px-3 text-xs font-medium text-[#F3EFF4] transition-colors hover:border-[#FF5CA8]/50 hover:bg-[#253044] whitespace-nowrap"
            >
              <Copy className="h-3.5 w-3.5" />
              Copy Link
            </button>
            <button
              type="button"
              onClick={() => onWatchStream(entry)}
              title={entry.url}
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3.5 text-xs font-bold text-[#0D111A] transition-opacity whitespace-nowrap"
            >
              Watch Stream
              <ExternalLink className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>

        {/* Ranked Most Likely Streamers List */}
        {entry.candidates && entry.candidates.length > 1 && (
          <div className="mt-3 rounded-md border border-[#273144] bg-[#090D14] px-3 py-2.5">
            <p className="text-[10px] font-bold tracking-wider text-[#98A2B8] uppercase">
              Ranked Candidates for {entry.riot_id} ({entry.candidates.length} matches)
            </p>
            <div className="mt-2 space-y-1.5">
              {entry.candidates.map((cand, idx) => {
                const cPlatColor = platformColor(cand.platform);
                const cTier = cand.confidence_tier ?? (cand.confidence >= 85 ? 'Confirmed' : cand.confidence >= 65 ? 'Probable' : 'Unverified');
                const cStyle = tierBadgeStyle(cTier);
                return (
                  <div
                    key={`${cand.url}-${idx}`}
                    className="flex flex-wrap items-center justify-between gap-2 text-[11.5px]"
                  >
                    <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
                      <span
                        className={`font-mono font-bold ${
                          idx === 0 ? 'text-[#FFA6D2]' : 'text-[#64748B]'
                        }`}
                      >
                        #{idx + 1}
                      </span>
                      <span
                        className="font-mono font-semibold tabular-nums"
                        style={{ color: cStyle.color }}
                      >
                        {cTier} ({cand.confidence}%)
                      </span>
                      <span className="font-semibold" style={{ color: cPlatColor }}>
                        {cand.platform}
                      </span>
                      <span className="font-semibold text-[#F3EFF4]">{cand.channel}</span>
                      {cand.region_hint && (
                        <span className="text-[10.5px] text-[#2DD4BF]">
                          [{cand.region_hint}]
                        </span>
                      )}
                      <span className="truncate text-[10.5px] text-[#98A2B8]">
                        — {cand.match_reason}
                      </span>
                    </div>
                    <a
                      href={cand.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex shrink-0 items-center gap-1 rounded border border-[#273144] bg-[#1D2636] px-2 py-0.5 text-[10.5px] font-medium text-[#FFF5FA] hover:border-[#FF5CA8]/60"
                    >
                      Open
                      <ExternalLink className="h-2.5 w-2.5" />
                    </a>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </article>
    );
  };

  if (!visible) {
    return (
      <div className="flex min-h-[540px] flex-col items-center justify-center rounded-lg border border-dashed border-[#273144] bg-[#0D111A]/80 px-6 py-16 text-center">
        <RadarIcon size={42} />
        <h3 className="mt-4 text-base font-semibold text-[#FFF5FA]">
          Window Hidden in Windows System Tray
        </h3>
        <p className="mt-1.5 max-w-md text-xs leading-relaxed text-[#98A2B8]">
          Closing the window intercepts the close event and sets{' '}
          <span className="font-mono text-[#F3EFF4]">WINDOW_VISIBLE = false</span>.
          The UI loop schedules zero repaints while the background{' '}
          <span className="font-mono text-[#F3EFF4]">radar-bg</span> thread continues
          tailing <span className="font-mono text-[#F3EFF4]">ShooterGame.log</span>.
        </p>
        <button
          type="button"
          onClick={onRestoreFromTray}
          className="mt-5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-4 py-2 text-xs font-bold text-[#0D111A] transition-opacity"
        >
          Open Streamer History (Left-Click Tray)
        </button>
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-lg border border-[#273144] bg-[#0D111A] shadow-2xl">
      {/* Pink-to-White Top Gradient Accent Strip */}
      <div className="h-[3px] w-full bg-pink-white-strip" />

      {/* Native Windows Title Bar */}
      <div className="flex items-center justify-between border-b border-[#273144] bg-[#070A10] px-3.5 py-2 select-none">
        <div className="flex items-center gap-2">
          <RadarIcon size={16} />
          <span className="text-xs font-medium text-[#F3EFF4]">
            Valorant Streamer Radar
          </span>
          <span className="font-mono text-[10.5px] text-[#64748B]">v{APP_VERSION}</span>
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={onHideToTray}
            title="Minimize to tray"
            className="rounded p-1 text-[#98A2B8] transition-colors hover:bg-[#141B27] hover:text-white"
          >
            <Minus className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            onClick={onHideToTray}
            title="Close (intercepted -> hide to system tray)"
            className="rounded p-1 text-[#98A2B8] transition-colors hover:bg-[#FF5CA8] hover:text-[#0D111A]"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      {/* Header Panel (#090D14) */}
      <div className="border-b border-[#273144] bg-[#090D14] px-6 py-4">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="min-w-0">
            <div className="flex items-baseline gap-2">
              <span
                className="text-2xl font-bold tracking-wide text-pink-white-gradient"
                style={{ fontFamily: 'var(--font-display)' }}
              >
                VALORANT STREAMER
              </span>
              <span
                className="text-2xl font-bold tracking-wide text-[#FFF5FA]"
                style={{ fontFamily: 'var(--font-display)' }}
              >
                RADAR
              </span>
            </div>
            <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-[#98A2B8]">
              <span className="inline-flex items-center gap-1.5">
                <span
                  className="inline-block h-2 w-2 rounded-full"
                  style={{
                    backgroundColor: valorantRunning ? '#22C55E' : '#64748B',
                  }}
                />
                <span>
                  {valorantRunning
                    ? `Valorant Active (${effectiveRegion.label})`
                    : `Waiting for Valorant (${effectiveRegion.label})`}
                </span>
              </span>
              <span className="text-[#273144]">|</span>
              <span className="truncate">{status}</span>
            </div>
          </div>

          <div className="flex shrink-0 items-center gap-2">
            <button
              type="button"
              onClick={() => onOpenAddModal()}
              className="inline-flex h-9 items-center gap-1.5 rounded-md border border-[#FF5CA8]/40 bg-[#1D2636] px-3.5 text-xs font-semibold text-[#FFF5FA] transition-colors hover:border-[#FF5CA8] hover:bg-[#253044] whitespace-nowrap"
            >
              <Plus className="h-3.5 w-3.5 text-[#FFA6D2]" />
              Add Streamer
            </button>
            <button
              type="button"
              disabled={isScanning}
              onClick={onTriggerManualScan}
              className={`inline-flex h-9 min-w-[142px] items-center justify-center rounded-md px-4 text-xs font-bold transition-all whitespace-nowrap ${
                isScanning
                  ? 'cursor-not-allowed bg-[#273144] text-[#98A2B8]'
                  : 'bg-pink-white-gradient bg-pink-white-gradient-hover text-[#0D111A] shadow-sm shadow-[#FF5CA8]/20'
              }`}
            >
              {isScanning ? 'Scanning...' : 'Check Last Match'}
            </button>
          </div>
        </div>

        {/* Desktop App Tab Switcher + Auto-Expiring Toast */}
        <div className="mt-3.5 flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-center gap-1.5">
            {(
              [
                ['history', `History (${history.length})`],
                [
                  'directory',
                  `Directory (${customDirectory.length + communityDirectory.length})`,
                ],
                [
                  'discover',
                  config.enable_recommendations ? 'Discover (Live)' : 'Discover (Off)',
                ],
                ['settings', 'Settings'],
              ] as [DesktopTab, string][]
            ).map(([tabKey, label]) => {
              const active = activeTab === tabKey;
              return (
                <button
                  key={tabKey}
                  type="button"
                  onClick={() => onChangeTab(tabKey)}
                  className={`rounded-[5px] border px-3 py-1.5 text-xs font-semibold transition-colors whitespace-nowrap tabular-nums ${
                    active
                      ? 'border-[#FF5CA8] bg-[#1D2636] text-[#FFF5FA]'
                      : 'border-transparent text-[#98A2B8] hover:text-white'
                  }`}
                >
                  {label}
                </button>
              );
            })}
          </div>

          <div className="flex items-center gap-3">
            {toastMessage && (
              <span className="inline-flex items-center gap-1.5 text-xs font-medium text-[#FFA6D2]">
                <Check className="h-3.5 w-3.5 text-[#FF5CA8]" />
                {toastMessage}
              </span>
            )}
            <button
              type="button"
              disabled={updateState.status === 'checking'}
              onClick={() => handleCheckForUpdates(false)}
              className={`inline-flex h-7 items-center gap-1.5 rounded-[5px] border px-2.5 text-[11.5px] font-semibold transition-colors whitespace-nowrap ${
                updateState.status === 'available'
                  ? 'border-[#FF5CA8] bg-[#FF5CA8]/20 text-[#FFF5FA]'
                  : 'border-[#273144] bg-[#1D2636] text-[#98A2B8] hover:border-[#FF5CA8]/60 hover:text-[#FFF5FA]'
              } disabled:opacity-60`}
            >
              <RefreshCw
                className={`h-3 w-3 ${updateState.status === 'checking' ? 'animate-spin text-[#FFA6D2]' : ''}`}
              />
              {updateState.status === 'checking'
                ? 'Checking...'
                : updateState.status === 'available'
                ? `Update ${updateState.tagName}`
                : 'Check for Updates'}
            </button>
          </div>
        </div>
      </div>

      {/* Main Panel (#0D111A) */}
      <div className="min-h-[460px] max-h-[680px] overflow-y-auto px-6 py-4">
        {/* Global Update Banner — automatically visible ONLY when a newer release exists, auto-hidden when up to date */}
        {updateState.status === 'available' && (
          <div className="mb-4 rounded-lg border border-[#FF5CA8] bg-[#141B27] px-4 py-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="inline-flex items-center gap-1 text-[10.5px] font-bold tracking-wider text-[#FF5CA8]">
                    <ArrowUpCircle className="h-3.5 w-3.5" />
                    UPDATE AVAILABLE
                  </span>
                  <span className="text-xs font-bold text-[#FFF5FA]">
                    {updateState.releaseName}
                  </span>
                  <span className="font-mono text-[11px] text-[#98A2B8]">
                    (Installed: v{APP_VERSION} &rarr; Latest: {updateState.tagName})
                  </span>
                </div>
                <p className="mt-0.5 font-mono text-[11px] text-[#98A2B8]">
                  {[
                    updateState.buildSha ? `Build ${updateState.buildSha}` : '',
                    updateState.sizeLabel,
                    updateState.publishedAt,
                    updateState.shaVerified ? 'SHA-256 verified' : '',
                  ]
                    .filter(Boolean)
                    .join('  |  ')}
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  onClick={() => setShowChangelog((prev) => !prev)}
                  className="inline-flex h-7 items-center gap-1 rounded-md border border-[#273144] bg-[#1D2636] px-2.5 text-[11.5px] font-medium text-[#F3EFF4] hover:border-[#FF5CA8]/50 whitespace-nowrap"
                >
                  {showChangelog ? 'Hide Changelog' : 'View Changelog'}
                  <ChevronDown
                    className={`h-3 w-3 transition-transform ${showChangelog ? 'rotate-180' : ''}`}
                  />
                </button>
                <a
                  href={updateState.downloadUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex h-7 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3 text-xs font-bold text-[#0D111A] whitespace-nowrap"
                >
                  <Download className="h-3.5 w-3.5" />
                  Download &amp; Install
                </a>
                <a
                  href={updateState.htmlUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex h-7 items-center rounded-md border border-[#273144] bg-[#1D2636] px-2.5 text-[11.5px] font-medium text-[#F3EFF4] hover:border-[#FF5CA8]/50 whitespace-nowrap"
                >
                  Releases
                </a>
              </div>
            </div>

            {showChangelog && (
              <div className="mt-2.5 max-h-36 overflow-y-auto rounded border border-[#273144] bg-[#090D14] p-2.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-[#CBD5E1]">
                {updateState.changelog}
              </div>
            )}
          </div>
        )}

        {/* Explicit error banner when a manual update check fails (never misreported as up to date) */}
        {updateState.status === 'error' && activeTab !== 'settings' && (
          <div className="mb-3.5 flex items-center justify-between gap-3 rounded-lg border border-[#FF668A]/60 bg-[#141B27] px-3.5 py-2 text-xs text-[#FF668A]">
            <span className="inline-flex items-center gap-1.5">
              <XCircle className="h-3.5 w-3.5 shrink-0" />
              {updateState.message}
            </span>
            <div className="flex items-center gap-2">
              <a
                href={`https://github.com/${COMMUNITY_REPO_OWNER}/${COMMUNITY_REPO_NAME}/releases`}
                target="_blank"
                rel="noreferrer"
                className="text-[11px] font-semibold text-[#FFA6D2] hover:underline"
              >
                Releases
              </a>
              <button
                type="button"
                onClick={() => setUpdateState({ status: 'idle' })}
                className="text-[11px] text-[#98A2B8] hover:text-white"
              >
                Close
              </button>
            </div>
          </div>
        )}

        {/* TAB 1: HISTORY (Latest Match Subsection + Multi-Criteria Filtering + Previous History) */}
        {activeTab === 'history' && (
          <div className="space-y-4">
            {/* Multi-Criteria Non-Destructive Filter Bar */}
            <div className="rounded-lg border border-[#273144] bg-[#090D14] p-3">
              <div className="flex flex-wrap items-center justify-between gap-2.5">
                <div className="flex flex-1 flex-wrap items-center gap-2">
                  <div className="relative min-w-[210px] flex-1 sm:max-w-[280px]">
                    <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[#64748B]" />
                    <input
                      type="text"
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      placeholder="Search Riot ID, channel, map, server..."
                      className="w-full rounded border border-[#273144] bg-[#141B27] py-1.5 pr-3 pl-8 text-xs text-white placeholder-[#64748B] focus:border-[#FF5CA8] focus:outline-none"
                    />
                  </div>

                  <div className="flex items-center rounded-md border border-[#273144] bg-[#141B27] p-0.5">
                    {(['All', 'Ally', 'Enemy'] as const).map((f) => (
                      <button
                        key={f}
                        type="button"
                        onClick={() => setTeamFilter(f)}
                        className={`rounded px-2.5 py-1 text-xs font-medium transition-colors whitespace-nowrap ${
                          teamFilter === f
                            ? 'bg-[#253044] text-[#FFF5FA]'
                            : 'text-[#98A2B8] hover:text-white'
                        }`}
                      >
                        {f}
                      </button>
                    ))}
                  </div>
                </div>

                <div className="flex items-center gap-2">
                  <span className="font-mono text-[11px] text-[#98A2B8] tabular-nums">
                    {filteredHistory.length} / {history.length} shown
                  </span>
                  {hasActiveFilters && (
                    <button
                      type="button"
                      onClick={resetHistoryFilters}
                      className="inline-flex items-center gap-1 rounded border border-[#FF5CA8]/50 bg-[#1D2636] px-2.5 py-1 text-[11px] font-semibold text-[#FFF5FA] hover:bg-[#253044] whitespace-nowrap"
                    >
                      <RotateCcw className="h-3 w-3 text-[#FFA6D2]" />
                      Reset Filters
                    </button>
                  )}
                  {history.length > 0 && (
                    <button
                      type="button"
                      onClick={onClearHistory}
                      className="rounded border border-[#273144] px-2.5 py-1 text-[11px] text-[#98A2B8] transition-colors hover:border-[#FF5CA8] hover:text-white whitespace-nowrap"
                    >
                      Clear History
                    </button>
                  )}
                </div>
              </div>

              <div className="mt-2.5 flex flex-wrap items-center gap-2 border-t border-[#273144]/70 pt-2.5">
                <span className="inline-flex items-center gap-1 text-[10.5px] font-semibold tracking-wider text-[#64748B] uppercase">
                  <Filter className="h-3 w-3" />
                  Filters
                </span>

                <select
                  aria-label="Filter by match type"
                  value={matchTypeFilter}
                  onChange={(e) => setMatchTypeFilter(e.target.value)}
                  className="rounded border border-[#273144] bg-[#141B27] px-2.5 py-1 text-[11.5px] text-[#F3EFF4] focus:border-[#FF5CA8] focus:outline-none"
                >
                  {availableMatchTypes.map((qt) => (
                    <option key={qt} value={qt}>
                      {qt === 'All' ? 'Match Type: All' : qt}
                    </option>
                  ))}
                </select>

                <select
                  aria-label="Filter by server"
                  value={serverFilter}
                  onChange={(e) => setServerFilter(e.target.value)}
                  className="rounded border border-[#273144] bg-[#141B27] px-2.5 py-1 text-[11.5px] text-[#F3EFF4] focus:border-[#FF5CA8] focus:outline-none"
                >
                  {availableServers.map((srv) => (
                    <option key={srv} value={srv}>
                      {srv === 'All' ? 'Server: All' : srv}
                    </option>
                  ))}
                </select>

                <select
                  aria-label="Filter by date range"
                  value={dateRangeFilter}
                  onChange={(e) => setDateRangeFilter(e.target.value as DateRangeFilter)}
                  className="rounded border border-[#273144] bg-[#141B27] px-2.5 py-1 text-[11.5px] text-[#F3EFF4] focus:border-[#FF5CA8] focus:outline-none"
                >
                  <option value="all">Date: All Time</option>
                  <option value="today">Date: Today</option>
                  <option value="7d">Date: Last 7 Days</option>
                  <option value="30d">Date: Last 30 Days</option>
                </select>

                <select
                  aria-label="Filter by detection status"
                  value={statusFilter}
                  onChange={(e) => setStatusFilter(e.target.value as StatusFilter)}
                  className="rounded border border-[#273144] bg-[#141B27] px-2.5 py-1 text-[11.5px] text-[#F3EFF4] focus:border-[#FF5CA8] focus:outline-none"
                >
                  <option value="all">Status: All</option>
                  <option value="Confirmed">Status: Confirmed</option>
                  <option value="Probable">Status: Probable</option>
                  <option value="Unverified">Status: Unverified</option>
                  <option value="Chat">Status: In-Game Chat</option>
                </select>
              </div>
            </div>

            {history.length === 0 && !lastScannedMatch ? (
              <div className="flex flex-col items-center justify-center py-16 text-center">
                <h3 className="text-base font-bold text-[#F3EFF4]">
                  No Streamers Recorded Yet
                </h3>
                <p className="mt-1.5 max-w-md text-xs text-[#98A2B8]">
                  Play a match on any supported server, run a simulated match in Match Lab,
                  or click &ldquo;Add Streamer&rdquo; to register a Riot ID and channel.
                </p>
              </div>
            ) : filteredHistory.length === 0 && hasActiveFilters ? (
              <div className="rounded-lg border border-[#273144] bg-[#141B27]/50 py-14 text-center">
                <p className="text-sm font-semibold text-[#FFF5FA]">
                  No history records match your active filters
                </p>
                <p className="mt-1 text-xs text-[#98A2B8]">
                  Your saved match history ({history.length} records) is intact. Adjust or reset filters to view all entries.
                </p>
                <button
                  type="button"
                  onClick={resetHistoryFilters}
                  className="mt-3.5 inline-flex items-center gap-1.5 rounded-md border border-[#FF5CA8]/60 bg-[#1D2636] px-3.5 py-1.5 text-xs font-semibold text-[#FFF5FA] hover:bg-[#253044]"
                >
                  <RotateCcw className="h-3.5 w-3.5 text-[#FFA6D2]" />
                  Reset Filters
                </button>
              </div>
            ) : (
              <div className="space-y-5">
                {/* SUBSECTION 1: LATEST MATCH */}
                {lastScannedMatch && (
                  <section aria-label="Latest Match">
                    <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <span className="inline-block h-2 w-2 rounded-full bg-[#FF5CA8]" />
                        <h3 className="text-[11px] font-bold tracking-wider text-[#FFA6D2] uppercase">
                          Latest Match
                        </h3>
                      </div>
                      <span className="font-mono text-[11px] text-[#98A2B8] tabular-nums">
                        {lastScannedMatch.scanned_at}
                      </span>
                    </div>

                    <div className="mb-2.5 flex flex-wrap items-center gap-x-2.5 gap-y-1.5 rounded-md border border-[#FF5CA8]/35 bg-[#090D14] px-3.5 py-2.5 text-xs">
                      <span
                        className="rounded border px-1.5 py-0.5 text-[11px] font-semibold"
                        style={{
                          color:
                            lastScannedMatch.queue_name === 'Competitive' ||
                            lastScannedMatch.queue_name === 'Premier'
                              ? '#FF5CA8'
                              : lastScannedMatch.queue_name.includes('Deathmatch')
                              ? '#2DD4BF'
                              : lastScannedMatch.queue_name === 'Swiftplay' ||
                                lastScannedMatch.queue_name === 'Spike Rush'
                              ? '#FB923C'
                              : '#FFA6D2',
                          borderColor:
                            lastScannedMatch.queue_name === 'Competitive' ||
                            lastScannedMatch.queue_name === 'Premier'
                              ? '#FF5CA8'
                              : lastScannedMatch.queue_name.includes('Deathmatch')
                              ? '#2DD4BF'
                              : lastScannedMatch.queue_name === 'Swiftplay' ||
                                lastScannedMatch.queue_name === 'Spike Rush'
                              ? '#FB923C'
                              : '#273144',
                        }}
                      >
                        {lastScannedMatch.queue_name}
                      </span>
                      <span className="font-semibold text-[#FFF5FA]">
                        {lastScannedMatch.map_name}
                      </span>
                      {lastScannedMatch.server_name && (
                        <>
                          <span className="text-[#273144]">|</span>
                          <span className="font-medium text-[#2DD4BF]">
                            {lastScannedMatch.server_name}
                          </span>
                        </>
                      )}
                      {lastScannedMatch.my_agent && (
                        <>
                          <span className="text-[#273144]">|</span>
                          <span className="text-[#F3EFF4]">
                            Agent: {lastScannedMatch.my_agent}
                          </span>
                        </>
                      )}
                      <span className="text-[#273144]">|</span>
                      <span className="font-mono text-[#98A2B8] tabular-nums">
                        {lastScannedMatch.players_checked} scanned
                      </span>
                      {lastScannedMatch.party_skipped > 0 && (
                        <>
                          <span className="text-[#273144]">|</span>
                          <span className="inline-flex items-center gap-1 font-mono text-[11px] text-[#38BDF8] tabular-nums">
                            <Users className="h-3 w-3" />
                            {lastScannedMatch.party_skipped} party skipped
                          </span>
                        </>
                      )}
                      <span className="text-[#273144]">|</span>
                      <span
                        className={
                          lastScannedMatch.streamers_found > 0
                            ? 'font-semibold text-[#22C55E]'
                            : 'text-[#98A2B8]'
                        }
                      >
                        {lastScannedMatch.streamers_found > 0
                          ? `${lastScannedMatch.streamers_found} detected`
                          : '0 detected'}
                      </span>
                    </div>

                    {latestMatchEntries.length > 0 ? (
                      <div className="space-y-2.5">
                        {latestMatchEntries.map((entry) => renderHistoryCard(entry))}
                      </div>
                    ) : (
                      <div className="rounded-md border border-[#273144] bg-[#141B27]/50 px-4 py-3 text-xs text-[#98A2B8]">
                        {hasActiveFilters
                          ? 'Latest match entries are hidden by your active filters.'
                          : 'No live streamers were detected in your latest match.'}
                      </div>
                    )}
                  </section>
                )}

                {/* VISUAL DIVIDER + SUBSECTION 2: PREVIOUS HISTORY */}
                {previousHistoryEntries.length > 0 && (
                  <section aria-label="Previous History" className="border-t border-[#273144] pt-4">
                    <div className="mb-2.5 flex items-center justify-between">
                      <h3 className="text-[11px] font-bold tracking-wider text-[#98A2B8] uppercase">
                        Previous History ({previousHistoryEntries.length})
                      </h3>
                      <span className="font-mono text-[10.5px] text-[#64748B]">
                        Most recent first
                      </span>
                    </div>
                    <div className="space-y-2.5">
                      {previousHistoryEntries.map((entry) => renderHistoryCard(entry))}
                    </div>
                  </section>
                )}
              </div>
            )}
          </div>
        )}

        {/* TAB 2: DIRECTORY MANAGER */}
        {activeTab === 'directory' && (
          <div className="space-y-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex flex-wrap items-center gap-2.5">
                <div className="flex items-center rounded-md border border-[#273144] bg-[#090D14] p-0.5">
                  <button
                    type="button"
                    onClick={() => setDirSource('custom')}
                    className={`rounded px-3 py-1 text-xs font-medium transition-colors whitespace-nowrap tabular-nums ${
                      dirSource === 'custom'
                        ? 'bg-[#1D2636] text-[#FFF5FA]'
                        : 'text-[#98A2B8] hover:text-white'
                    }`}
                  >
                    My Custom Streamers ({customDirectory.length})
                  </button>
                  <button
                    type="button"
                    onClick={() => setDirSource('community')}
                    className={`rounded px-3 py-1 text-xs font-medium transition-colors whitespace-nowrap tabular-nums ${
                      dirSource === 'community'
                        ? 'bg-[#1D2636] text-[#FFF5FA]'
                        : 'text-[#98A2B8] hover:text-white'
                    }`}
                  >
                    Community Database ({communityDirectory.length})
                  </button>
                </div>

                <div className="relative w-60">
                  <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[#64748B]" />
                  <input
                    type="text"
                    value={dirSearch}
                    onChange={(e) => setDirSearch(e.target.value)}
                    placeholder="Filter Riot ID, alias, channel, server..."
                    className="w-full rounded border border-[#273144] bg-[#141B27] py-1.5 pr-3 pl-8 text-xs text-white placeholder-[#64748B] focus:border-[#FF5CA8] focus:outline-none"
                  />
                </div>
              </div>

              <button
                type="button"
                disabled={isSyncingDirectory}
                onClick={onSyncDirectory}
                className="inline-flex items-center gap-1.5 rounded-md border border-[#273144] bg-[#1D2636] px-3 py-1.5 text-xs font-medium text-[#F3EFF4] transition-colors hover:border-[#FF5CA8]/50 hover:bg-[#253044] disabled:opacity-50 whitespace-nowrap"
              >
                <RefreshCw
                  className={`h-3.5 w-3.5 ${isSyncingDirectory ? 'animate-spin' : ''}`}
                />
                {isSyncingDirectory ? 'Syncing...' : 'Sync Database'}
              </button>
            </div>

            <p className="text-[11.5px] text-[#98A2B8]">
              Use <span className="font-semibold text-[#FFF5FA]">+ Add Streamer</span> to submit
              new streamers or alternate Riot ID aliases. Maintainers review and verify submissions
              before they enter the shared database.
            </p>

            {filteredDirectory.length === 0 ? (
              <div className="rounded-lg border border-[#273144] bg-[#141B27]/50 py-12 text-center text-xs text-[#98A2B8]">
                No entries match your filter.
              </div>
            ) : (
              <div className="space-y-2">
                {filteredDirectory.map((d) => {
                  const pColor = platformColor(d.platform);
                  const regShort = d.server_region
                    ? getServerRegionInfo(d.server_region).shortLabel
                    : null;
                  return (
                    <div
                      key={`${d.id || d.riot_id}-${d.platform}-${d.channel_url}`}
                      className="flex flex-col justify-between gap-2.5 rounded-lg border border-[#273144] bg-[#141B27] px-4 py-3 sm:flex-row sm:items-center"
                    >
                      <div className="min-w-0">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-sm font-semibold text-[#FFF5FA]">
                            {d.riot_id}
                          </span>
                          <span className="text-xs font-semibold" style={{ color: pColor }}>
                            {d.platform}
                          </span>
                          <span className="text-[#273144]">·</span>
                          <span className="text-xs text-[#F3EFF4]">{d.channel_name}</span>
                          {regShort && (
                            <span className="rounded border border-[#273144] bg-[#1D2636] px-1.5 py-0.5 text-[10px] font-medium text-[#2DD4BF]">
                              {regShort}
                            </span>
                          )}
                          {d.verified ? (
                            <span className="rounded border border-[#22C55E]/40 bg-[#22C55E]/10 px-1.5 py-0.5 text-[10px] font-medium text-[#22C55E]">
                              Verified
                            </span>
                          ) : (
                            <span className="rounded border border-[#FB923C]/40 bg-[#FB923C]/10 px-1.5 py-0.5 text-[10px] font-medium text-[#FB923C]">
                              Unverified
                            </span>
                          )}
                        </div>
                        {d.aliases && d.aliases.length > 0 && (
                          <p className="mt-1 font-mono text-[10.5px] text-[#FFA6D2]">
                            Aliases: {d.aliases.join(', ')}
                          </p>
                        )}
                        <p className="mt-0.5 truncate font-mono text-[11px] text-[#98A2B8]">
                          {d.channel_url}
                        </p>
                      </div>

                      <div className="flex shrink-0 items-center gap-1.5">
                        {dirSource === 'custom' && (
                          <>
                            <button
                              type="button"
                              onClick={() => onOpenAddModal(d)}
                              className="inline-flex items-center gap-1 rounded border border-[#273144] bg-[#1D2636] px-2.5 py-1 text-xs text-[#F3EFF4] hover:bg-[#253044]"
                            >
                              <Pencil className="h-3 w-3" />
                              Edit
                            </button>
                            <button
                              type="button"
                              onClick={() => onDeleteCustomEntry(d.riot_id, d.platform)}
                              className="inline-flex items-center gap-1 rounded border border-[#273144] px-2.5 py-1 text-xs text-[#FF668A] hover:border-[#FF668A]"
                            >
                              <Trash2 className="h-3 w-3" />
                              Delete
                            </button>
                          </>
                        )}
                        <a
                          href={d.channel_url}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="inline-flex items-center gap-1 rounded border border-[#273144] bg-[#1D2636] px-2.5 py-1 text-xs text-[#F3EFF4] hover:bg-[#253044]"
                        >
                          Open
                          <ExternalLink className="h-3 w-3" />
                        </a>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}

            {/* Untrusted Input Sanitizer & IGN Heuristic Live Verification Strip */}
            <div className="border-t border-[#273144] pt-4">
              <div className="flex items-center gap-2 text-xs font-semibold text-[#FFF5FA]">
                <ShieldCheck className="h-4 w-4 text-[#22C55E]" />
                <span>Untrusted Directory URL &amp; IGN Heuristic Verifier</span>
              </div>
              <div className="mt-2.5 grid gap-3 md:grid-cols-2">
                <div className="rounded-lg border border-[#273144] bg-[#090D14] p-3">
                  <label className="block text-[11px] font-medium text-[#98A2B8]">
                    Test Rust <span className="font-mono text-[#F3EFF4]">clean_ign()</span> suffix stripper
                  </label>
                  <input
                    type="text"
                    value={testIgn}
                    onChange={(e) => setTestIgn(e.target.value)}
                    placeholder="Try Desktop, Alive, RohanYT, Vex_TV"
                    className="mt-1.5 w-full rounded border border-[#273144] bg-[#141B27] px-2.5 py-1.5 font-mono text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                  />
                  <p className="mt-2 font-mono text-[11px] text-[#98A2B8]">
                    Cleaned:{' '}
                    <span className="text-[#F3EFF4]">&ldquo;{cleanedIgnResult}&rdquo;</span>
                    {' · '}
                    {cleanedIgnResult.replace(/[^a-zA-Z0-9]/g, '').length >= 4 ? (
                      <span className="text-[#22C55E]">Eligible (&ge;4 alnum)</span>
                    ) : (
                      <span className="text-[#FF668A]">Skipped (&lt;4 alnum)</span>
                    )}
                  </p>
                </div>

                <div className="rounded-lg border border-[#273144] bg-[#090D14] p-3">
                  <div className="flex items-center justify-between gap-2">
                    <label className="text-[11px] font-medium text-[#98A2B8]">
                      Test Rust <span className="font-mono text-[#F3EFF4]">host_allowed()</span> guard
                    </label>
                    <select
                      value={testPlatform}
                      onChange={(e) => setTestPlatform(e.target.value as Platform)}
                      className="rounded border border-[#273144] bg-[#141B27] px-2 py-0.5 text-[11px] text-white focus:outline-none"
                    >
                      <option value="YouTube">YouTube</option>
                      <option value="Twitch">Twitch</option>
                      <option value="Kick">Kick</option>
                    </select>
                  </div>
                  <input
                    type="text"
                    value={testUrl}
                    onChange={(e) => setTestUrl(e.target.value)}
                    className="mt-1.5 w-full rounded border border-[#273144] bg-[#141B27] px-2.5 py-1.5 font-mono text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                  />
                  <p className="mt-2 font-mono text-[11px]">
                    {testedHostOk ? (
                      <span className="text-[#22C55E]">Allowed host: {testedHost}</span>
                    ) : (
                      <span className="text-[#FF668A]">
                        Blocked untrusted URL ({testedHost ? `host "${testedHost}" mismatch` : 'invalid scheme/userinfo/port'})
                      </span>
                    )}
                  </p>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* TAB 3: DISCOVER (OPTIONAL RECOMMENDATIONS — OFF BY DEFAULT) */}
        {activeTab === 'discover' && (
          <div>
            {!config.enable_recommendations ? (
              <div className="mx-auto my-8 max-w-2xl rounded-lg border border-[#273144] bg-[#141B27] p-6">
                <div className="flex items-start gap-3">
                  <div className="rounded-md border border-[#273144] bg-[#090D14] p-2.5">
                    <Radio className="h-5 w-5 text-[#FF5CA8]" />
                  </div>
                  <div>
                    <h3 className="text-base font-semibold text-[#FFF5FA]">
                      Live Streamer Recommendations are OFF
                    </h3>
                    <p className="mt-1 text-xs leading-relaxed text-[#98A2B8]">
                      Disabled by default to conserve internet bandwidth and external API quota.
                      Zero recommendation network requests are made while this feature is OFF.
                      Enabling it may increase internet usage and fetches cached live Valorant
                      creators near your active server (<span className="font-semibold text-[#2DD4BF]">{effectiveRegion.label}</span>).
                    </p>
                  </div>
                </div>
                <div className="mt-5">
                  <ToggleSettingRow
                    checked={false}
                    onChange={(next) => {
                      onToggleRecommendationsImmediate(next);
                      if (next) handleRefreshRecs(true);
                    }}
                    title="Enable Optional Streamer Recommendations (Twitch & YouTube)"
                    description="May increase internet usage. Recommendations are discovery suggestions only and are never treated as lobby detections."
                  />
                </div>
              </div>
            ) : (
              <div className="space-y-3.5">
                <div className="flex items-start gap-2 rounded-md border border-[#38BDF8]/35 bg-[#090D14] px-3 py-2 text-[11.5px] text-[#CBD5E1]">
                  <Info className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[#38BDF8]" />
                  <span>
                    <strong className="text-[#FFF5FA]">Discovery Suggestions — Not Matched to Your Lobby.</strong>{' '}
                    These streams are optional regional recommendations cached for 5 minutes to conserve API quota.
                  </span>
                </div>

                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="flex flex-wrap items-center gap-2.5">
                    <div className="flex items-center rounded-md border border-[#273144] bg-[#090D14] p-0.5">
                      {(['All', 'Twitch', 'YouTube'] as const).map((plat) => (
                        <button
                          key={plat}
                          type="button"
                          onClick={() => setDiscoverPlatform(plat)}
                          className={`rounded px-3 py-1 text-xs font-medium transition-colors whitespace-nowrap ${
                            discoverPlatform === plat
                              ? 'bg-[#1D2636] text-[#FFF5FA]'
                              : 'text-[#98A2B8] hover:text-white'
                          }`}
                        >
                          {plat === 'All' ? 'All Platforms' : plat}
                        </button>
                      ))}
                    </div>

                    <span className="inline-flex items-center gap-1.5 rounded border border-[#273144] bg-[#1D2636] px-2.5 py-1 text-[11px] font-medium text-[#2DD4BF]">
                      <Globe className="h-3 w-3" />
                      Server: {effectiveRegion.label}
                    </span>
                  </div>

                  <div className="flex items-center gap-3">
                    <button
                      type="button"
                      disabled={isRefreshingRecs}
                      onClick={() => handleRefreshRecs(true)}
                      className="inline-flex h-8 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3 text-xs font-bold text-[#0D111A] disabled:opacity-60 whitespace-nowrap"
                    >
                      <RefreshCw className={`h-3.5 w-3.5 ${isRefreshingRecs ? 'animate-spin' : ''}`} />
                      {isRefreshingRecs ? 'Refreshing...' : 'Refresh Live'}
                    </button>
                    <ToggleSwitch
                      checked={true}
                      onChange={(next) => onToggleRecommendationsImmediate(next)}
                      ariaLabel="Toggle Live Streamer Recommendations"
                    />
                  </div>
                </div>

                <p className="font-mono text-[11px] text-[#98A2B8]">
                  Showing {visibleRecommendations.length} suggestion(s) near {effectiveRegion.shortLabel} · Cached (5 min TTL) · Updated {recsUpdatedAt}
                </p>

                <div className="space-y-2.5">
                  {visibleRecommendations.map((rec) => {
                    const pColor = platformColor(rec.platform);
                    return (
                      <div
                        key={rec.id}
                        className="flex flex-col justify-between gap-3 rounded-lg border border-[#273144] bg-[#141B27] px-4 py-3 sm:flex-row sm:items-center"
                      >
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="inline-block h-2 w-2 rounded-full bg-[#22C55E]" />
                            <span className="text-sm font-bold text-[#FFF5FA]">
                              {rec.channel}
                            </span>
                            <span
                              className="rounded border px-1.5 py-0.5 text-[10.5px] font-semibold"
                              style={{
                                color: pColor,
                                borderColor: `${pColor}66`,
                                backgroundColor: `${pColor}1A`,
                              }}
                            >
                              {rec.platform}
                            </span>
                            <span className="font-mono text-[11px] font-medium text-[#22C55E] tabular-nums">
                              {formatViewers(rec.viewers)}
                            </span>
                            <span className="text-[11px] text-[#98A2B8]">
                              · {rec.region_label}
                            </span>
                          </div>
                          <p className="mt-1 truncate text-xs text-[#F3EFF4]">
                            {rec.title}
                          </p>
                        </div>

                        <div className="flex shrink-0 items-center gap-2">
                          <button
                            type="button"
                            onClick={() => onOpenAddFromRecommendation(rec)}
                            className="inline-flex h-8 items-center gap-1 rounded-md border border-[#273144] bg-[#1D2636] px-2.5 text-xs font-medium text-[#F3EFF4] hover:border-[#FF5CA8]/50 hover:bg-[#253044] whitespace-nowrap"
                          >
                            <Plus className="h-3.5 w-3.5 text-[#FFA6D2]" />
                            Directory
                          </button>
                          <a
                            href={rec.url}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="inline-flex h-8 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3 text-xs font-bold text-[#0D111A] whitespace-nowrap"
                          >
                            Watch Live
                            <ExternalLink className="h-3.5 w-3.5" />
                          </a>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        )}

        {/* TAB 4: SETTINGS */}
        {activeTab === 'settings' && (
          <div className="space-y-4">
            {/* Valorant Servers & Detection (GitHub repo settings intentionally hidden) */}
            <section className="rounded-lg border border-[#273144] bg-[#141B27] p-4">
              <h3 className="text-sm font-semibold text-[#FFF5FA]">
                Valorant Servers &amp; Detection
              </h3>
              <p className="mt-0.5 text-xs text-[#98A2B8]">
                Prioritizes your actual in-game Valorant server to prevent cross-server false positives (such as NA streamers appearing in Mumbai lobbies).
              </p>

              <div className="mt-3">
                <label className="mb-1.5 block text-xs font-medium text-[#F3EFF4]">
                  Active Valorant Server
                </label>
                <select
                  value={draft.server_override}
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      server_override: e.target.value as ServerRegionOverride,
                    })
                  }
                  className="w-full max-w-md rounded border border-[#273144] bg-[#0D111A] px-3 py-2 text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                >
                  <option value="auto">
                    Auto-detect from Valorant log (Current: {detectedRegionInfo.label})
                  </option>
                  {SERVER_REGIONS.map((reg) => (
                    <option key={reg.code} value={reg.code}>
                      {reg.label}
                    </option>
                  ))}
                </select>

                <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
                  <span className="mr-1 text-[11px] text-[#98A2B8]">Quick Select:</span>
                  {QUICK_SERVER_CHIPS.map((chip) => (
                    <button
                      key={chip.code}
                      type="button"
                      onClick={() => setDraft({ ...draft, server_override: chip.code })}
                      className={`rounded border px-2.5 py-1 text-[11px] font-medium transition-colors whitespace-nowrap ${
                        draft.server_override === chip.code
                          ? 'border-[#FF5CA8] bg-[#1D2636] text-[#FFF5FA]'
                          : 'border-[#273144] bg-[#0D111A] text-[#98A2B8] hover:text-white'
                      }`}
                    >
                      {chip.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="mt-3 space-y-1 text-[11px]">
                <p className="font-mono text-[#2DD4BF]">
                  Detected Server: {detectedRegionInfo.label} ({detectedRegionInfo.podExample})
                </p>
                <p className="text-[#98A2B8]">
                  Shared streamer database updates automatically in the background — no repository configuration required.
                </p>
              </div>
            </section>

            {/* Features & Windows Integration — Left-to-Right ON/OFF Toggle Sliders */}
            <section className="rounded-lg border border-[#273144] bg-[#141B27] p-4">
              <h3 className="text-sm font-semibold text-[#FFF5FA]">
                Features &amp; Windows Integration
              </h3>
              <div className="mt-3 space-y-2.5">
                <ToggleSettingRow
                  checked={draft.skip_party_members}
                  onChange={(next) =>
                    setDraft({ ...draft, skip_party_members: next })
                  }
                  title="Skip Party Members (Save API Calls)"
                  description="ON by default. Uses Riot match-details partyId (zero extra Riot API requests) to exclude your party teammates from external YouTube/Twitch/Kick lookups."
                />
                <ToggleSettingRow
                  checked={draft.enable_recommendations}
                  onChange={(next) =>
                    setDraft({ ...draft, enable_recommendations: next })
                  }
                  title="Live Streamer Recommendations (Twitch & YouTube)"
                  description="OFF by default to save internet bandwidth. Enabling may increase network usage and unlocks cached regional suggestions in the Discover tab."
                />
                <ToggleSettingRow
                  checked={draft.capture_in_game_chat}
                  onChange={(next) =>
                    setDraft({ ...draft, capture_in_game_chat: next })
                  }
                  title="Post-Match In-Game Chat Stream Plugs"
                  description={
                    <>
                      Reads post-match text chat for messages like{' '}
                      <span className="font-mono text-[#FFA6D2]">&ldquo;im live on yt&rdquo;</span>{' '}
                      or dropped YouTube/Twitch/Kick links via{' '}
                      <span className="font-mono text-[#F3EFF4]">/chat/v6/messages</span>.
                    </>
                  }
                />
                <ToggleSettingRow
                  checked={draft.start_with_windows}
                  onChange={(next) =>
                    setDraft({ ...draft, start_with_windows: next })
                  }
                  title="Start Silently with Windows"
                  description={
                    <>
                      Launches minimized in the system tray at logon via{' '}
                      <span className="font-mono text-[#F3EFF4]">
                        HKCU\Software\Microsoft\Windows\CurrentVersion\Run
                      </span>
                      .
                    </>
                  }
                />
                <ToggleSettingRow
                  checked={draft.use_custom_toast_app_id}
                  onChange={(next) =>
                    setDraft({ ...draft, use_custom_toast_app_id: next })
                  }
                  title="Custom Windows Toast App ID"
                  description="Shows 'Valorant Streamer Radar' as the sender on desktop notifications instead of Windows PowerShell."
                />
              </div>
            </section>

            {/* Optional Platform API Keys */}
            <section className="rounded-lg border border-[#273144] bg-[#141B27] p-4">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <h3 className="text-sm font-semibold text-[#FFF5FA]">
                    Optional Platform API Keys
                  </h3>
                  <p className="mt-0.5 text-xs text-[#98A2B8]">
                    Leave blank to run 100% keyless on YouTube, Twitch, and Kick, or add free keys for faster official API checks.
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-[11px] text-[#98A2B8]">Show keys</span>
                  <ToggleSwitch
                    checked={showSecrets}
                    onChange={setShowSecrets}
                    ariaLabel="Show API keys"
                    showStateText={false}
                  />
                </div>
              </div>

              <div className="mt-3.5 space-y-3.5">
                {/* YouTube Data API v3 */}
                <div>
                  <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2">
                    <label className="text-xs font-medium text-[#F3EFF4]">
                      YouTube Data API v3 Key
                    </label>
                    <div className="flex items-center gap-1.5">
                      <a
                        href="https://console.cloud.google.com/apis/credentials"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center gap-1 rounded bg-pink-white-gradient bg-pink-white-gradient-hover px-2.5 py-1 text-[11px] font-bold text-[#0D111A] whitespace-nowrap"
                      >
                        Get YouTube Key
                        <ExternalLink className="h-3 w-3" />
                      </a>
                      <button
                        type="button"
                        onClick={handleTestYoutubeKey}
                        className="rounded border border-[#273144] bg-[#1D2636] px-2.5 py-1 text-[11px] font-medium text-[#F3EFF4] hover:bg-[#253044] whitespace-nowrap"
                      >
                        Test Key
                      </button>
                    </div>
                  </div>
                  <input
                    type={showSecrets ? 'text' : 'password'}
                    value={draft.youtube_api_key}
                    onChange={(e) => {
                      setDraft({ ...draft, youtube_api_key: e.target.value });
                      setYtKeyStatus(null);
                    }}
                    placeholder="AIza... (optional — falls back to keyless search automatically)"
                    className="w-full rounded border border-[#273144] bg-[#0D111A] px-3 py-1.5 font-mono text-xs text-white placeholder-[#64748B] focus:border-[#FF5CA8] focus:outline-none"
                  />
                  {ytKeyStatus && (
                    <div
                      className={`mt-1.5 flex items-start gap-1.5 rounded border px-2.5 py-1.5 text-[11px] ${
                        ytKeyStatus.kind === 'ok'
                          ? 'border-[#22C55E]/40 bg-[#22C55E]/10 text-[#22C55E]'
                          : 'border-[#FF668A]/40 bg-[#FF668A]/10 text-[#FF668A]'
                      }`}
                    >
                      {ytKeyStatus.kind === 'ok' ? (
                        <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                      ) : (
                        <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                      )}
                      <span>{ytKeyStatus.msg}</span>
                    </div>
                  )}
                </div>

                {/* Twitch Helix API */}
                <div>
                  <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2">
                    <span className="text-xs font-medium text-[#F3EFF4]">
                      Twitch Helix API (Client ID &amp; Secret)
                    </span>
                    <div className="flex items-center gap-1.5">
                      <a
                        href="https://dev.twitch.tv/console/apps"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center gap-1 rounded bg-[#9146FF] px-2.5 py-1 text-[11px] font-semibold text-white transition-colors hover:bg-[#7c35e8] whitespace-nowrap"
                      >
                        Get Twitch Keys
                        <ExternalLink className="h-3 w-3" />
                      </a>
                      <button
                        type="button"
                        onClick={handleTestTwitchKeys}
                        className="rounded border border-[#273144] bg-[#1D2636] px-2.5 py-1 text-[11px] font-medium text-[#F3EFF4] hover:bg-[#253044] whitespace-nowrap"
                      >
                        Test Twitch
                      </button>
                    </div>
                  </div>
                  <div className="grid gap-2.5 sm:grid-cols-2">
                    <div>
                      <label className="mb-1 block text-[11px] text-[#98A2B8]">
                        Twitch Client ID
                      </label>
                      <input
                        type={showSecrets ? 'text' : 'password'}
                        value={draft.twitch_client_id}
                        onChange={(e) => {
                          setDraft({ ...draft, twitch_client_id: e.target.value });
                          setTwitchKeyStatus(null);
                        }}
                        placeholder="Optional Helix Client ID"
                        className="w-full rounded border border-[#273144] bg-[#0D111A] px-3 py-1.5 font-mono text-xs text-white placeholder-[#64748B] focus:border-[#FF5CA8] focus:outline-none"
                      />
                    </div>
                    <div>
                      <label className="mb-1 block text-[11px] text-[#98A2B8]">
                        Twitch Client Secret
                      </label>
                      <input
                        type={showSecrets ? 'text' : 'password'}
                        value={draft.twitch_client_secret}
                        onChange={(e) => {
                          setDraft({ ...draft, twitch_client_secret: e.target.value });
                          setTwitchKeyStatus(null);
                        }}
                        placeholder="Optional Helix Client Secret"
                        className="w-full rounded border border-[#273144] bg-[#0D111A] px-3 py-1.5 font-mono text-xs text-white placeholder-[#64748B] focus:border-[#FF5CA8] focus:outline-none"
                      />
                    </div>
                  </div>
                  {twitchKeyStatus && (
                    <div
                      className={`mt-1.5 flex items-start gap-1.5 rounded border px-2.5 py-1.5 text-[11px] ${
                        twitchKeyStatus.kind === 'ok'
                          ? 'border-[#22C55E]/40 bg-[#22C55E]/10 text-[#22C55E]'
                          : 'border-[#FF668A]/40 bg-[#FF668A]/10 text-[#FF668A]'
                      }`}
                    >
                      {twitchKeyStatus.kind === 'ok' ? (
                        <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                      ) : (
                        <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                      )}
                      <span>{twitchKeyStatus.msg}</span>
                    </div>
                  )}
                </div>

                {/* Collapsible Step-by-Step API Key Tutorial */}
                <div className="rounded-md border border-[#273144] bg-[#0D111A]">
                  <button
                    type="button"
                    onClick={() => setShowApiGuide((prev) => !prev)}
                    className="flex w-full items-center justify-between px-3 py-2 text-left text-xs font-semibold text-[#FFA6D2] transition-colors hover:text-[#FFF5FA]"
                  >
                    <span className="inline-flex items-center gap-1.5">
                      <KeyRound className="h-3.5 w-3.5" />
                      How to get free API keys (Step-by-Step Guide)
                    </span>
                    <ChevronDown
                      className={`h-3.5 w-3.5 transition-transform ${showApiGuide ? 'rotate-180' : ''}`}
                    />
                  </button>
                  {showApiGuide && (
                    <div className="space-y-3 border-t border-[#273144] px-3.5 py-3 text-[11.5px] leading-relaxed text-[#F3EFF4]">
                      <div>
                        <p className="font-semibold text-[#FF5CA8]">
                          YouTube Data API v3 (Free — 10,000 units/day, no card needed)
                        </p>
                        <ol className="mt-1 list-decimal space-y-0.5 pl-4 text-[#CBD5E1]">
                          <li>Click <strong className="text-white">Get YouTube Key</strong> above to open Google Cloud Console.</li>
                          <li>Create a new project (e.g. <span className="font-mono text-white">Valorant-Radar</span>) from the top-left selector.</li>
                          <li>Go to <strong className="text-white">APIs &amp; Services &rarr; Library</strong>, search <span className="font-mono text-white">YouTube Data API v3</span>, and click <strong className="text-white">Enable</strong>.</li>
                          <li>Go to <strong className="text-white">APIs &amp; Services &rarr; Credentials &rarr; + CREATE CREDENTIALS &rarr; API key</strong>.</li>
                          <li>Paste the <span className="font-mono text-white">AIza...</span> key above, click <strong className="text-white">Test Key</strong>, then <strong className="text-white">Save Settings</strong>.</li>
                        </ol>
                      </div>
                      <div>
                        <p className="font-semibold text-[#A770FF]">
                          Twitch Helix API (Free — requires 2FA on your Twitch account)
                        </p>
                        <ol className="mt-1 list-decimal space-y-0.5 pl-4 text-[#CBD5E1]">
                          <li>Click <strong className="text-white">Get Twitch Keys</strong> above to open the Twitch Developer Console.</li>
                          <li>Click <strong className="text-white">Register Your Application</strong> and name it <span className="font-mono text-white">Valorant Radar</span>.</li>
                          <li>Set OAuth Redirect URL to <span className="font-mono text-white">http://localhost</span> and Category to <strong className="text-white">Application Integration</strong>.</li>
                          <li>Open <strong className="text-white">Manage</strong> &rarr; copy <strong className="text-white">Client ID</strong>, click <strong className="text-white">New Secret</strong> &rarr; copy <strong className="text-white">Client Secret</strong>.</li>
                          <li>Paste both above, click <strong className="text-white">Test Twitch</strong>, then <strong className="text-white">Save Settings</strong>.</li>
                        </ol>
                      </div>
                      <p className="flex items-center gap-1.5 text-[11px] text-[#98A2B8]">
                        <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-[#FB923C]" />
                        Kick needs no API key. Leaving all keys blank still works via keyless fallbacks.
                      </p>
                    </div>
                  )}
                </div>
              </div>
            </section>

            {/* App Updates & Version */}
            <section className="rounded-lg border border-[#273144] bg-[#141B27] p-4">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <h3 className="text-sm font-semibold text-[#FFF5FA]">
                    App Updates &amp; Version
                  </h3>
                  <p className="mt-0.5 font-mono text-[11.5px] text-[#98A2B8]">
                    Installed: v{APP_VERSION} · Checks GitHub Releases automatically
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <button
                    type="button"
                    disabled={updateState.status === 'checking'}
                    onClick={() => handleCheckForUpdates(false)}
                    className="inline-flex h-8 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3.5 text-xs font-bold text-[#0D111A] transition-opacity disabled:opacity-60 whitespace-nowrap"
                  >
                    <RefreshCw
                      className={`h-3.5 w-3.5 ${updateState.status === 'checking' ? 'animate-spin' : ''}`}
                    />
                    {updateState.status === 'checking' ? 'Checking...' : 'Check for Updates'}
                  </button>
                  <a
                    href={`https://github.com/${COMMUNITY_REPO_OWNER}/${COMMUNITY_REPO_NAME}/releases`}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[#273144] bg-[#1D2636] px-3 text-xs font-medium text-[#F3EFF4] hover:border-[#FF5CA8]/50 whitespace-nowrap"
                  >
                    Open Releases
                    <ExternalLink className="h-3.5 w-3.5" />
                  </a>
                </div>
              </div>

              {updateState.status === 'up_to_date' && (
                <p className="mt-2.5 flex items-center gap-1.5 text-xs text-[#22C55E]">
                  <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />
                  {updateState.message}
                </p>
              )}
              {updateState.status === 'available' && (
                <div className="mt-2.5 space-y-2 rounded border border-[#FF5CA8]/40 bg-[#090D14] p-3 text-xs">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="font-semibold text-[#FFA6D2]">
                      New release: {updateState.releaseName} ({updateState.tagName})
                      {updateState.sizeLabel ? ` · ${updateState.sizeLabel}` : ''}
                    </span>
                    <a
                      href={updateState.downloadUrl}
                      target="_blank"
                      rel="noreferrer"
                      className="inline-flex items-center gap-1.5 rounded bg-pink-white-gradient px-3 py-1 text-[11.5px] font-bold text-[#0D111A]"
                    >
                      <Download className="h-3 w-3" />
                      Download &amp; Install
                    </a>
                  </div>
                  <div className="max-h-32 overflow-y-auto rounded border border-[#273144] bg-[#0D111A] p-2 font-mono text-[11px] whitespace-pre-wrap text-[#CBD5E1]">
                    {updateState.changelog}
                  </div>
                </div>
              )}
              {updateState.status === 'error' && (
                <p className="mt-2.5 flex items-center gap-1.5 text-xs text-[#FF668A]">
                  <XCircle className="h-3.5 w-3.5 shrink-0" />
                  {updateState.message}
                </p>
              )}
            </section>

            <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
              <span className="font-mono text-[11px] text-[#98A2B8]">
                Stored atomically in %APPDATA%\ValorantStreamerRadar\config.json
              </span>
              <button
                type="button"
                onClick={handleSaveSettings}
                className="rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-5 py-2 text-xs font-bold text-[#0D111A] transition-opacity whitespace-nowrap"
              >
                Save Settings
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
