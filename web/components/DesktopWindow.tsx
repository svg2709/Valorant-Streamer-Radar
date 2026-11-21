import React, { useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  ArrowUpCircle,
  Check,
  CheckCircle2,
  ChevronDown,
  Copy,
  Download,
  Eye,
  EyeOff,
  ExternalLink,
  KeyRound,
  MessageSquareQuote,
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
      return '#A770FF';
    case 'Kick':
      return '#53FC18';
    default:
      return '#FF5CA8';
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

  // App Updater state (queries real GitHub Releases for svg2709/Valorant-Streamer-Radar)
  const [updateState, setUpdateState] = useState<
    | { status: 'idle' }
    | { status: 'checking' }
    | { status: 'up_to_date'; message: string }
    | {
        status: 'available';
        tagName: string;
        releaseName: string;
        downloadUrl: string;
        htmlUrl: string;
        sizeLabel: string;
        publishedAt: string;
        buildSha?: string;
        shaVerified: boolean;
      }
    | { status: 'error'; message: string }
  >({ status: 'idle' });

  const handleCheckForUpdates = async () => {
    setUpdateState({ status: 'checking' });
    try {
      const res = await fetch(
        'https://api.github.com/repos/svg2709/Valorant-Streamer-Radar/releases/latest',
        { headers: { Accept: 'application/vnd.github+json' } }
      );
      if (res.status === 404) {
        setUpdateState({
          status: 'up_to_date',
          message: 'No GitHub Release published yet (v1.0.0). Push to main to trigger the automated build.',
        });
        return;
      }
      if (res.status === 403) {
        setUpdateState({
          status: 'error',
          message: 'GitHub API rate limit reached. Open Releases directly or try again shortly.',
        });
        return;
      }
      if (!res.ok) {
        setUpdateState({
          status: 'error',
          message: `GitHub Releases check failed (HTTP ${res.status}).`,
        });
        return;
      }
      const data = await res.json();
      const assets: Array<{ name?: string; browser_download_url?: string; size?: number; digest?: string }> =
        Array.isArray(data.assets) ? data.assets : [];
      const exe = assets.find((a) => (a.name || '').toLowerCase().endsWith('.exe'));
      const shaAsset = assets.find((a) => (a.name || '').toLowerCase().endsWith('.sha256'));
      if (!exe?.browser_download_url) {
        setUpdateState({
          status: 'up_to_date',
          message: `Latest release (${data.tag_name || 'latest'}) has no Windows .exe attached yet.`,
        });
        return;
      }
      const bytes = Number(exe.size || 0);
      const sizeLabel =
        bytes >= 1024 * 1024
          ? `${(bytes / (1024 * 1024)).toFixed(1)} MB`
          : bytes >= 1024
          ? `${Math.round(bytes / 1024)} KB`
          : '';
      const shaMatch = typeof data.body === 'string' ? data.body.match(/commit\s+`?([0-9a-f]{7,40})`?/i) : null;
      setUpdateState({
        status: 'available',
        tagName: data.tag_name || 'latest',
        releaseName: data.name || data.tag_name || 'Latest Windows Build',
        downloadUrl: exe.browser_download_url,
        htmlUrl: data.html_url || 'https://github.com/svg2709/Valorant-Streamer-Radar/releases',
        sizeLabel,
        publishedAt: typeof data.published_at === 'string' ? data.published_at.slice(0, 10) : '',
        buildSha: shaMatch ? shaMatch[1].slice(0, 7) : undefined,
        shaVerified: Boolean(shaAsset || exe.digest),
      });
    } catch {
      setUpdateState({
        status: 'error',
        message: 'Could not reach GitHub API. Check your internet connection or open Releases.',
      });
    }
  };

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
        (e.chat_message?.toLowerCase().includes(q) ?? false) ||
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
      <div className="flex min-h-[540px] flex-col items-center justify-center rounded-lg border border-dashed border-[#273144] bg-[#0D111A]/80 px-6 py-16 text-center">
        <RadarIcon size={42} />
        <h3 className="mt-4 text-base font-semibold text-[#FFF5FA]">
          Window Hidden in Windows System Tray
        </h3>
        <p className="mt-1.5 max-w-md text-xs leading-relaxed text-[#98A2B8]">
          Closing the window intercepts the close event and sets{' '}
          <span className="font-mono text-[#F3EFF4]">WINDOW_VISIBLE = false</span>.
          The UI loop schedules zero repaints (0% idle CPU) while the background{' '}
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
                    ? `Valorant Active (${SHARD_LABELS[effectiveShard]})`
                    : `Waiting for Valorant (${SHARD_LABELS[effectiveShard]})`}
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
              onClick={handleCheckForUpdates}
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
                ? 'Update Ready!'
                : 'Check for Updates'}
            </button>
          </div>
        </div>
      </div>

      {/* Main Panel (#0D111A) */}
      <div className="min-h-[440px] max-h-[600px] overflow-y-auto px-6 py-4">
        {/* Global Update Status Banner */}
        {updateState.status === 'available' && (
          <div className="mb-3.5 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-[#FF5CA8] bg-[#141B27] px-4 py-2.5">
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-2">
                <span className="inline-flex items-center gap-1 text-[10.5px] font-bold tracking-wider text-[#FF5CA8]">
                  <ArrowUpCircle className="h-3.5 w-3.5" />
                  UPDATE AVAILABLE
                </span>
                <span className="text-xs font-bold text-[#FFF5FA]">
                  {updateState.releaseName}
                </span>
              </div>
              <p className="mt-0.5 font-mono text-[11px] text-[#98A2B8]">
                {[
                  `Tag: ${updateState.tagName}`,
                  updateState.buildSha ? `Build ${updateState.buildSha}` : '',
                  updateState.sizeLabel,
                  updateState.publishedAt,
                  updateState.shaVerified ? 'SHA-256 verified' : '',
                ]
                  .filter(Boolean)
                  .join('  |  ')}
              </p>
            </div>
            <div className="flex items-center gap-2">
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
              <button
                type="button"
                onClick={() => setUpdateState({ status: 'idle' })}
                className="rounded p-1 text-[#98A2B8] hover:text-white"
                title="Dismiss"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        )}

        {updateState.status === 'up_to_date' && activeTab !== 'settings' && (
          <div className="mb-3.5 flex items-center justify-between gap-3 rounded-lg border border-[#22C55E]/60 bg-[#141B27] px-3.5 py-2 text-xs text-[#22C55E]">
            <span className="inline-flex items-center gap-1.5">
              <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />
              {updateState.message}
            </span>
            <button
              type="button"
              onClick={() => setUpdateState({ status: 'idle' })}
              className="text-[11px] text-[#98A2B8] hover:text-white"
            >
              Dismiss
            </button>
          </div>
        )}

        {updateState.status === 'error' && activeTab !== 'settings' && (
          <div className="mb-3.5 flex items-center justify-between gap-3 rounded-lg border border-[#FF668A]/60 bg-[#141B27] px-3.5 py-2 text-xs text-[#FF668A]">
            <span className="inline-flex items-center gap-1.5">
              <XCircle className="h-3.5 w-3.5 shrink-0" />
              {updateState.message}
            </span>
            <div className="flex items-center gap-2">
              <a
                href="https://github.com/svg2709/Valorant-Streamer-Radar/releases"
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
                Dismiss
              </button>
            </div>
          </div>
        )}
        {/* TAB 1: HISTORY */}
        {activeTab === 'history' && (
          <div>
            {lastScannedMatch && (
              <div className="mb-3.5 flex flex-wrap items-center justify-between gap-2 rounded-md border border-[#273144] bg-[#090D14] px-3.5 py-2.5 text-xs">
                <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
                  <span className="text-[10px] font-bold tracking-wider text-[#98A2B8]">
                    LAST SCANNED MATCH
                  </span>
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
                    {lastScannedMatch.players_checked} players checked
                  </span>
                  <span className="text-[#273144]">|</span>
                  <span
                    className={
                      lastScannedMatch.streamers_found > 0
                        ? 'font-semibold text-[#22C55E]'
                        : 'text-[#98A2B8]'
                    }
                  >
                    {lastScannedMatch.streamers_found > 0
                      ? `${lastScannedMatch.streamers_found} live found`
                      : '0 live'}
                  </span>
                </div>
                <span className="font-mono text-[11px] text-[#98A2B8] tabular-nums">
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
                    placeholder="Search Riot ID, chat quote, platform, map, or title..."
                    className="w-full rounded border border-[#273144] bg-[#141B27] py-1.5 pr-3 pl-8 text-xs text-white placeholder-[#64748B] focus:border-[#FF5CA8] focus:outline-none"
                  />
                </div>

                <div className="flex items-center rounded-md border border-[#273144] bg-[#090D14] p-0.5">
                  {(['All', 'Ally', 'Enemy'] as const).map((f) => (
                    <button
                      key={f}
                      type="button"
                      onClick={() => setTeamFilter(f)}
                      className={`rounded px-2.5 py-1 text-xs font-medium transition-colors whitespace-nowrap ${
                        teamFilter === f
                          ? 'bg-[#1D2636] text-[#FFF5FA]'
                          : 'text-[#98A2B8] hover:text-white'
                      }`}
                    >
                      {f}
                    </button>
                  ))}
                </div>

                <span className="font-mono text-xs text-[#98A2B8] tabular-nums">
                  {filteredHistory.length} detected
                </span>
              </div>

              {history.length > 0 && (
                <button
                  type="button"
                  onClick={onClearHistory}
                  className="rounded border border-[#273144] px-3 py-1.5 text-xs text-[#98A2B8] transition-colors hover:border-[#FF5CA8] hover:text-white whitespace-nowrap"
                >
                  Clear History
                </button>
              )}
            </div>

            {filteredHistory.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-20 text-center">
                <h3 className="text-base font-bold text-[#F3EFF4]">
                  No Streamers Recorded Yet
                </h3>
                <p className="mt-1.5 max-w-md text-xs text-[#98A2B8]">
                  Play a match on any supported shard, run a simulated match in Match Lab,
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
                      className={`flex flex-col justify-between gap-3 rounded-lg border bg-[#141B27] px-4 py-3.5 sm:flex-row sm:items-center ${
                        entry.chat_message
                          ? 'border-[#FF5CA8]/45'
                          : 'border-[#273144]'
                      }`}
                    >
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
                          <span
                            className="text-xs font-semibold"
                            style={{ color: pColor }}
                          >
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
                          {entry.chat_message && (
                            <span className="inline-flex items-center gap-1 rounded border border-[#FFA6D2]/60 bg-[#FF5CA8]/15 px-1.5 py-0.5 text-[10.5px] font-semibold text-[#FFF5FA]">
                              <MessageSquareQuote className="h-3 w-3 text-[#FFA6D2]" />
                              In-Game Chat
                            </span>
                          )}
                        </div>

                        <p className="mt-1 truncate text-[13px] text-[#F3EFF4]">
                          {entry.title}
                        </p>

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
                {isSyncingDirectory ? 'Syncing...' : 'Sync GitHub'}
              </button>
            </div>

            {filteredDirectory.length === 0 ? (
              <div className="rounded-lg border border-[#273144] bg-[#141B27]/50 py-12 text-center text-xs text-[#98A2B8]">
                No entries match your filter.
              </div>
            ) : (
              <div className="space-y-2">
                {filteredDirectory.map((d) => {
                  const pColor = platformColor(d.platform);
                  return (
                    <div
                      key={`${d.riot_id}-${d.platform}-${d.channel_url}`}
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
                        </div>
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
                      <span className="text-[#22C55E]">
                        Allowed host: {testedHost}
                      </span>
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

        {/* TAB 3: SETTINGS */}
        {activeTab === 'settings' && (
          <div className="space-y-4">
            {/* Shard & GitHub Directory */}
            <section className="rounded-lg border border-[#273144] bg-[#141B27] p-4">
              <h3 className="text-sm font-semibold text-[#FFF5FA]">
                Riot Shard &amp; Community GitHub Directory
              </h3>

              <div className="mt-3">
                <span className="block text-xs font-medium text-[#98A2B8]">
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
                          ? 'border-[#FF5CA8] bg-[#1D2636] text-[#FFF5FA]'
                          : 'border-[#273144] bg-[#0D111A] text-[#98A2B8] hover:text-white'
                      }`}
                    >
                      {opt.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="mt-4 grid gap-3 sm:grid-cols-2">
                <div>
                  <label className="mb-1 block text-xs font-medium text-[#F3EFF4]">
                    GitHub Directory Owner
                  </label>
                  <input
                    type="text"
                    value={draft.github_owner}
                    onChange={(e) => {
                      setDraft({ ...draft, github_owner: e.target.value });
                      setSettingsError(null);
                    }}
                    placeholder="svg2709"
                    className="w-full rounded border border-[#273144] bg-[#0D111A] px-3 py-2 font-mono text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                  />
                </div>
                <div>
                  <label className="mb-1 block text-xs font-medium text-[#F3EFF4]">
                    GitHub Directory Repository
                  </label>
                  <input
                    type="text"
                    value={draft.github_repo}
                    onChange={(e) => {
                      setDraft({ ...draft, github_repo: e.target.value });
                      setSettingsError(null);
                    }}
                    placeholder="Valorant-Streamer-Radar"
                    className="w-full rounded border border-[#273144] bg-[#0D111A] px-3 py-2 font-mono text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                  />
                </div>
              </div>
              <p className="mt-2 font-mono text-[11px] text-[#98A2B8]">
                Fetches: https://raw.githubusercontent.com/{draft.github_owner || '<owner>'}/{draft.github_repo || '<repo>'}/main/streamers.json
              </p>
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
                <button
                  type="button"
                  onClick={() => setShowSecrets((prev) => !prev)}
                  className="inline-flex items-center gap-1.5 rounded border border-[#273144] bg-[#1D2636] px-2.5 py-1 text-xs text-[#F3EFF4] hover:bg-[#253044] whitespace-nowrap"
                >
                  {showSecrets ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
                  {showSecrets ? 'Hide keys' : 'Show keys'}
                </button>
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

            {/* In-Game Chat & Windows Integration */}
            <section className="rounded-lg border border-[#273144] bg-[#141B27] p-4">
              <h3 className="text-sm font-semibold text-[#FFF5FA]">
                In-Game Chat &amp; Windows Integration
              </h3>
              <div className="mt-3 space-y-2.5">
                <label className="flex cursor-pointer items-start gap-2.5 text-xs text-[#F3EFF4]">
                  <input
                    type="checkbox"
                    checked={draft.capture_in_game_chat}
                    onChange={(e) =>
                      setDraft({ ...draft, capture_in_game_chat: e.target.checked })
                    }
                    className="mt-0.5 accent-[#FF5CA8]"
                  />
                  <span>
                    Read post-match in-game text chat for stream links (captures messages like{' '}
                    <span className="font-mono text-[#FFA6D2]">&ldquo;im live on yt&rdquo;</span> or dropped
                    YouTube/Twitch/Kick links via Riot Client{' '}
                    <span className="font-mono text-[#98A2B8]">/chat/v6/messages</span>)
                  </span>
                </label>
                <label className="flex cursor-pointer items-start gap-2.5 text-xs text-[#F3EFF4]">
                  <input
                    type="checkbox"
                    checked={draft.start_with_windows}
                    onChange={(e) =>
                      setDraft({ ...draft, start_with_windows: e.target.checked })
                    }
                    className="mt-0.5 accent-[#FF5CA8]"
                  />
                  <span>
                    Start silently with Windows (writes{' '}
                    <span className="font-mono text-[#98A2B8]">
                      HKCU\Software\Microsoft\Windows\CurrentVersion\Run\ValorantStreamerRadar
                    </span>
                    )
                  </span>
                </label>
                <label className="flex cursor-pointer items-start gap-2.5 text-xs text-[#F3EFF4]">
                  <input
                    type="checkbox"
                    checked={draft.use_custom_toast_app_id}
                    onChange={(e) =>
                      setDraft({ ...draft, use_custom_toast_app_id: e.target.checked })
                    }
                    className="mt-0.5 accent-[#FF5CA8]"
                  />
                  <span>
                    Register custom Windows Toast AppUserModelID (shows{' '}
                    <span className="font-semibold text-[#FFF5FA]">Valorant Streamer Radar</span> + pink-white V icon
                    instead of Windows PowerShell)
                  </span>
                </label>
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
                    Installed: v1.0.0 &nbsp;|&nbsp; Source: svg2709/Valorant-Streamer-Radar
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    disabled={updateState.status === 'checking'}
                    onClick={handleCheckForUpdates}
                    className="inline-flex h-8 items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-3.5 text-xs font-bold text-[#0D111A] transition-opacity disabled:opacity-60 whitespace-nowrap"
                  >
                    <RefreshCw
                      className={`h-3.5 w-3.5 ${updateState.status === 'checking' ? 'animate-spin' : ''}`}
                    />
                    {updateState.status === 'checking' ? 'Checking...' : 'Check for Updates'}
                  </button>
                  <a
                    href="https://github.com/svg2709/Valorant-Streamer-Radar/releases"
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
                <div className="mt-2.5 flex flex-wrap items-center justify-between gap-2 rounded border border-[#FF5CA8]/40 bg-[#090D14] px-3 py-2 text-xs">
                  <span className="font-semibold text-[#FFA6D2]">
                    New build ready: {updateState.releaseName}
                    {updateState.sizeLabel ? ` (${updateState.sizeLabel})` : ''}
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
              )}
              {updateState.status === 'error' && (
                <p className="mt-2.5 flex items-center gap-1.5 text-xs text-[#FF668A]">
                  <XCircle className="h-3.5 w-3.5 shrink-0" />
                  {updateState.message}
                </p>
              )}
            </section>

            {settingsError && (
              <p className="text-xs font-medium text-[#FF668A]" role="alert">
                {settingsError}
              </p>
            )}

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
