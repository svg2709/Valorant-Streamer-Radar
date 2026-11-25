import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  AlertTriangle,
  ArrowUpCircle,
  BookOpen,
  Check,
  CheckCircle2,
  ChevronDown,
  Clock,
  Compass,
  Copy,
  Download,
  ExternalLink,
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
  Settings as SettingsIcon,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
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

function tierColor(tier: ConfidenceTier): string {
  if (tier === 'Confirmed') return '#22C55E';
  if (tier === 'Probable') return '#FB923C';
  return '#8E95A5';
}

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
          checked ? 'text-[#FFA6D2]' : 'text-[#5A6275]'
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
          : 'border-[#1F1F26] bg-[#121216] hover:border-[#FF5CA8]/50'
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
        ? 'border-[#FF5CA8]/35 bg-[#000000]'
        : 'border-[#1F1F26] bg-[#000000]'
    }`}
  >
    <div className="min-w-0">
      <p className="text-xs font-semibold text-[#F4F4F6]">{title}</p>
      <p className="mt-0.5 text-[11px] leading-relaxed text-[#8E95A5]">
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
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [expandedCards, setExpandedCards] = useState<Record<string, boolean>>({});
  const filterPopoverRef = useRef<HTMLDivElement | null>(null);

  // Directory state
  const [dirSource, setDirSource] = useState<'custom' | 'community'>('custom');
  const [dirSearch, setDirSearch] = useState('');
  const [showSanitizerTools, setShowSanitizerTools] = useState(false);
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

  // App Updater state
  const [updateState, setUpdateState] = useState<UpdateState>({ status: 'idle' });
  const [showChangelog, setShowChangelog] = useState(true);
  const startupCheckedRef = useRef(false);

  // Close filters popover on outside click or Escape
  useEffect(() => {
    if (!filtersOpen) return;
    const handleMouseDown = (e: MouseEvent) => {
      if (filterPopoverRef.current && !filterPopoverRef.current.contains(e.target as Node)) {
        setFiltersOpen(false);
      }
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setFiltersOpen(false);
    };
    document.addEventListener('mousedown', handleMouseDown);
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('mousedown', handleMouseDown);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [filtersOpen]);

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

      if (compareSemver(latestTag, APP_VERSION) <= 0) {
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

  const activePopoverFilterCount = useMemo(() => {
    let count = 0;
    if (teamFilter !== 'All') count += 1;
    if (matchTypeFilter !== 'All') count += 1;
    if (serverFilter !== 'All') count += 1;
    if (dateRangeFilter !== 'all') count += 1;
    if (statusFilter !== 'all') count += 1;
    return count;
  }, [teamFilter, matchTypeFilter, serverFilter, dateRangeFilter, statusFilter]);

  const hasActiveFilters = searchQuery.trim() !== '' || activePopoverFilterCount > 0;

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

  const toggleCardDetails = (cardKey: string) => {
    setExpandedCards((prev) => ({ ...prev, [cardKey]: !prev[cardKey] }));
  };

  const renderHistoryCard = (entry: StreamerEntry) => {
    const cardKey = `${entry.match_id}-${entry.riot_id}-${entry.platform}`;
    const pColor = platformColor(entry.platform);
    const tier = resolveEntryTier(entry);
    const tColor = tierColor(tier);
    const metaItems = [
      entry.map_name,
      entry.agent_name,
      entry.queue_name,
      entry.timestamp,
    ].filter(Boolean);

    const hasExpandableDetails = Boolean(
      entry.chat_message ||
        entry.server_match_status ||
        entry.match_reason ||
        (entry.candidates && entry.candidates.length > 1)
    );
    const isExpanded = Boolean(expandedCards[cardKey]);

    return (
      <article
        key={cardKey}
        className={`rounded-lg border bg-[#0B0B0E] px-4 py-3 transition-colors ${
          entry.chat_message
            ? 'border-[#FF5CA8]/40'
            : 'border-[#1F1F26] hover:border-[#2B2B36]'
        }`}
      >
        <div className="flex flex-col justify-between gap-3 sm:flex-row sm:items-center">
          <div className="min-w-0 flex-1">
            {/* Primary typographic line: zero boxed pill clutter */}
            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs">
              <span className="font-mono text-[14px] font-bold text-[#FFF5FA]">
                {entry.riot_id}
              </span>
              <span className="text-[#1F1F26]" aria-hidden="true">/</span>
              <span className="font-semibold" style={{ color: pColor }}>
                {entry.platform}
              </span>
              <span className="font-medium text-[#F4F4F6]">{entry.channel}</span>
              {entry.team_relation && (
                <>
                  <span className="text-[#1F1F26]" aria-hidden="true">/</span>
                  <span
                    className="font-semibold"
                    style={{
                      color: entry.team_relation === 'Ally' ? '#2DD4BF' : '#FB923C',
                    }}
                  >
                    {entry.team_relation}
                  </span>
                </>
              )}
              <span className="text-[#1F1F26]" aria-hidden="true">/</span>
              <span
                className="font-mono text-[11px] font-semibold tabular-nums"
                style={{ color: tColor }}
              >
                {tier}
                {entry.confidence ? ` · ${entry.confidence}%` : ''}
              </span>
              {entry.server_region && (
                <span className="text-[11px] font-medium text-[#2DD4BF]">
                  · {entry.server_region}
                </span>
              )}
              {entry.chat_message && (
                <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#FFA6D2]">
                  · <MessageSquareQuote className="h-3 w-3" /> Chat
                </span>
              )}
            </div>

            <p className="mt-1 truncate text-[12.5px] text-[#D8DCE6]">{entry.title}</p>

            <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
              <span className="font-mono text-[11px] text-[#8E95A5] tabular-nums">
                {metaItems.join('  ·  ')}
              </span>
              {hasExpandableDetails && (
                <button
                  type="button"
                  onClick={() => toggleCardDetails(cardKey)}
                  className="inline-flex items-center gap-1 text-[11px] font-medium text-[#8E95A5] transition-colors hover:text-[#FFA6D2]"
                >
                  <span>
                    {isExpanded
                      ? 'Hide details'
                      : entry.candidates && entry.candidates.length > 1
                      ? `Details (${entry.candidates.length})`
                      : 'Details'}
                  </span>
                  <ChevronDown
                    className={`h-3 w-3 transition-transform ${isExpanded ? 'rotate-180' : ''}`}
                  />
                </button>
              )}
            </div>
          </div>

          <div className="flex shrink-0 items-center gap-1.5">
            <button
              type="button"
              onClick={() => onCopyLink(entry.url)}
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#121216] px-2.5 text-xs font-medium text-[#F4F4F6] transition-colors hover:border-[#FF5CA8]/45 hover:bg-[#1A1A20] whitespace-nowrap"
            >
              <Copy className="h-3.5 w-3.5 text-[#8E95A5]" />
              Copy
            </button>
            <button
              type="button"
              onClick={() => onWatchStream(entry)}
              title={entry.url}
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3 text-xs font-bold text-[#000000] transition-opacity whitespace-nowrap"
            >
              Watch
              <ExternalLink className="h-3 w-3" />
            </button>
          </div>
        </div>

        {/* Expandable Details Disclosure (chat plug, match rationale, alternate candidates) */}
        {hasExpandableDetails && isExpanded && (
          <div className="mt-3 space-y-2 border-t border-[#1F1F26] pt-2.5">
            {entry.chat_message && (
              <div className="rounded border border-[#FF5CA8]/30 bg-[#000000] px-2.5 py-1.5 text-[11.5px] text-[#FFA6D2]">
                <span className="font-semibold text-[#FFF5FA]">In-Game Chat:</span>{' '}
                &ldquo;{entry.chat_message}&rdquo;
              </div>
            )}

            {(entry.server_match_status || entry.match_reason) && (
              <p className="text-[11px] text-[#8E95A5]">
                {entry.server_match_status && (
                  <span className="font-medium text-[#D8DCE6]">
                    {entry.server_match_status}
                  </span>
                )}
                {entry.server_match_status && entry.match_reason ? ' — ' : ''}
                {entry.match_reason}
              </p>
            )}

            {entry.candidates && entry.candidates.length > 1 && (
              <div className="rounded-md border border-[#1F1F26] bg-[#000000] px-3 py-2">
                <p className="text-[10px] font-semibold tracking-wider text-[#8E95A5] uppercase">
                  Ranked Candidates ({entry.candidates.length})
                </p>
                <div className="mt-1.5 space-y-1.5">
                  {entry.candidates.map((cand, idx) => {
                    const cPlatColor = platformColor(cand.platform);
                    const cTier =
                      cand.confidence_tier ??
                      (cand.confidence >= 85
                        ? 'Confirmed'
                        : cand.confidence >= 65
                        ? 'Probable'
                        : 'Unverified');
                    return (
                      <div
                        key={`${cand.url}-${idx}`}
                        className="flex flex-wrap items-center justify-between gap-2 text-[11.5px]"
                      >
                        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
                          <span
                            className={`font-mono font-bold ${
                              idx === 0 ? 'text-[#FFA6D2]' : 'text-[#5A6275]'
                            }`}
                          >
                            #{idx + 1}
                          </span>
                          <span
                            className="font-mono font-semibold tabular-nums"
                            style={{ color: tierColor(cTier) }}
                          >
                            {cTier} ({cand.confidence}%)
                          </span>
                          <span className="font-semibold" style={{ color: cPlatColor }}>
                            {cand.platform}
                          </span>
                          <span className="font-medium text-[#F4F4F6]">{cand.channel}</span>
                          {cand.region_hint && (
                            <span className="text-[10.5px] text-[#2DD4BF]">
                              · {cand.region_hint}
                            </span>
                          )}
                          <span className="truncate text-[10.5px] text-[#8E95A5]">
                            — {cand.match_reason}
                          </span>
                        </div>
                        <a
                          href={cand.url}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="inline-flex shrink-0 items-center gap-1 text-[11px] font-semibold text-[#FFA6D2] hover:text-[#FFF5FA]"
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
          </div>
        )}
      </article>
    );
  };

  if (!visible) {
    return (
      <div className="flex min-h-[540px] flex-col items-center justify-center rounded-lg border border-dashed border-[#1F1F26] bg-[#000000] px-6 py-16 text-center">
        <RadarIcon size={40} />
        <h3 className="mt-4 text-base font-semibold text-[#FFF5FA]">
          Window Hidden in Windows System Tray
        </h3>
        <p className="mt-1.5 max-w-md text-xs leading-relaxed text-[#8E95A5]">
          Closing the custom title bar intercepts the close event and sets{' '}
          <span className="font-mono text-[#F4F4F6]">WINDOW_VISIBLE = false</span>.
          The UI loop schedules zero repaints while the background{' '}
          <span className="font-mono text-[#F4F4F6]">radar-bg</span> thread continues
          tailing <span className="font-mono text-[#F4F4F6]">ShooterGame.log</span>.
        </p>
        <button
          type="button"
          onClick={onRestoreFromTray}
          className="mt-5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-4 py-2 text-xs font-bold text-[#000000] transition-opacity"
        >
          Open Streamer History (Left-Click Tray)
        </button>
      </div>
    );
  }

  const navItems: Array<{
    key: DesktopTab;
    label: string;
    meta?: string;
    icon: React.ComponentType<{ className?: string }>;
  }> = [
    { key: 'history', label: 'History', meta: String(history.length), icon: Clock },
    {
      key: 'directory',
      label: 'Directory',
      meta: String(customDirectory.length + communityDirectory.length),
      icon: BookOpen,
    },
    {
      key: 'discover',
      label: 'Discover',
      meta: config.enable_recommendations ? 'Live' : 'Off',
      icon: Compass,
    },
    { key: 'settings', label: 'Settings', icon: SettingsIcon },
  ];

  return (
    <div className="overflow-hidden rounded-lg border border-[#1F1F26] bg-[#000000] shadow-2xl">
      {/* 2px Pink-to-Pearl Top Accent Hairline */}
      <div className="h-[2px] w-full bg-pink-white-strip" />

      {/* Custom Frameless 34px AMOLED Title Bar (Minimize & Close-to-tray ONLY) */}
      <div className="flex h-[34px] items-center justify-between border-b border-[#1F1F26] bg-[#000000] pl-3 pr-1 select-none">
        <div className="flex min-w-0 items-center gap-2">
          <RadarIcon size={16} className="shrink-0" />
          <span
            className="text-[13px] font-bold tracking-wider whitespace-nowrap"
            style={{ fontFamily: 'var(--font-display)' }}
          >
            <span className="text-pink-white-gradient">VALORANT</span>{' '}
            <span className="text-[#FFF5FA]">RADAR</span>
          </span>
          <span className="hidden text-[#1F1F26] sm:inline">/</span>
          <span className="hidden truncate text-[11px] text-[#8E95A5] sm:inline">
            {status}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {toastMessage && (
            <span className="hidden max-w-[280px] items-center gap-1 truncate text-[11px] font-medium text-[#FFA6D2] md:inline-flex">
              <Check className="h-3 w-3 shrink-0 text-[#FF5CA8]" />
              <span className="truncate">{toastMessage}</span>
            </span>
          )}
          <div className="flex items-center">
            <button
              type="button"
              onClick={onHideToTray}
              title="Minimize"
              aria-label="Minimize window"
              className="flex h-[32px] w-[38px] items-center justify-center text-[#8E95A5] transition-colors hover:bg-[#1A1A20] hover:text-white"
            >
              <Minus className="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              onClick={onHideToTray}
              title="Close to system tray"
              aria-label="Close to system tray"
              className="flex h-[32px] w-[38px] items-center justify-center text-[#8E95A5] transition-colors hover:bg-[#DC2626] hover:text-white"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>

      {/* Body: 208px Left Sidebar Navigation + Main AMOLED Content Canvas */}
      <div className="flex min-h-[580px] flex-col md:flex-row">
        {/* Left Sidebar Navigation Rail */}
        <aside className="flex w-full shrink-0 flex-col justify-between border-b border-[#1F1F26] bg-[#000000] p-3 md:w-[208px] md:border-r md:border-b-0">
          <nav className="flex flex-row gap-1 overflow-x-auto md:flex-col md:overflow-visible" aria-label="App sections">
            {navItems.map(({ key, label, meta, icon: IconComp }) => {
              const active = activeTab === key;
              return (
                <button
                  key={key}
                  type="button"
                  onClick={() => onChangeTab(key)}
                  className={`relative flex shrink-0 items-center justify-between gap-2 rounded-md px-2.5 py-2 text-xs font-medium transition-colors whitespace-nowrap md:w-full ${
                    active
                      ? 'bg-[#121216] text-[#FFF5FA]'
                      : 'text-[#8E95A5] hover:bg-[#0B0B0E] hover:text-[#F4F4F6]'
                  }`}
                >
                  {active && (
                    <span
                      className="absolute top-1.5 bottom-1.5 left-0 w-[2px] rounded-r bg-[#FF5CA8]"
                      aria-hidden="true"
                    />
                  )}
                  <span className="inline-flex items-center gap-2">
                    <IconComp
                      className={`h-3.5 w-3.5 shrink-0 ${
                        active ? 'text-[#FF5CA8]' : 'text-[#5A6275]'
                      }`}
                    />
                    <span>{label}</span>
                  </span>
                  {meta && (
                    <span
                      className={`font-mono text-[10.5px] tabular-nums ${
                        key === 'discover' && config.enable_recommendations
                          ? 'text-[#22C55E]'
                          : active
                          ? 'text-[#FFA6D2]'
                          : 'text-[#5A6275]'
                      }`}
                    >
                      {meta}
                    </span>
                  )}
                </button>
              );
            })}
          </nav>

          {/* Bottom-Anchored Status & Primary Actions */}
          <div className="mt-3 space-y-2 border-t border-[#1F1F26] pt-3 md:mt-6">
            <div className="rounded-md border border-[#1F1F26] bg-[#0B0B0E] px-2.5 py-2">
              <div className="flex items-center gap-1.5 text-[11px] font-medium text-[#F4F4F6]">
                <span
                  className="inline-block h-1.5 w-1.5 shrink-0 rounded-full"
                  style={{
                    backgroundColor: valorantRunning ? '#22C55E' : '#5A6275',
                  }}
                />
                <span className="truncate">
                  {valorantRunning ? 'Valorant Active' : 'Waiting for Riot'}
                </span>
              </div>
              <p className="mt-0.5 truncate font-mono text-[10.5px] text-[#2DD4BF]">
                {effectiveRegion.shortLabel}
              </p>
            </div>

            <button
              type="button"
              disabled={isScanning}
              onClick={onTriggerManualScan}
              className={`inline-flex h-8 w-full items-center justify-center rounded-md px-3 text-xs font-bold transition-all whitespace-nowrap ${
                isScanning
                  ? 'cursor-not-allowed border border-[#1F1F26] bg-[#121216] text-[#8E95A5]'
                  : 'bg-pink-white-gradient bg-pink-white-gradient-hover text-[#000000]'
              }`}
            >
              {isScanning ? 'Scanning...' : 'Check Last Match'}
            </button>

            <button
              type="button"
              onClick={() => onOpenAddModal()}
              className="inline-flex h-8 w-full items-center justify-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#121216] px-3 text-xs font-medium text-[#F4F4F6] transition-colors hover:border-[#FF5CA8]/50 hover:bg-[#1A1A20] whitespace-nowrap"
            >
              <Plus className="h-3.5 w-3.5 text-[#FFA6D2]" />
              Add Streamer
            </button>

            <div className="flex items-center justify-between pt-1">
              <span className="font-mono text-[10.5px] text-[#5A6275]">v{APP_VERSION}</span>
              <button
                type="button"
                disabled={updateState.status === 'checking'}
                onClick={() => handleCheckForUpdates(false)}
                className={`inline-flex items-center gap-1 text-[10.5px] font-medium transition-colors whitespace-nowrap ${
                  updateState.status === 'available'
                    ? 'text-[#FF5CA8] hover:text-[#FFA6D2]'
                    : 'text-[#8E95A5] hover:text-[#FFF5FA]'
                } disabled:opacity-60`}
              >
                <RefreshCw
                  className={`h-3 w-3 ${updateState.status === 'checking' ? 'animate-spin text-[#FFA6D2]' : ''}`}
                />
                {updateState.status === 'checking'
                  ? 'Checking...'
                  : updateState.status === 'available'
                  ? `Update ${updateState.tagName}`
                  : 'Check Updates'}
              </button>
            </div>
          </div>
        </aside>

        {/* Main Panel (#000000) */}
        <div className="min-w-0 flex-1 max-h-[680px] overflow-y-auto bg-[#000000] px-5 py-4">
          {/* Global Update Banner */}
          {updateState.status === 'available' && (
            <div className="mb-4 rounded-lg border border-[#FF5CA8]/70 bg-[#0B0B0E] px-4 py-3">
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
                    <span className="font-mono text-[11px] text-[#8E95A5]">
                      (v{APP_VERSION} &rarr; {updateState.tagName})
                    </span>
                  </div>
                  <p className="mt-0.5 font-mono text-[11px] text-[#8E95A5]">
                    {[
                      updateState.buildSha ? `Build ${updateState.buildSha}` : '',
                      updateState.sizeLabel,
                      updateState.publishedAt,
                      updateState.shaVerified ? 'SHA-256 verified' : '',
                    ]
                      .filter(Boolean)
                      .join('  ·  ')}
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <button
                    type="button"
                    onClick={() => setShowChangelog((prev) => !prev)}
                    className="inline-flex h-7 items-center gap-1 rounded-md border border-[#1F1F26] bg-[#121216] px-2.5 text-[11.5px] font-medium text-[#F4F4F6] hover:border-[#FF5CA8]/50 whitespace-nowrap"
                  >
                    {showChangelog ? 'Hide Notes' : 'Notes'}
                    <ChevronDown
                      className={`h-3 w-3 transition-transform ${showChangelog ? 'rotate-180' : ''}`}
                    />
                  </button>
                  <a
                    href={updateState.downloadUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex h-7 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3 text-xs font-bold text-[#000000] whitespace-nowrap"
                  >
                    <Download className="h-3.5 w-3.5" />
                    Download
                  </a>
                </div>
              </div>

              {showChangelog && (
                <div className="mt-2.5 max-h-32 overflow-y-auto rounded border border-[#1F1F26] bg-[#000000] p-2.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-[#D8DCE6]">
                  {updateState.changelog}
                </div>
              )}
            </div>
          )}

          {updateState.status === 'error' && activeTab !== 'settings' && (
            <div className="mb-3.5 flex items-center justify-between gap-3 rounded-lg border border-[#FF668A]/50 bg-[#0B0B0E] px-3.5 py-2 text-xs text-[#FF668A]">
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
                  className="text-[11px] text-[#8E95A5] hover:text-white"
                >
                  Close
                </button>
              </div>
            </div>
          )}

          {/* TAB 1: HISTORY (Single Search + Collapsible Filters Popover + Streamlined Cards) */}
          {activeTab === 'history' && (
            <div className="space-y-4">
              {/* Clean Single-Row Toolbar */}
              <div className="flex flex-wrap items-center justify-between gap-2.5">
                <div className="flex flex-1 flex-wrap items-center gap-2">
                  <div className="relative min-w-[200px] flex-1 sm:max-w-[320px]">
                    <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[#5A6275]" />
                    <input
                      type="text"
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      placeholder="Search Riot ID, channel, map, server..."
                      className="h-8 w-full rounded-md border border-[#1F1F26] bg-[#0B0B0E] pr-3 pl-8 text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
                    />
                  </div>

                  {/* Anchored Collapsible Filters Popover */}
                  <div className="relative" ref={filterPopoverRef}>
                    <button
                      type="button"
                      onClick={() => setFiltersOpen((prev) => !prev)}
                      aria-expanded={filtersOpen}
                      className={`inline-flex h-8 items-center gap-1.5 rounded-md border px-3 text-xs font-medium transition-colors whitespace-nowrap ${
                        filtersOpen || activePopoverFilterCount > 0
                          ? 'border-[#FF5CA8]/60 bg-[#121216] text-[#FFF5FA]'
                          : 'border-[#1F1F26] bg-[#0B0B0E] text-[#8E95A5] hover:border-[#2B2B36] hover:text-white'
                      }`}
                    >
                      <SlidersHorizontal className="h-3.5 w-3.5 text-[#FFA6D2]" />
                      <span>
                        Filters
                        {activePopoverFilterCount > 0 ? ` (${activePopoverFilterCount})` : ''}
                      </span>
                    </button>

                    {filtersOpen && (
                      <div
                        role="dialog"
                        aria-label="History filters"
                        className="absolute top-10 left-0 z-30 w-[300px] rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-3.5 shadow-2xl"
                      >
                        <div className="space-y-3">
                          <div>
                            <label className="mb-1 block text-[11px] font-medium text-[#8E95A5]">
                              Team
                            </label>
                            <div className="grid grid-cols-3 gap-1 rounded-md border border-[#1F1F26] bg-[#000000] p-0.5">
                              {(['All', 'Ally', 'Enemy'] as const).map((f) => (
                                <button
                                  key={f}
                                  type="button"
                                  onClick={() => setTeamFilter(f)}
                                  className={`rounded py-1 text-xs font-medium transition-colors ${
                                    teamFilter === f
                                      ? 'bg-[#1A1A20] text-[#FFF5FA]'
                                      : 'text-[#8E95A5] hover:text-white'
                                  }`}
                                >
                                  {f}
                                </button>
                              ))}
                            </div>
                          </div>

                          <div className="grid grid-cols-2 gap-2.5">
                            <div>
                              <label className="mb-1 block text-[11px] font-medium text-[#8E95A5]">
                                Match Type
                              </label>
                              <select
                                aria-label="Filter by match type"
                                value={matchTypeFilter}
                                onChange={(e) => setMatchTypeFilter(e.target.value)}
                                className="w-full rounded border border-[#1F1F26] bg-[#000000] px-2 py-1.5 text-[11.5px] text-[#F4F4F6] focus:border-[#FF5CA8] focus:outline-none"
                              >
                                {availableMatchTypes.map((qt) => (
                                  <option key={qt} value={qt}>
                                    {qt}
                                  </option>
                                ))}
                              </select>
                            </div>

                            <div>
                              <label className="mb-1 block text-[11px] font-medium text-[#8E95A5]">
                                Server
                              </label>
                              <select
                                aria-label="Filter by server"
                                value={serverFilter}
                                onChange={(e) => setServerFilter(e.target.value)}
                                className="w-full rounded border border-[#1F1F26] bg-[#000000] px-2 py-1.5 text-[11.5px] text-[#F4F4F6] focus:border-[#FF5CA8] focus:outline-none"
                              >
                                {availableServers.map((srv) => (
                                  <option key={srv} value={srv}>
                                    {srv}
                                  </option>
                                ))}
                              </select>
                            </div>

                            <div>
                              <label className="mb-1 block text-[11px] font-medium text-[#8E95A5]">
                                Date Range
                              </label>
                              <select
                                aria-label="Filter by date range"
                                value={dateRangeFilter}
                                onChange={(e) => setDateRangeFilter(e.target.value as DateRangeFilter)}
                                className="w-full rounded border border-[#1F1F26] bg-[#000000] px-2 py-1.5 text-[11.5px] text-[#F4F4F6] focus:border-[#FF5CA8] focus:outline-none"
                              >
                                <option value="all">All Time</option>
                                <option value="today">Today</option>
                                <option value="7d">Last 7 Days</option>
                                <option value="30d">Last 30 Days</option>
                              </select>
                            </div>

                            <div>
                              <label className="mb-1 block text-[11px] font-medium text-[#8E95A5]">
                                Status
                              </label>
                              <select
                                aria-label="Filter by detection status"
                                value={statusFilter}
                                onChange={(e) => setStatusFilter(e.target.value as StatusFilter)}
                                className="w-full rounded border border-[#1F1F26] bg-[#000000] px-2 py-1.5 text-[11.5px] text-[#F4F4F6] focus:border-[#FF5CA8] focus:outline-none"
                              >
                                <option value="all">All</option>
                                <option value="Confirmed">Confirmed</option>
                                <option value="Probable">Probable</option>
                                <option value="Unverified">Unverified</option>
                                <option value="Chat">In-Game Chat</option>
                              </select>
                            </div>
                          </div>

                          <div className="flex items-center justify-between border-t border-[#1F1F26] pt-2.5">
                            <button
                              type="button"
                              onClick={resetHistoryFilters}
                              className="inline-flex items-center gap-1 text-[11px] font-medium text-[#8E95A5] hover:text-[#FFA6D2]"
                            >
                              <RotateCcw className="h-3 w-3" />
                              Reset
                            </button>
                            <button
                              type="button"
                              onClick={() => setFiltersOpen(false)}
                              className="rounded-md bg-[#121216] border border-[#1F1F26] px-3 py-1 text-[11.5px] font-semibold text-[#FFF5FA] hover:border-[#FF5CA8]/50"
                            >
                              Done
                            </button>
                          </div>
                        </div>
                      </div>
                    )}
                  </div>

                  {hasActiveFilters && (
                    <button
                      type="button"
                      onClick={resetHistoryFilters}
                      className="text-xs font-medium text-[#FFA6D2] hover:text-[#FFF5FA] whitespace-nowrap"
                    >
                      Reset
                    </button>
                  )}
                </div>

                <div className="flex items-center gap-2.5">
                  <span className="font-mono text-[11px] text-[#8E95A5] tabular-nums">
                    {filteredHistory.length} / {history.length}
                  </span>
                  {history.length > 0 && (
                    <button
                      type="button"
                      onClick={onClearHistory}
                      className="rounded-md border border-[#1F1F26] bg-[#0B0B0E] px-2.5 py-1 text-[11px] text-[#8E95A5] transition-colors hover:border-[#FF5CA8]/50 hover:text-white whitespace-nowrap"
                    >
                      Clear
                    </button>
                  )}
                </div>
              </div>

              {history.length === 0 && !lastScannedMatch ? (
                <div className="flex flex-col items-center justify-center py-16 text-center">
                  <h3 className="text-base font-bold text-[#F4F4F6]">
                    No Streamers Recorded Yet
                  </h3>
                  <p className="mt-1.5 max-w-md text-xs text-[#8E95A5]">
                    Play a match on any supported server, run a simulated match in Match Lab,
                    or click &ldquo;Add Streamer&rdquo; to register a Riot ID and channel.
                  </p>
                </div>
              ) : filteredHistory.length === 0 && hasActiveFilters ? (
                <div className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] py-14 text-center">
                  <p className="text-sm font-semibold text-[#FFF5FA]">
                    No history records match your active filters
                  </p>
                  <p className="mt-1 text-xs text-[#8E95A5]">
                    Your saved match history ({history.length} records) is intact.
                  </p>
                  <button
                    type="button"
                    onClick={resetHistoryFilters}
                    className="mt-3.5 inline-flex items-center gap-1.5 rounded-md border border-[#FF5CA8]/50 bg-[#121216] px-3.5 py-1.5 text-xs font-semibold text-[#FFF5FA] hover:bg-[#1A1A20]"
                  >
                    <RotateCcw className="h-3.5 w-3.5 text-[#FFA6D2]" />
                    Reset Filters
                  </button>
                </div>
              ) : (
                <div className="space-y-5">
                  {/* SUBSECTION 1: LATEST MATCH (quiet unboxed summary bar) */}
                  {lastScannedMatch && (
                    <section aria-label="Latest Match">
                      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                          <span className="inline-block h-1.5 w-1.5 rounded-full bg-[#FF5CA8]" />
                          <h3 className="text-[11px] font-bold tracking-wider text-[#FFA6D2] uppercase">
                            Latest Match
                          </h3>
                          <span className="text-[#1F1F26]">/</span>
                          <span className="font-semibold text-[#FFF5FA]">
                            {lastScannedMatch.queue_name}
                          </span>
                          <span className="text-[#F4F4F6]">{lastScannedMatch.map_name}</span>
                          {lastScannedMatch.server_name && (
                            <>
                              <span className="text-[#1F1F26]">·</span>
                              <span className="text-[#2DD4BF]">
                                {lastScannedMatch.server_name}
                              </span>
                            </>
                          )}
                          <span className="text-[#1F1F26]">·</span>
                          <span className="font-mono text-[11px] text-[#8E95A5] tabular-nums">
                            {lastScannedMatch.players_checked} scanned
                            {lastScannedMatch.party_skipped > 0
                              ? `, ${lastScannedMatch.party_skipped} party skipped`
                              : ''}
                          </span>
                          <span className="text-[#1F1F26]">·</span>
                          <span
                            className={`font-mono text-[11px] font-semibold tabular-nums ${
                              lastScannedMatch.streamers_found > 0
                                ? 'text-[#22C55E]'
                                : 'text-[#8E95A5]'
                            }`}
                          >
                            {lastScannedMatch.streamers_found} detected
                          </span>
                        </div>
                        <span className="font-mono text-[11px] text-[#5A6275] tabular-nums">
                          {lastScannedMatch.scanned_at}
                        </span>
                      </div>

                      {latestMatchEntries.length > 0 ? (
                        <div className="space-y-2">
                          {latestMatchEntries.map((entry) => renderHistoryCard(entry))}
                        </div>
                      ) : (
                        <div className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] px-4 py-3 text-xs text-[#8E95A5]">
                          {hasActiveFilters
                            ? 'Latest match entries are hidden by your active filters.'
                            : 'No live streamers were detected in your latest match.'}
                        </div>
                      )}
                    </section>
                  )}

                  {/* SUBSECTION 2: PREVIOUS HISTORY */}
                  {previousHistoryEntries.length > 0 && (
                    <section aria-label="Previous History" className="border-t border-[#1F1F26] pt-4">
                      <div className="mb-2 flex items-center justify-between">
                        <h3 className="text-[11px] font-bold tracking-wider text-[#8E95A5] uppercase">
                          Previous History ({previousHistoryEntries.length})
                        </h3>
                        <span className="font-mono text-[10.5px] text-[#5A6275]">
                          Most recent first
                        </span>
                      </div>
                      <div className="space-y-2">
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
              <div className="flex flex-wrap items-center justify-between gap-2.5">
                <div className="flex flex-wrap items-center gap-2">
                  <div className="flex items-center rounded-md border border-[#1F1F26] bg-[#0B0B0E] p-0.5">
                    <button
                      type="button"
                      onClick={() => setDirSource('custom')}
                      className={`rounded px-2.5 py-1 text-xs font-medium transition-colors whitespace-nowrap tabular-nums ${
                        dirSource === 'custom'
                          ? 'bg-[#1A1A20] text-[#FFF5FA]'
                          : 'text-[#8E95A5] hover:text-white'
                      }`}
                    >
                      Custom ({customDirectory.length})
                    </button>
                    <button
                      type="button"
                      onClick={() => setDirSource('community')}
                      className={`rounded px-2.5 py-1 text-xs font-medium transition-colors whitespace-nowrap tabular-nums ${
                        dirSource === 'community'
                          ? 'bg-[#1A1A20] text-[#FFF5FA]'
                          : 'text-[#8E95A5] hover:text-white'
                      }`}
                    >
                      Community ({communityDirectory.length})
                    </button>
                  </div>

                  <div className="relative w-56">
                    <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[#5A6275]" />
                    <input
                      type="text"
                      value={dirSearch}
                      onChange={(e) => setDirSearch(e.target.value)}
                      placeholder="Filter Riot ID, alias, channel..."
                      className="h-8 w-full rounded-md border border-[#1F1F26] bg-[#0B0B0E] pr-3 pl-8 text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
                    />
                  </div>
                </div>

                <button
                  type="button"
                  disabled={isSyncingDirectory}
                  onClick={onSyncDirectory}
                  className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#121216] px-3 text-xs font-medium text-[#F4F4F6] transition-colors hover:border-[#FF5CA8]/50 hover:bg-[#1A1A20] disabled:opacity-50 whitespace-nowrap"
                >
                  <RefreshCw
                    className={`h-3.5 w-3.5 ${isSyncingDirectory ? 'animate-spin' : ''}`}
                  />
                  {isSyncingDirectory ? 'Syncing...' : 'Sync Database'}
                </button>
              </div>

              {filteredDirectory.length === 0 ? (
                <div className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] py-12 text-center text-xs text-[#8E95A5]">
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
                        className="flex flex-col justify-between gap-2.5 rounded-lg border border-[#1F1F26] bg-[#0B0B0E] px-4 py-3 sm:flex-row sm:items-center"
                      >
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs">
                            <span className="font-mono text-[13.5px] font-semibold text-[#FFF5FA]">
                              {d.riot_id}
                            </span>
                            <span className="text-[#1F1F26]">/</span>
                            <span className="font-semibold" style={{ color: pColor }}>
                              {d.platform}
                            </span>
                            <span className="text-[#F4F4F6]">{d.channel_name}</span>
                            {regShort && (
                              <>
                                <span className="text-[#1F1F26]">/</span>
                                <span className="text-[11px] font-medium text-[#2DD4BF]">
                                  {regShort}
                                </span>
                              </>
                            )}
                            <span className="text-[#1F1F26]">/</span>
                            <span
                              className={`text-[11px] font-medium ${
                                d.verified ? 'text-[#22C55E]' : 'text-[#FB923C]'
                              }`}
                            >
                              {d.verified ? 'Verified' : 'Unverified'}
                            </span>
                          </div>
                          {d.aliases && d.aliases.length > 0 && (
                            <p className="mt-0.5 font-mono text-[10.5px] text-[#FFA6D2]">
                              Aliases: {d.aliases.join(', ')}
                            </p>
                          )}
                          <p className="mt-0.5 truncate font-mono text-[11px] text-[#8E95A5]">
                            {d.channel_url}
                          </p>
                        </div>

                        <div className="flex shrink-0 items-center gap-1.5">
                          {dirSource === 'custom' && (
                            <>
                              <button
                                type="button"
                                onClick={() => onOpenAddModal(d)}
                                className="inline-flex h-7 items-center gap-1 rounded border border-[#1F1F26] bg-[#121216] px-2.5 text-xs text-[#F4F4F6] hover:bg-[#1A1A20]"
                              >
                                <Pencil className="h-3 w-3" />
                                Edit
                              </button>
                              <button
                                type="button"
                                onClick={() => onDeleteCustomEntry(d.riot_id, d.platform)}
                                className="inline-flex h-7 items-center gap-1 rounded border border-[#1F1F26] px-2.5 text-xs text-[#FF668A] hover:border-[#FF668A]"
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
                            className="inline-flex h-7 items-center gap-1 rounded border border-[#1F1F26] bg-[#121216] px-2.5 text-xs text-[#F4F4F6] hover:bg-[#1A1A20]"
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

              {/* Collapsible Security / Heuristic Verifier */}
              <div className="border-t border-[#1F1F26] pt-3">
                <button
                  type="button"
                  onClick={() => setShowSanitizerTools((prev) => !prev)}
                  className="inline-flex items-center gap-1.5 text-xs font-medium text-[#8E95A5] hover:text-[#FFF5FA]"
                >
                  <ShieldCheck className="h-3.5 w-3.5 text-[#22C55E]" />
                  <span>URL &amp; IGN Heuristic Verifier</span>
                  <ChevronDown
                    className={`h-3.5 w-3.5 transition-transform ${
                      showSanitizerTools ? 'rotate-180' : ''
                    }`}
                  />
                </button>

                {showSanitizerTools && (
                  <div className="mt-2.5 grid gap-3 md:grid-cols-2">
                    <div className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-3">
                      <label className="block text-[11px] font-medium text-[#8E95A5]">
                        Test Rust <span className="font-mono text-[#F4F4F6]">clean_ign()</span>
                      </label>
                      <input
                        type="text"
                        value={testIgn}
                        onChange={(e) => setTestIgn(e.target.value)}
                        placeholder="Try Desktop, Alive, RohanYT, Vex_TV"
                        className="mt-1.5 w-full rounded border border-[#1F1F26] bg-[#000000] px-2.5 py-1.5 font-mono text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                      />
                      <p className="mt-2 font-mono text-[11px] text-[#8E95A5]">
                        Cleaned:{' '}
                        <span className="text-[#F4F4F6]">&ldquo;{cleanedIgnResult}&rdquo;</span>
                        {' · '}
                        {cleanedIgnResult.replace(/[^a-zA-Z0-9]/g, '').length >= 4 ? (
                          <span className="text-[#22C55E]">Eligible (&ge;4 alnum)</span>
                        ) : (
                          <span className="text-[#FF668A]">Skipped (&lt;4 alnum)</span>
                        )}
                      </p>
                    </div>

                    <div className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-3">
                      <div className="flex items-center justify-between gap-2">
                        <label className="text-[11px] font-medium text-[#8E95A5]">
                          Test Rust <span className="font-mono text-[#F4F4F6]">host_allowed()</span>
                        </label>
                        <select
                          value={testPlatform}
                          onChange={(e) => setTestPlatform(e.target.value as Platform)}
                          className="rounded border border-[#1F1F26] bg-[#000000] px-2 py-0.5 text-[11px] text-white focus:outline-none"
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
                        className="mt-1.5 w-full rounded border border-[#1F1F26] bg-[#000000] px-2.5 py-1.5 font-mono text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                      />
                      <p className="mt-2 font-mono text-[11px]">
                        {testedHostOk ? (
                          <span className="text-[#22C55E]">Allowed host: {testedHost}</span>
                        ) : (
                          <span className="text-[#FF668A]">
                            Blocked untrusted URL (
                            {testedHost ? `host "${testedHost}" mismatch` : 'invalid scheme/userinfo/port'})
                          </span>
                        )}
                      </p>
                    </div>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* TAB 3: DISCOVER */}
          {activeTab === 'discover' && (
            <div>
              {!config.enable_recommendations ? (
                <div className="mx-auto my-8 max-w-xl rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-5">
                  <div className="flex items-start gap-3">
                    <div className="rounded-md border border-[#1F1F26] bg-[#000000] p-2.5">
                      <Radio className="h-5 w-5 text-[#FF5CA8]" />
                    </div>
                    <div>
                      <h3 className="text-sm font-semibold text-[#FFF5FA]">
                        Live Streamer Recommendations are OFF
                      </h3>
                      <p className="mt-1 text-xs leading-relaxed text-[#8E95A5]">
                        Disabled by default to conserve bandwidth and API quota. Enabling fetches
                        cached live Valorant creators near{' '}
                        <span className="font-semibold text-[#2DD4BF]">{effectiveRegion.label}</span>.
                      </p>
                    </div>
                  </div>
                  <div className="mt-4">
                    <ToggleSettingRow
                      checked={false}
                      onChange={(next) => {
                        onToggleRecommendationsImmediate(next);
                        if (next) handleRefreshRecs(true);
                      }}
                      title="Enable Optional Streamer Recommendations"
                      description="Discovery suggestions only — never treated as lobby detections."
                    />
                  </div>
                </div>
              ) : (
                <div className="space-y-3.5">
                  <div className="flex flex-wrap items-center justify-between gap-2.5">
                    <div className="flex flex-wrap items-center gap-2.5">
                      <div className="flex items-center rounded-md border border-[#1F1F26] bg-[#0B0B0E] p-0.5">
                        {(['All', 'Twitch', 'YouTube'] as const).map((plat) => (
                          <button
                            key={plat}
                            type="button"
                            onClick={() => setDiscoverPlatform(plat)}
                            className={`rounded px-2.5 py-1 text-xs font-medium transition-colors whitespace-nowrap ${
                              discoverPlatform === plat
                                ? 'bg-[#1A1A20] text-[#FFF5FA]'
                                : 'text-[#8E95A5] hover:text-white'
                            }`}
                          >
                            {plat === 'All' ? 'All' : plat}
                          </button>
                        ))}
                      </div>
                      <span className="font-mono text-[11px] text-[#2DD4BF]">
                        {effectiveRegion.shortLabel}
                      </span>
                      <span className="text-[#1F1F26]">·</span>
                      <span className="font-mono text-[11px] text-[#8E95A5]">
                        Updated {recsUpdatedAt}
                      </span>
                    </div>

                    <div className="flex items-center gap-3">
                      <button
                        type="button"
                        disabled={isRefreshingRecs}
                        onClick={() => handleRefreshRecs(true)}
                        className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#121216] px-3 text-xs font-medium text-[#F4F4F6] hover:border-[#FF5CA8]/50 disabled:opacity-60 whitespace-nowrap"
                      >
                        <RefreshCw className={`h-3.5 w-3.5 ${isRefreshingRecs ? 'animate-spin' : ''}`} />
                        {isRefreshingRecs ? 'Refreshing...' : 'Refresh'}
                      </button>
                      <ToggleSwitch
                        checked={true}
                        onChange={(next) => onToggleRecommendationsImmediate(next)}
                        ariaLabel="Toggle Live Streamer Recommendations"
                      />
                    </div>
                  </div>

                  <div className="flex items-center gap-2 text-[11px] text-[#8E95A5]">
                    <Info className="h-3.5 w-3.5 shrink-0 text-[#38BDF8]" />
                    <span>
                      Discovery suggestions near {effectiveRegion.shortLabel} — not matched to your lobby.
                    </span>
                  </div>

                  <div className="space-y-2">
                    {visibleRecommendations.map((rec) => {
                      const pColor = platformColor(rec.platform);
                      return (
                        <div
                          key={rec.id}
                          className="flex flex-col justify-between gap-3 rounded-lg border border-[#1F1F26] bg-[#0B0B0E] px-4 py-3 sm:flex-row sm:items-center"
                        >
                          <div className="min-w-0 flex-1">
                            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs">
                              <span className="inline-block h-1.5 w-1.5 rounded-full bg-[#22C55E]" />
                              <span className="text-[13.5px] font-bold text-[#FFF5FA]">
                                {rec.channel}
                              </span>
                              <span className="text-[#1F1F26]">/</span>
                              <span className="font-semibold" style={{ color: pColor }}>
                                {rec.platform}
                              </span>
                              <span className="text-[#1F1F26]">/</span>
                              <span className="font-mono text-[11px] font-medium text-[#22C55E] tabular-nums">
                                {formatViewers(rec.viewers)}
                              </span>
                              <span className="text-[11px] text-[#8E95A5]">
                                · {rec.region_label}
                              </span>
                            </div>
                            <p className="mt-1 truncate text-xs text-[#D8DCE6]">
                              {rec.title}
                            </p>
                          </div>

                          <div className="flex shrink-0 items-center gap-1.5">
                            <button
                              type="button"
                              onClick={() => onOpenAddFromRecommendation(rec)}
                              className="inline-flex h-8 items-center gap-1 rounded-md border border-[#1F1F26] bg-[#121216] px-2.5 text-xs font-medium text-[#F4F4F6] hover:border-[#FF5CA8]/50 hover:bg-[#1A1A20] whitespace-nowrap"
                            >
                              <Plus className="h-3.5 w-3.5 text-[#FFA6D2]" />
                              Save
                            </button>
                            <a
                              href={rec.url}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3 text-xs font-bold text-[#000000] whitespace-nowrap"
                            >
                              Watch
                              <ExternalLink className="h-3 w-3" />
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
              {/* Valorant Servers & Detection (clean dropdown, no redundant chip wall) */}
              <section className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-4">
                <h3 className="text-sm font-semibold text-[#FFF5FA]">
                  Valorant Server
                </h3>
                <p className="mt-0.5 text-xs text-[#8E95A5]">
                  Prioritizes your actual in-game Valorant server to prevent cross-server false positives.
                </p>

                <div className="mt-3">
                  <select
                    aria-label="Active Valorant Server"
                    value={draft.server_override}
                    onChange={(e) =>
                      setDraft({
                        ...draft,
                        server_override: e.target.value as ServerRegionOverride,
                      })
                    }
                    className="w-full max-w-md rounded-md border border-[#1F1F26] bg-[#000000] px-3 py-2 text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
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
                </div>

                <p className="mt-2.5 font-mono text-[11px] text-[#2DD4BF]">
                  Detected: {detectedRegionInfo.label} ({detectedRegionInfo.podExample})
                </p>
              </section>

              {/* Features & Windows Integration */}
              <section className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-4">
                <h3 className="text-sm font-semibold text-[#FFF5FA]">
                  Features &amp; Windows Integration
                </h3>
                <div className="mt-3 space-y-2">
                  <ToggleSettingRow
                    checked={draft.skip_party_members}
                    onChange={(next) =>
                      setDraft({ ...draft, skip_party_members: next })
                    }
                    title="Skip Party Members (Save API Calls)"
                    description="Excludes your party teammates from external YouTube/Twitch/Kick lookups using Riot's partyId."
                  />
                  <ToggleSettingRow
                    checked={draft.enable_recommendations}
                    onChange={(next) =>
                      setDraft({ ...draft, enable_recommendations: next })
                    }
                    title="Live Streamer Recommendations"
                    description="OFF by default to conserve bandwidth. Unlocks cached regional suggestions in Discover."
                  />
                  <ToggleSettingRow
                    checked={draft.capture_in_game_chat}
                    onChange={(next) =>
                      setDraft({ ...draft, capture_in_game_chat: next })
                    }
                    title="Post-Match In-Game Chat Stream Plugs"
                    description={
                      <>
                        Reads post-match chat for plugs like{' '}
                        <span className="font-mono text-[#FFA6D2]">&ldquo;im live on yt&rdquo;</span>{' '}
                        or dropped stream links.
                      </>
                    }
                  />
                  <ToggleSettingRow
                    checked={draft.start_with_windows}
                    onChange={(next) =>
                      setDraft({ ...draft, start_with_windows: next })
                    }
                    title="Start Silently with Windows"
                    description="Launches hidden in the Windows system tray at logon."
                  />
                  <ToggleSettingRow
                    checked={draft.use_custom_toast_app_id}
                    onChange={(next) =>
                      setDraft({ ...draft, use_custom_toast_app_id: next })
                    }
                    title="Custom Windows Toast App ID"
                    description="Shows 'Valorant Streamer Radar' as the sender on Windows desktop notifications."
                  />
                </div>
              </section>

              {/* Optional Platform API Keys */}
              <section className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-4">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <h3 className="text-sm font-semibold text-[#FFF5FA]">
                      Optional Platform API Keys
                    </h3>
                    <p className="mt-0.5 text-xs text-[#8E95A5]">
                      Leave blank to run keyless, or add free keys for faster official API checks.
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-[11px] text-[#8E95A5]">Show keys</span>
                    <ToggleSwitch
                      checked={showSecrets}
                      onChange={setShowSecrets}
                      ariaLabel="Show API keys"
                      showStateText={false}
                    />
                  </div>
                </div>

                <div className="mt-3.5 space-y-3.5">
                  <div>
                    <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2">
                      <label className="text-xs font-medium text-[#F4F4F6]">
                        YouTube Data API v3 Key
                      </label>
                      <div className="flex items-center gap-1.5">
                        <a
                          href="https://console.cloud.google.com/apis/credentials"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="inline-flex items-center gap-1 rounded border border-[#1F1F26] bg-[#121216] px-2.5 py-1 text-[11px] font-medium text-[#FFA6D2] hover:border-[#FF5CA8]/50 whitespace-nowrap"
                        >
                          Get Key
                          <ExternalLink className="h-3 w-3" />
                        </a>
                        <button
                          type="button"
                          onClick={handleTestYoutubeKey}
                          className="rounded border border-[#1F1F26] bg-[#121216] px-2.5 py-1 text-[11px] font-medium text-[#F4F4F6] hover:bg-[#1A1A20] whitespace-nowrap"
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
                      placeholder="AIza... (optional)"
                      className="w-full rounded border border-[#1F1F26] bg-[#000000] px-3 py-1.5 font-mono text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
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

                  <div>
                    <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2">
                      <span className="text-xs font-medium text-[#F4F4F6]">
                        Twitch Helix API (Client ID &amp; Secret)
                      </span>
                      <div className="flex items-center gap-1.5">
                        <a
                          href="https://dev.twitch.tv/console/apps"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="inline-flex items-center gap-1 rounded border border-[#1F1F26] bg-[#121216] px-2.5 py-1 text-[11px] font-medium text-[#A770FF] hover:border-[#A770FF]/60 whitespace-nowrap"
                        >
                          Get Keys
                          <ExternalLink className="h-3 w-3" />
                        </a>
                        <button
                          type="button"
                          onClick={handleTestTwitchKeys}
                          className="rounded border border-[#1F1F26] bg-[#121216] px-2.5 py-1 text-[11px] font-medium text-[#F4F4F6] hover:bg-[#1A1A20] whitespace-nowrap"
                        >
                          Test Twitch
                        </button>
                      </div>
                    </div>
                    <div className="grid gap-2.5 sm:grid-cols-2">
                      <div>
                        <label className="mb-1 block text-[11px] text-[#8E95A5]">
                          Client ID
                        </label>
                        <input
                          type={showSecrets ? 'text' : 'password'}
                          value={draft.twitch_client_id}
                          onChange={(e) => {
                            setDraft({ ...draft, twitch_client_id: e.target.value });
                            setTwitchKeyStatus(null);
                          }}
                          placeholder="Optional Client ID"
                          className="w-full rounded border border-[#1F1F26] bg-[#000000] px-3 py-1.5 font-mono text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
                        />
                      </div>
                      <div>
                        <label className="mb-1 block text-[11px] text-[#8E95A5]">
                          Client Secret
                        </label>
                        <input
                          type={showSecrets ? 'text' : 'password'}
                          value={draft.twitch_client_secret}
                          onChange={(e) => {
                            setDraft({ ...draft, twitch_client_secret: e.target.value });
                            setTwitchKeyStatus(null);
                          }}
                          placeholder="Optional Client Secret"
                          className="w-full rounded border border-[#1F1F26] bg-[#000000] px-3 py-1.5 font-mono text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
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
                  <div className="rounded-md border border-[#1F1F26] bg-[#000000]">
                    <button
                      type="button"
                      onClick={() => setShowApiGuide((prev) => !prev)}
                      className="flex w-full items-center justify-between px-3 py-2 text-left text-xs font-medium text-[#FFA6D2] transition-colors hover:text-[#FFF5FA]"
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
                      <div className="space-y-3 border-t border-[#1F1F26] px-3.5 py-3 text-[11.5px] leading-relaxed text-[#F4F4F6]">
                        <div>
                          <p className="font-semibold text-[#FF5CA8]">
                            YouTube Data API v3 (Free — 10,000 units/day)
                          </p>
                          <ol className="mt-1 list-decimal space-y-0.5 pl-4 text-[#D8DCE6]">
                            <li>Click <strong className="text-white">Get Key</strong> above to open Google Cloud Console.</li>
                            <li>Create a new project (e.g. <span className="font-mono text-white">Valorant-Radar</span>).</li>
                            <li>Enable <span className="font-mono text-white">YouTube Data API v3</span> under Library.</li>
                            <li>Create an API key under <strong className="text-white">Credentials</strong> and paste it above.</li>
                          </ol>
                        </div>
                        <div>
                          <p className="font-semibold text-[#A770FF]">
                            Twitch Helix API (Free — requires 2FA)
                          </p>
                          <ol className="mt-1 list-decimal space-y-0.5 pl-4 text-[#D8DCE6]">
                            <li>Click <strong className="text-white">Get Keys</strong> above to open Twitch Developer Console.</li>
                            <li>Register an application with redirect URL <span className="font-mono text-white">http://localhost</span>.</li>
                            <li>Copy <strong className="text-white">Client ID</strong> and generate a <strong className="text-white">Client Secret</strong>.</li>
                          </ol>
                        </div>
                        <p className="flex items-center gap-1.5 text-[11px] text-[#8E95A5]">
                          <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-[#FB923C]" />
                          Kick needs no API key. Leaving all keys blank still works keyless.
                        </p>
                      </div>
                    )}
                  </div>
                </div>
              </section>

              {/* App Updates & Version */}
              <section className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-4">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <h3 className="text-sm font-semibold text-[#FFF5FA]">
                      App Updates
                    </h3>
                    <p className="mt-0.5 font-mono text-[11px] text-[#8E95A5]">
                      Installed v{APP_VERSION} · Checks GitHub Releases automatically
                    </p>
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      disabled={updateState.status === 'checking'}
                      onClick={() => handleCheckForUpdates(false)}
                      className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#121216] px-3 text-xs font-medium text-[#F4F4F6] hover:border-[#FF5CA8]/50 disabled:opacity-60 whitespace-nowrap"
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
                      className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#000000] px-3 text-xs font-medium text-[#8E95A5] hover:text-white whitespace-nowrap"
                    >
                      Releases
                      <ExternalLink className="h-3 w-3" />
                    </a>
                  </div>
                </div>

                {updateState.status === 'up_to_date' && (
                  <p className="mt-2.5 flex items-center gap-1.5 text-xs text-[#22C55E]">
                    <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />
                    {updateState.message}
                  </p>
                )}
                {updateState.status === 'error' && (
                  <p className="mt-2.5 flex items-center gap-1.5 text-xs text-[#FF668A]">
                    <XCircle className="h-3.5 w-3.5 shrink-0" />
                    {updateState.message}
                  </p>
                )}
              </section>

              <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
                <span className="font-mono text-[11px] text-[#5A6275]">
                  %APPDATA%\ValorantStreamerRadar\config.json
                </span>
                <button
                  type="button"
                  onClick={handleSaveSettings}
                  className="rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-5 py-2 text-xs font-bold text-[#000000] transition-opacity whitespace-nowrap"
                >
                  Save Settings
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
