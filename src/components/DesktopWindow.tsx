import React, { useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  ChevronDown,
  Copy,
  Eye,
  EyeOff,
  ExternalLink,
  KeyRound,
  Minus,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  ShieldCheck,
  Trash2,
  X,
  XCircle,
} from 'lucide-react';
import {
  AppConfig,
  DesktopTab,
  DirectoryStreamer,
  LastMatchSummary,
  Platform,
  SHARD_LABELS,
  ShardCode,
  ShardOverride,
  StreamerEntry,
} from '../types/radar';
import {
  cleanIgn,
  hostAllowed,
  hostOf,
  resolveEffectiveShard,
  validGithubSegment,
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
  detectedShard: ShardCode;
  config: AppConfig;
  history: StreamerEntry[];
  lastScannedMatch: LastMatchSummary | null;
  customDirectory: DirectoryStreamer[];
  communityDirectory: DirectoryStreamer[];
  toastMessage: string | null;
  onTriggerManualScan: () => void;
  onOpenAddModal: (editEntry?: DirectoryStreamer) => void;
  onDeleteCustomEntry: (riotId: string, platform: Platform) => void;
  onSyncDirectory: () => void;
  onClearHistory: () => void;
  onCopyLink: (url: string) => void;
  onWatchStream: (entry: StreamerEntry) => void;
  onSaveConfig: (nextConfig: AppConfig) => void;
  onHideToTray: () => void;
  onRestoreFromTray: () => void;
}

function platformColor(platform: Platform): string {
  switch (platform) {
    case 'Twitch':
      return '#9146FF';
    case 'Kick':
      return '#53FC18';
    default:
      return '#FF4655';
  }
}

const SHARD_OPTIONS: { code: ShardOverride; label: string }[] = [
  { code: 'auto', label: 'Auto-detect (ShooterGame.log)' },
  { code: 'ap', label: 'AP (Mumbai)' },
  { code: 'na', label: 'NA / LATAM / BR' },
  { code: 'eu', label: 'EU (Europe)' },
  { code: 'kr', label: 'KR (Korea)' },
];

export const DesktopWindow: React.FC<DesktopWindowProps> = ({
  visible,
  activeTab,
  onChangeTab,
  status,
  isScanning,
  isSyncingDirectory,
  valorantRunning,
  detectedShard,
  config,
  history,
  lastScannedMatch,
  customDirectory,
  communityDirectory,
  toastMessage,
  onTriggerManualScan,
  onOpenAddModal,
  onDeleteCustomEntry,
  onSyncDirectory,
  onClearHistory,
  onCopyLink,
  onWatchStream,
  onSaveConfig,
  onHideToTray,
  onRestoreFromTray,
}) => {
  // History state
  const [searchQuery, setSearchQuery] = useState('');
  const [teamFilter, setTeamFilter] = useState<'All' | 'Ally' | 'Enemy'>('All');

  // Directory state
  const [dirSource, setDirSource] = useState<'custom' | 'community'>('custom');
  const [dirSearch, setDirSearch] = useState('');
  const [testIgn, setTestIgn] = useState('Desktop');
  const [testUrl, setTestUrl] = useState('https://evil.example.com@youtube.com/@fake');
  const [testPlatform, setTestPlatform] = useState<Platform>('YouTube');

  // Settings draft state
  const [draft, setDraft] = useState<AppConfig>(config);
  const [showSecrets, setShowSecrets] = useState(false);
  const [showApiGuide, setShowApiGuide] = useState(false);
  const [ytKeyStatus, setYtKeyStatus] = useState<{ kind: 'ok' | 'err'; msg: string } | null>(null);
  const [twitchKeyStatus, setTwitchKeyStatus] = useState<{ kind: 'ok' | 'err'; msg: string } | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);

  useEffect(() => {
    setDraft(config);
  }, [config]);

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

  const effectiveShard = resolveEffectiveShard(config, detectedShard);

  const filteredHistory = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return history.filter((e) => {
      const teamOk =
        teamFilter === 'All' ||
        e.team_relation.toLowerCase() === teamFilter.toLowerCase();
      if (!teamOk) return false;
      if (!q) return true;
      return (
        e.riot_id.toLowerCase().includes(q) ||
        e.platform.toLowerCase().includes(q) ||
        e.channel.toLowerCase().includes(q) ||
        e.title.toLowerCase().includes(q) ||
        e.map_name.toLowerCase().includes(q) ||
        e.agent_name.toLowerCase().includes(q)
      );
    });
  }, [history, searchQuery, teamFilter]);

  const filteredDirectory = useMemo(() => {
    const list = dirSource === 'custom' ? customDirectory : communityDirectory;
    const q = dirSearch.trim().toLowerCase();
    if (!q) return list;
    return list.filter(
      (d) =>
        d.riot_id.toLowerCase().includes(q) ||
        d.channel_name.toLowerCase().includes(q) ||
        d.platform.toLowerCase().includes(q)
    );
  }, [dirSource, customDirectory, communityDirectory, dirSearch]);

  const cleanedIgnResult = cleanIgn(testIgn);
  const testedHost = hostOf(testUrl.trim());
  const testedHostOk = testedHost ? hostAllowed(testPlatform, testedHost) : false;

  const handleSaveSettings = () => {
    if (!validGithubSegment(draft.github_owner) || !validGithubSegment(draft.github_repo)) {
      setSettingsError('Enter a valid GitHub owner and repository name.');
      return;
    }
    setSettingsError(null);
    onSaveConfig({
      ...draft,
      github_owner: draft.github_owner.trim(),
      github_repo: draft.github_repo.trim(),
      youtube_api_key: draft.youtube_api_key.trim(),
      twitch_client_id: draft.twitch_client_id.trim(),
      twitch_client_secret: draft.twitch_client_secret.trim(),
    });
  };

  if (!visible) {
    return (
      <div className="flex min-h-[540px] flex-col items-center justify-center rounded-lg border border-dashed border-[#2B3A48] bg-[#0F1923]/60 px-6 py-16 text-center">
        <RadarIcon size={40} />
        <h3 className="mt-4 text-base font-semibold text-white">
          Window Hidden in Windows System Tray
        </h3>
        <p className="mt-1.5 max-w-md text-xs leading-relaxed text-[#94A3B8]">
          Closing the window intercepts the close event and sets{' '}
          <span className="font-mono text-[#ECE8E1]">WINDOW_VISIBLE = false</span>.
          The UI loop schedules zero repaints (0% idle CPU) while the background{' '}
          <span className="font-mono text-[#ECE8E1]">radar-bg</span> thread continues
          tailing <span className="font-mono text-[#ECE8E1]">ShooterGame.log</span>.
        </p>
        <button
          type="button"
          onClick={onRestoreFromTray}
          className="mt-5 rounded-md bg-[#FF4655] px-4 py-2 text-xs font-semibold text-white transition-colors hover:bg-[#e53a49]"
        >
          Open Streamer History (Left-Click Tray)
        </button>
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-lg border border-[#2B3A48] bg-[#0F1923] shadow-2xl">
      {/* Native Windows Title Bar */}
      <div className="flex items-center justify-between border-b border-[#2B3A48] bg-[#080E14] px-3.5 py-2 select-none">
        <div className="flex items-center gap-2">
          <RadarIcon size={16} />
          <span className="text-xs font-medium text-[#ECE8E1]">
            Valorant Streamer Radar
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={onHideToTray}
            title="Minimize to tray"
            className="rounded p-1 text-[#94A3B8] transition-colors hover:bg-[#182430] hover:text-white"
          >
            <Minus className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            onClick={onHideToTray}
            title="Close (intercepted -> hide to system tray)"
            className="rounded p-1 text-[#94A3B8] transition-colors hover:bg-[#FF4655] hover:text-white"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      {/* Header Panel (#0B131B) */}
      <div className="border-b border-[#2B3A48] bg-[#0B131B] px-6 py-4">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="min-w-0">
            <div className="flex items-baseline gap-2">
              <span
                className="text-2xl font-bold tracking-wide text-[#FF4655]"
                style={{ fontFamily: 'var(--font-display)' }}
              >
                VALORANT
              </span>
              <span
                className="text-2xl font-bold tracking-wide text-[#ECE8E1]"
                style={{ fontFamily: 'var(--font-display)' }}
              >
                STREAMER RADAR
              </span>
            </div>
            <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-[#94A3B8]">
              <span className="inline-flex items-center gap-1.5">
                <span
                  className="inline-block h-2 w-2 rounded-full"
                  style={{
                    backgroundColor: valorantRunning ? '#22C55E' : '#64748B',
                  }}
                />
                <span>
                  {valorantRunning
                    ? `Valorant Active (${SHARD_LABELS[effectiveShard]})`
                    : `Waiting for Valorant (${SHARD_LABELS[effectiveShard]})`}
                </span>
              </span>
              <span className="text-[#2B3A48]">|</span>
              <span className="truncate">{status}</span>
            </div>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            <button
              type="button"
              onClick={() => onOpenAddModal()}
              className="inline-flex h-9 items-center gap-1.5 rounded-md border border-[#2B3A48] bg-[#21303F] px-3.5 text-xs font-semibold text-[#ECE8E1] transition-colors hover:bg-[#2a3c4e] whitespace-nowrap"
            >
              <Plus className="h-3.5 w-3.5" />
              Add Streamer
            </button>
            <button
              type="button"
              disabled={isScanning}
              onClick={onTriggerManualScan}
              className={`inline-flex h-9 min-w-[138px] items-center justify-center rounded-md px-4 text-xs font-semibold text-white transition-colors whitespace-nowrap ${
                isScanning
                  ? 'cursor-not-allowed bg-[#2B3A48] text-[#94A3B8]'
                  : 'bg-[#FF4655] hover:bg-[#e53a49]'
              }`}
            >
              {isScanning ? 'Scanning...' : 'Check Last Match'}
            </button>
          </div>
        </div>

        {/* Desktop App Tab Switcher + Auto-Expiring Toast */}
        <div className="mt-3.5 flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-1.5">
            {(
              [
                ['history', `History (${history.length})`],
                [
                  'directory',
                  `Directory (${customDirectory.length + communityDirectory.length})`,
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
                      ? 'border-[#FF4655] bg-[#21303F] text-white'
                      : 'border-transparent text-[#94A3B8] hover:text-white'
                  }`}
                >
                  {label}
                </button>
              );
            })}
          </div>

          {toastMessage && (
            <span className="inline-flex items-center gap-1.5 text-xs font-medium text-[#22C55E]">
              <Check className="h-3.5 w-3.5" />
              {toastMessage}
            </span>
          )}
        </div>
      </div>

      {/* Main Panel (#0F1923) */}
      <div className="min-h-[440px] max-h-[600px] overflow-y-auto px-6 py-4">
        {/* TAB 1: HISTORY */}
        {activeTab === 'history' && (
          <div>
            {lastScannedMatch && (
              <div className="mb-3.5 flex flex-wrap items-center justify-between gap-2 rounded-md border border-[#2B3A48] bg-[#0B131B] px-3.5 py-2.5 text-xs">
                <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
                  <span className="text-[10px] font-bold tracking-wider text-[#94A3B8]">
                    LAST SCANNED MATCH
                  </span>
                  <span
                    className="rounded border px-1.5 py-0.5 text-[11px] font-semibold"
                    style={{
                      color:
                        lastScannedMatch.queue_name === 'Competitive' ||
                        lastScannedMatch.queue_name === 'Premier'
                          ? '#FF4655'
                          : lastScannedMatch.queue_name.includes('Deathmatch')
                          ? '#2DD4BF'
                          : lastScannedMatch.queue_name === 'Swiftplay' ||
                            lastScannedMatch.queue_name === 'Spike Rush'
                          ? '#FB923C'
                          : '#ECE8E1',
                      borderColor:
                        lastScannedMatch.queue_name === 'Competitive' ||
                        lastScannedMatch.queue_name === 'Premier'
                          ? '#FF4655'
                          : lastScannedMatch.queue_name.includes('Deathmatch')
                          ? '#2DD4BF'
                          : lastScannedMatch.queue_name === 'Swiftplay' ||
                            lastScannedMatch.queue_name === 'Spike Rush'
                          ? '#FB923C'
                          : '#2B3A48',
                    }}
                  >
                    {lastScannedMatch.queue_name}
                  </span>
                  <span className="font-semibold text-white">
                    {lastScannedMatch.map_name}
                  </span>
                  {lastScannedMatch.my_agent && (
                    <>
                      <span className="text-[#2B3A48]">|</span>
                      <span className="text-[#ECE8E1]">
                        Agent: {lastScannedMatch.my_agent}
                      </span>
                    </>
                  )}
                  <span className="text-[#2B3A48]">|</span>
                  <span className="font-mono text-[#94A3B8] tabular-nums">
                    {lastScannedMatch.players_checked} players checked
                  </span>
                  <span className="text-[#2B3A48]">|</span>
                  <span
                    className={
                      lastScannedMatch.streamers_found > 0
                        ? 'font-semibold text-[#22C55E]'
                        : 'text-[#94A3B8]'
                    }
                  >
                    {lastScannedMatch.streamers_found > 0
                      ? `${lastScannedMatch.streamers_found} live found`
                      : '0 live'}
                  </span>
                </div>
                <span className="font-mono text-[11px] text-[#94A3B8] tabular-nums">
                  {lastScannedMatch.scanned_at}
                </span>
              </div>
            )}

            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex flex-wrap items-center gap-2.5">
                <div className="relative w-72 sm:w-80">
                  <Search className="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[#64748B]" />
                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder="Search Riot ID, platform, channel, map, or title..."
                    className="w-full rounded border border-[#2B3A48] bg-[#182430] py-1.5 pr-3 pl-8 text-xs text-white placeholder-[#64748B] focus:border-[#FF4655] focus:outline-none"
                  />
                </div>

                <div className="flex items-center rounded-md border border-[#2B3A48] bg-[#0B131B] p-0.5">
                  {(['All', 'Ally', 'Enemy'] as const).map((f) => (
                    <button
                      key={f}
                      type="button"
                      onClick={() => setTeamFilter(f)}
                      className={`rounded px-2.5 py-1 text-xs font-medium transition-colors whitespace-nowrap ${
                        teamFilter === f
                          ? 'bg-[#21303F] text-white'
                          : 'text-[#94A3B8] hover:text-white'
                      }`}
                    >
                      {f}
                    </button>
                  ))}
                </div>

                <span className="font-mono text-xs text-[#94A3B8] tabular-nums">
                  {filteredHistory.length} detected
                </span>
              </div>

              {history.length > 0 && (
                <button
                  type="button"
                  onClick={onClearHistory}
                  className="rounded border border-[#2B3A48] px-3 py-1.5 text-xs text-[#94A3B8] transition-colors hover:border-[#FF4655] hover:text-white whitespace-nowrap"
                >
                  Clear History
                </button>
              )}
            </div>

            {filteredHistory.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-20 text-center">
                <h3 className="text-base font-bold text-[#ECE8E1]">
                  No Streamers Recorded Yet
                </h3>
                <p className="mt-1.5 max-w-md text-xs text-[#94A3B8]">
                  Play a match on any supported shard, run a simulated match below,
                  or click &ldquo;Add Streamer&rdquo; to register your Riot ID and channel.
                </p>
              </div>
            ) : (
              <div className="mt-4 space-y-2.5">
                {filteredHistory.map((entry) => {
                  const pColor = platformColor(entry.platform);
                  const metaItems = [
                    entry.map_name,
                    entry.agent_name,
                    entry.queue_name,
                    entry.timestamp,
                  ].filter(Boolean);

                  return (
                    <article
                      key={`${entry.match_id}-${entry.riot_id}-${entry.platform}`}
                      className="flex flex-col justify-between gap-3 rounded-lg border border-[#2B3A48] bg-[#182430] px-4 py-3.5 sm:flex-row sm:items-center"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
                          <span className="font-mono text-[15px] font-bold text-white">
                            {entry.riot_id}
                          </span>
                          <span
                            className="text-xs font-semibold"
                            style={{ color: pColor }}
                          >
                            {entry.platform}
                          </span>
                          <span className="text-[#2B3A48]" aria-hidden="true">
                            ·
                          </span>
                          <span
                            className="text-xs font-semibold"
                            style={{ color: pColor }}
                          >
                            {entry.channel}
                          </span>
                          {entry.team_relation && (
                            <>
                              <span className="text-[#2B3A48]" aria-hidden="true">
                                ·
                              </span>
                              <span
                                className="text-xs font-semibold"
                                style={{
                                  color:
                                    entry.team_relation === 'Ally'
                                      ? '#2DD4BF'
                                      : '#FB923C',
                                }}
                              >
                                {entry.team_relation}
                              </span>
                            </>
                          )}
                        </div>

                        <p className="mt-1 truncate text-[13px] text-[#ECE8E1]">
                          {entry.title}
                        </p>

                        <p className="mt-1.5 font-mono text-[11px] text-[#94A3B8] tabular-nums">
                          {metaItems.join('  ·  ')}
                        </p>
                      </div>

                      <div className="flex items-center gap-2 shrink-0">
                        <button
                          type="button"
                          onClick={() => onCopyLink(entry.url)}
                          className="inline-flex h-8 items-center gap-1.5 rounded-md border border-[#2B3A48] bg-[#21303F] px-3 text-xs font-medium text-[#ECE8E1] transition-colors hover:bg-[#2a3c4e] whitespace-nowrap"
                        >
                          <Copy className="h-3.5 w-3.5" />
                          Copy Link
                        </button>
                        <button
                          type="button"
                          onClick={() => onWatchStream(entry)}
                          title={entry.url}
                          className="inline-flex h-8 items-center gap-1.5 rounded-md bg-[#FF4655] px-3.5 text-xs font-semibold text-white transition-colors hover:bg-[#e53a49] whitespace-nowrap"
                        >
                          Watch Stream
                          <ExternalLink className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </article>
                  );
                })}
              </div>
            )}
          </div>
        )}

        {/* TAB 2: DIRECTORY MANAGER */}
        {activeTab === 'directory' && (
          <div className="space-y-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex flex-wrap items-center gap-2.5">
                <div className="flex items-center rounded-md border border-[#2B3A48] bg-[#0B131B] p-0.5">
                  <button
                    type="button"
                    onClick={() => setDirSource('custom')}
                    className={`rounded px-3 py-1 text-xs font-medium transition-colors whitespace-nowrap tabular-nums ${
                      dirSource === 'custom'
                        ? 'bg-[#21303F] text-white'
                        : 'text-[#94A3B8] hover:text-white'
                    }`}
                  >
                    My Custom Streamers ({customDirectory.length})
                  </button>
                  <button
                    type="button"
                    onClick={() => setDirSource('community')}
                    className={`rounded px-3 py-1 text-xs font-medium transition-colors whitespace-nowrap tabular-nums ${
                      dirSource === 'community'
                        ? 'bg-[#21303F] text-white'
                        : 'text-[#94A3B8] hover:text-white'
                    }`}
                  >
                    Community Cache ({communityDirectory.length})
                  </button>
                </div>

                <div className="relative w-56">
                  <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[#64748B]" />
                  <input
                    type="text"
                    value={dirSearch}
                    onChange={(e) => setDirSearch(e.target.value)}
                    placeholder="Filter Riot ID or channel..."
                    className="w-full rounded border border-[#2B3A48] bg-[#182430] py-1.5 pr-3 pl-8 text-xs text-white placeholder-[#64748B] focus:border-[#FF4655] focus:outline-none"
                  />
                </div>
              </div>

              <button
                type="button"
                disabled={isSyncingDirectory}
                onClick={onSyncDirectory}
                className="inline-flex items-center gap-1.5 rounded-md border border-[#2B3A48] bg-[#21303F] px-3 py-1.5 text-xs font-medium text-[#ECE8E1] transition-colors hover:bg-[#2a3c4e] disabled:opacity-50 whitespace-nowrap"
              >
                <RefreshCw
                  className={`h-3.5 w-3.5 ${isSyncingDirectory ? 'animate-spin' : ''}`}
                />
                {isSyncingDirectory ? 'Syncing...' : 'Sync GitHub'}
              </button>
            </div>

            {filteredDirectory.length === 0 ? (
              <div className="rounded-lg border border-[#2B3A48] bg-[#182430]/50 py-12 text-center text-xs text-[#94A3B8]">
                No entries match your filter.
              </div>
            ) : (
              <div className="space-y-2">
                {filteredDirectory.map((d) => {
                  const pColor = platformColor(d.platform);
                  return (
                    <div
                      key={`${d.riot_id}-${d.platform}-${d.channel_url}`}
                      className="flex flex-col justify-between gap-2.5 rounded-lg border border-[#2B3A48] bg-[#182430] px-4 py-3 sm:flex-row sm:items-center"
                    >
                      <div className="min-w-0">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-sm font-semibold text-white">
                            {d.riot_id}
                          </span>
                          <span className="text-xs font-semibold" style={{ color: pColor }}>
                            {d.platform}
                          </span>
                          <span className="text-[#2B3A48]">·</span>
                          <span className="text-xs text-[#ECE8E1]">{d.channel_name}</span>
                        </div>
                        <p className="mt-0.5 truncate font-mono text-[11px] text-[#94A3B8]">
                          {d.channel_url}
                        </p>
                      </div>

                      <div className="flex items-center gap-1.5 shrink-0">
                        {dirSource === 'custom' && (
                          <>
                            <button
                              type="button"
                              onClick={() => onOpenAddModal(d)}
                              className="inline-flex items-center gap-1 rounded border border-[#2B3A48] bg-[#21303F] px-2.5 py-1 text-xs text-[#ECE8E1] hover:bg-[#2a3c4e]"
                            >
                              <Pencil className="h-3 w-3" />
                              Edit
                            </button>
                            <button
                              type="button"
                              onClick={() => onDeleteCustomEntry(d.riot_id, d.platform)}
                              className="inline-flex items-center gap-1 rounded border border-[#2B3A48] px-2.5 py-1 text-xs text-[#FF4655] hover:border-[#FF4655]"
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
                          className="inline-flex items-center gap-1 rounded border border-[#2B3A48] bg-[#21303F] px-2.5 py-1 text-xs text-[#ECE8E1] hover:bg-[#2a3c4e]"
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
            <div className="border-t border-[#2B3A48] pt-4">
              <div className="flex items-center gap-2 text-xs font-semibold text-white">
                <ShieldCheck className="h-4 w-4 text-[#22C55E]" />
                <span>Untrusted Directory URL &amp; IGN Heuristic Verifier</span>
              </div>
              <div className="mt-2.5 grid gap-3 md:grid-cols-2">
                <div className="rounded-lg border border-[#2B3A48] bg-[#0B131B] p-3">
                  <label className="block text-[11px] font-medium text-[#94A3B8]">
                    Test Rust <span className="font-mono text-[#ECE8E1]">clean_ign()</span> suffix stripper
                  </label>
                  <input
                    type="text"
                    value={testIgn}
                    onChange={(e) => setTestIgn(e.target.value)}
                    placeholder="Try Desktop, Alive, RohanYT, Vex_TV"
                    className="mt-1.5 w-full rounded border border-[#2B3A48] bg-[#182430] px-2.5 py-1.5 font-mono text-xs text-white focus:border-[#FF4655] focus:outline-none"
                  />
                  <p className="mt-2 font-mono text-[11px] text-[#94A3B8]">
                    Cleaned:{' '}
                    <span className="text-[#ECE8E1]">&ldquo;{cleanedIgnResult}&rdquo;</span>
                    {' · '}
                    {cleanedIgnResult.replace(/[^a-zA-Z0-9]/g, '').length >= 4 ? (
                      <span className="text-[#22C55E]">Eligible (&ge;4 alnum)</span>
                    ) : (
                      <span className="text-[#FF4655]">Skipped (&lt;4 alnum)</span>
                    )}
                  </p>
                </div>

                <div className="rounded-lg border border-[#2B3A48] bg-[#0B131B] p-3">
                  <div className="flex items-center justify-between gap-2">
                    <label className="text-[11px] font-medium text-[#94A3B8]">
                      Test Rust <span className="font-mono text-[#ECE8E1]">host_allowed()</span> guard
                    </label>
                    <select
                      value={testPlatform}
                      onChange={(e) => setTestPlatform(e.target.value as Platform)}
                      className="rounded border border-[#2B3A48] bg-[#182430] px-2 py-0.5 text-[11px] text-white focus:outline-none"
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
                    className="mt-1.5 w-full rounded border border-[#2B3A48] bg-[#182430] px-2.5 py-1.5 font-mono text-xs text-white focus:border-[#FF4655] focus:outline-none"
                  />
                  <p className="mt-2 font-mono text-[11px]">
                    {testedHostOk ? (
                      <span className="text-[#22C55E]">
                        Allowed host: {testedHost}
                      </span>
                    ) : (
                      <span className="text-[#FF4655]">
                        Blocked untrusted URL ({testedHost ? `host "${testedHost}" mismatch` : 'invalid scheme/userinfo/port'})
                      </span>
                    )}
                  </p>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* TAB 3: SETTINGS */}
        {activeTab === 'settings' && (
          <div className="space-y-4">
            {/* Shard & GitHub Directory */}
            <section className="rounded-lg border border-[#2B3A48] bg-[#182430] p-4">
              <h3 className="text-sm font-semibold text-white">
                Riot Shard &amp; Community GitHub Directory
              </h3>

              <div className="mt-3">
                <span className="block text-xs font-medium text-[#94A3B8]">
                  Riot Shard Endpoint (<span className="font-mono">pd.&#123;shard&#125;.a.pvp.net</span>)
                </span>
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {SHARD_OPTIONS.map((opt) => (
                    <button
                      key={opt.code}
                      type="button"
                      onClick={() => setDraft({ ...draft, shard_override: opt.code })}
                      className={`rounded-md border px-3 py-1.5 text-xs font-medium transition-colors whitespace-nowrap ${
                        draft.shard_override === opt.code
                          ? 'border-[#FF4655] bg-[#21303F] text-white'
                          : 'border-[#2B3A48] bg-[#0F1923] text-[#94A3B8] hover:text-white'
                      }`}
                    >
                      {opt.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="mt-4 grid gap-3 sm:grid-cols-2">
                <div>
                  <label className="mb-1 block text-xs font-medium text-[#ECE8E1]">
                    GitHub Directory Owner
                  </label>
                  <input
                    type="text"
                    value={draft.github_owner}
                    onChange={(e) => {
                      setDraft({ ...draft, github_owner: e.target.value });
                      setSettingsError(null);
                    }}
                    placeholder="your-github-username"
                    className="w-full rounded border border-[#2B3A48] bg-[#0F1923] px-3 py-2 font-mono text-xs text-white focus:border-[#FF4655] focus:outline-none"
                  />
                </div>
                <div>
                  <label className="mb-1 block text-xs font-medium text-[#ECE8E1]">
                    GitHub Directory Repository
                  </label>
                  <input
                    type="text"
                    value={draft.github_repo}
                    onChange={(e) => {
                      setDraft({ ...draft, github_repo: e.target.value });
                      setSettingsError(null);
                    }}
                    placeholder="valorant-streamers-db"
                    className="w-full rounded border border-[#2B3A48] bg-[#0F1923] px-3 py-2 font-mono text-xs text-white focus:border-[#FF4655] focus:outline-none"
                  />
                </div>
              </div>
              <p className="mt-2 font-mono text-[11px] text-[#94A3B8]">
                Fetches: https://raw.githubusercontent.com/{draft.github_owner || '<owner>'}/{draft.github_repo || '<repo>'}/main/streamers.json
              </p>
            </section>

            {/* Optional Platform API Keys */}
            <section className="rounded-lg border border-[#2B3A48] bg-[#182430] p-4">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <h3 className="text-sm font-semibold text-white">
                    Optional Platform API Keys
                  </h3>
                  <p className="mt-0.5 text-xs text-[#94A3B8]">
                    Leave blank to run 100% keyless on YouTube, Twitch, and Kick, or add free keys for faster official API checks.
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => setShowSecrets((prev) => !prev)}
                  className="inline-flex items-center gap-1.5 rounded border border-[#2B3A48] bg-[#21303F] px-2.5 py-1 text-xs text-[#ECE8E1] hover:bg-[#2a3c4e] whitespace-nowrap"
                >
                  {showSecrets ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
                  {showSecrets ? 'Hide keys' : 'Show keys'}
                </button>
              </div>

              <div className="mt-3.5 space-y-3.5">
                {/* YouTube Data API v3 */}
                <div>
                  <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2">
                    <label className="text-xs font-medium text-[#ECE8E1]">
                      YouTube Data API v3 Key
                    </label>
                    <div className="flex items-center gap-1.5">
                      <a
                        href="https://console.cloud.google.com/apis/credentials"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center gap-1 rounded bg-[#FF4655] px-2.5 py-1 text-[11px] font-semibold text-white transition-colors hover:bg-[#e53a49] whitespace-nowrap"
                      >
                        Get YouTube Key
                        <ExternalLink className="h-3 w-3" />
                      </a>
                      <button
                        type="button"
                        onClick={handleTestYoutubeKey}
                        className="rounded border border-[#2B3A48] bg-[#21303F] px-2.5 py-1 text-[11px] font-medium text-[#ECE8E1] hover:bg-[#2a3c4e] whitespace-nowrap"
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
                    className="w-full rounded border border-[#2B3A48] bg-[#0F1923] px-3 py-1.5 font-mono text-xs text-white placeholder-[#64748B] focus:border-[#FF4655] focus:outline-none"
                  />
                  {ytKeyStatus && (
                    <div
                      className={`mt-1.5 flex items-start gap-1.5 rounded border px-2.5 py-1.5 text-[11px] ${
                        ytKeyStatus.kind === 'ok'
                          ? 'border-[#22C55E]/40 bg-[#22C55E]/10 text-[#22C55E]'
                          : 'border-[#FF4655]/40 bg-[#FF4655]/10 text-[#FF4655]'
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
                    <span className="text-xs font-medium text-[#ECE8E1]">
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
                        className="rounded border border-[#2B3A48] bg-[#21303F] px-2.5 py-1 text-[11px] font-medium text-[#ECE8E1] hover:bg-[#2a3c4e] whitespace-nowrap"
                      >
                        Test Twitch
                      </button>
                    </div>
                  </div>
                  <div className="grid gap-2.5 sm:grid-cols-2">
                    <div>
                      <label className="mb-1 block text-[11px] text-[#94A3B8]">
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
                        className="w-full rounded border border-[#2B3A48] bg-[#0F1923] px-3 py-1.5 font-mono text-xs text-white placeholder-[#64748B] focus:border-[#FF4655] focus:outline-none"
                      />
                    </div>
                    <div>
                      <label className="mb-1 block text-[11px] text-[#94A3B8]">
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
                        className="w-full rounded border border-[#2B3A48] bg-[#0F1923] px-3 py-1.5 font-mono text-xs text-white placeholder-[#64748B] focus:border-[#FF4655] focus:outline-none"
                      />
                    </div>
                  </div>
                  {twitchKeyStatus && (
                    <div
                      className={`mt-1.5 flex items-start gap-1.5 rounded border px-2.5 py-1.5 text-[11px] ${
                        twitchKeyStatus.kind === 'ok'
                          ? 'border-[#22C55E]/40 bg-[#22C55E]/10 text-[#22C55E]'
                          : 'border-[#FF4655]/40 bg-[#FF4655]/10 text-[#FF4655]'
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
                <div className="rounded-md border border-[#2B3A48] bg-[#0F1923]">
                  <button
                    type="button"
                    onClick={() => setShowApiGuide((prev) => !prev)}
                    className="flex w-full items-center justify-between px-3 py-2 text-left text-xs font-semibold text-[#2DD4BF] transition-colors hover:text-white"
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
                    <div className="space-y-3 border-t border-[#2B3A48] px-3.5 py-3 text-[11.5px] leading-relaxed text-[#ECE8E1]">
                      <div>
                        <p className="font-semibold text-[#FF4655]">
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
                        <p className="font-semibold text-[#9146FF]">
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
                      <p className="flex items-center gap-1.5 text-[11px] text-[#94A3B8]">
                        <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-[#FB923C]" />
                        Kick needs no API key. Leaving all keys blank still works via keyless fallbacks.
                      </p>
                    </div>
                  )}
                </div>
              </div>
            </section>

            {/* Windows Integration */}
            <section className="rounded-lg border border-[#2B3A48] bg-[#182430] p-4">
              <h3 className="text-sm font-semibold text-white">Windows Integration</h3>
              <div className="mt-3 space-y-2.5">
                <label className="flex items-start gap-2.5 text-xs text-[#ECE8E1] cursor-pointer">
                  <input
                    type="checkbox"
                    checked={draft.start_with_windows}
                    onChange={(e) =>
                      setDraft({ ...draft, start_with_windows: e.target.checked })
                    }
                    className="mt-0.5 accent-[#FF4655]"
                  />
                  <span>
                    Start silently with Windows (writes{' '}
                    <span className="font-mono text-[#94A3B8]">
                      HKCU\Software\Microsoft\Windows\CurrentVersion\Run\ValorantStreamerRadar
                    </span>
                    )
                  </span>
                </label>
                <label className="flex items-start gap-2.5 text-xs text-[#ECE8E1] cursor-pointer">
                  <input
                    type="checkbox"
                    checked={draft.use_custom_toast_app_id}
                    onChange={(e) =>
                      setDraft({ ...draft, use_custom_toast_app_id: e.target.checked })
                    }
                    className="mt-0.5 accent-[#FF4655]"
                  />
                  <span>
                    Register custom Windows Toast AppUserModelID (shows{' '}
                    <span className="font-semibold text-white">Valorant Streamer Radar</span> + V icon
                    instead of Windows PowerShell)
                  </span>
                </label>
              </div>
            </section>

            {settingsError && (
              <p className="text-xs font-medium text-[#FF4655]" role="alert">
                {settingsError}
              </p>
            )}

            <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
              <span className="font-mono text-[11px] text-[#94A3B8]">
                Stored atomically in %APPDATA%\ValorantStreamerRadar\config.json
              </span>
              <button
                type="button"
                onClick={handleSaveSettings}
                className="rounded-md bg-[#FF4655] px-5 py-2 text-xs font-semibold text-white transition-colors hover:bg-[#e53a49] whitespace-nowrap"
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
