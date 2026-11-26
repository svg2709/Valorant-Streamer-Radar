import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Activity,
  Bell,
  Camera,
  Monitor,
  Settings,
} from 'lucide-react';
import {
  APP_VERSION,
  AppConfig,
  DEFAULT_CONFIG,
  DesktopTab,
  DirectoryStreamer,
  LastMatchPlayerAudit,
  LastMatchSummary,
  LogLineEntry,
  Platform,
  RecommendedStreamer,
  ServerRegionCode,
  ServerRegionOverride,
  StreamerEntry,
  StudioView,
  getServerRegionInfo,
} from './types/radar';
import {
  INITIAL_COMMUNITY_DIRECTORY,
  INITIAL_CUSTOM_DIRECTORY,
  INITIAL_HISTORY,
  SIM_SCENARIOS,
} from './data/mockMatches';
import {
  extractChatStreamPlug,
  formatRadarTimestamp,
  resolveEffectiveRegion,
  resolveEntryTier,
} from './utils/validation';
import { RadarIcon } from './components/RadarIcon';
import { DesktopWindow } from './components/DesktopWindow';
import { StreamerModal } from './components/StreamerModal';
import { MatchTrayLab } from './components/MatchTrayLab';
import { WinRtToast } from './components/WinRtToast';

const STORAGE_KEYS = {
  config: 'vsr_v120_config',
  history: 'vsr_v127_na_history',
  customDir: 'vsr_v127_na_custom_dir',
  lastMatch: 'vsr_v127_na_last_match',
};

function loadStored<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function loadConfigWithMigration(): AppConfig {
  const current = loadStored<Partial<AppConfig> & { shard_override?: ServerRegionOverride }>(
    STORAGE_KEYS.config,
    {}
  );
  if (Object.keys(current).length > 0) {
    return {
      ...DEFAULT_CONFIG,
      ...current,
      server_override: current.server_override ?? current.shard_override ?? DEFAULT_CONFIG.server_override,
    };
  }
  const legacy = loadStored<Partial<AppConfig> & { shard_override?: ServerRegionOverride }>(
    'vsr_v110_config',
    {}
  );
  return {
    ...DEFAULT_CONFIG,
    ...legacy,
    server_override: legacy.server_override ?? legacy.shard_override ?? DEFAULT_CONFIG.server_override,
  };
}

function nowHms(): string {
  const d = new Date();
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, '0'))
    .join(':');
}

export default function App() {
  const [studioView, setStudioView] = useState<StudioView>('simulator');
  const [desktopTab, setDesktopTab] = useState<DesktopTab>('history');
  const [windowVisible, setWindowVisible] = useState<boolean>(true);
  const [screenshotMode, setScreenshotMode] = useState<boolean>(false);

  const [config, setConfig] = useState<AppConfig>(loadConfigWithMigration);
  const [history, setHistory] = useState<StreamerEntry[]>(() =>
    loadStored(STORAGE_KEYS.history, INITIAL_HISTORY)
  );
  const [customDirectory, setCustomDirectory] = useState<DirectoryStreamer[]>(() =>
    loadStored(STORAGE_KEYS.customDir, INITIAL_CUSTOM_DIRECTORY)
  );
  const [communityDirectory, setCommunityDirectory] = useState<DirectoryStreamer[]>(
    INITIAL_COMMUNITY_DIRECTORY
  );

  // Runtime simulator state
  const [valorantRunning, setValorantRunning] = useState<boolean>(true);
  const [inMatch, setInMatch] = useState<boolean>(false);
  const [isScanning, setIsScanning] = useState<boolean>(false);
  const [isSyncingDirectory, setIsSyncingDirectory] = useState<boolean>(false);
  const [detectedRegion, setDetectedRegion] = useState<ServerRegionCode>('na-us-east');
  const [status, setStatus] = useState<string>('Found 2 live stream(s) in Competitive on Ascent!');
  const [selectedScenarioId, setSelectedScenarioId] = useState<string>(SIM_SCENARIOS[0].id);
  const [lastScannedMatch, setLastScannedMatch] = useState<LastMatchSummary | null>(() =>
    loadStored<LastMatchSummary | null>(STORAGE_KEYS.lastMatch, {
      match_id: INITIAL_HISTORY[0]?.match_id ?? 'match-seed-01',
      queue_name: INITIAL_HISTORY[0]?.queue_name ?? 'Competitive',
      map_name: INITIAL_HISTORY[0]?.map_name ?? 'Ascent',
      my_agent: 'Omen',
      players_checked: 7,
      party_skipped: 2,
      hidden_skipped: 0,
      streamers_found: INITIAL_HISTORY.filter((h) => h.match_id === INITIAL_HISTORY[0]?.match_id).length,
      scanned_at: INITIAL_HISTORY[0]?.timestamp ?? formatRadarTimestamp(new Date()),
      server_name: getServerRegionInfo('na-us-east').label,
      player_audits: [
        {
          riot_id: INITIAL_HISTORY[0]?.riot_id ?? 'TylerFPS#NA1',
          team_relation: INITIAL_HISTORY[0]?.team_relation ?? 'Enemy',
          agent_name: INITIAL_HISTORY[0]?.agent_name ?? 'Jett',
          outcome: 'Detected',
          confidence_tier: INITIAL_HISTORY[0]?.confidence_tier ?? 'Confirmed',
          confidence: INITIAL_HISTORY[0]?.confidence ?? 98,
          summary: `${INITIAL_HISTORY[0]?.channel ?? 'TylerFPS'} on ${INITIAL_HISTORY[0]?.platform ?? 'Twitch'} (${INITIAL_HISTORY[0]?.source ?? 'Directory'}) — ${INITIAL_HISTORY[0]?.match_reason ?? 'Verified streamer directory match'}`,
          steps: [
            {
              stage: 'Directory',
              outcome: 'Live (Confirmed)',
              detail: `Matched '${INITIAL_HISTORY[0]?.channel ?? 'TylerFPS'}' in streamer database -> In-Game Server Confirmed.`,
            },
            {
              stage: 'ML Heuristic',
              outcome: 'Skipped',
              detail: 'Skipped heuristic lookup because verified Directory channel is already live.',
            },
            {
              stage: 'Match Chat',
              outcome: 'No Plug',
              detail: 'No stream URLs or self-promo mentions found in match chat.',
            },
          ],
        },
        {
          riot_id: 'KairoVal#NA1',
          team_relation: 'Ally',
          agent_name: 'Sova',
          outcome: 'No Match',
          confidence_tier: '',
          confidence: 0,
          summary: 'Checked Directory, YouTube, Twitch, Kick & Match Chat — no live match',
          steps: [
            {
              stage: 'Directory',
              outcome: 'No Entry',
              detail: 'Riot ID not listed in community or custom streamer database.',
            },
            {
              stage: 'YouTube',
              outcome: 'No Match',
              detail: "Searched live streams for 'KairoVal Valorant' (region US) -> 0 candidates passed ML, server & broadcast filters.",
            },
            {
              stage: 'Twitch',
              outcome: 'No Match',
              detail: 'Batched [kairoval, kairovaltv, kairoval_tv, kairovalttv] -> offline or not streaming VALORANT.',
            },
            {
              stage: 'Kick',
              outcome: 'No Match',
              detail: "Checked Kick slug(s) for 'KairoVal' -> offline or no matching live VALORANT stream.",
            },
            {
              stage: 'Match Chat',
              outcome: 'No Plug',
              detail: 'No stream URLs or self-promo mentions found in match chat.',
            },
          ],
        },
        {
          riot_id: 'DuoMate#NA1',
          team_relation: 'Ally',
          agent_name: 'Sage',
          outcome: 'Party Skipped',
          confidence_tier: '',
          confidence: 0,
          summary: 'Skipped automatically (same party as you)',
          steps: [
            {
              stage: 'Party Guard',
              outcome: 'Skipped',
              detail: 'Matched your match-details partyId — saved YouTube, Twitch & Kick lookups (toggleable in Settings).',
            },
          ],
        },
      ],
    })
  );

  // Modals & Toasts
  const [modalOpen, setModalOpen] = useState<boolean>(false);
  const [editingEntry, setEditingEntry] = useState<DirectoryStreamer | null>(null);
  const [prefillRec, setPrefillRec] = useState<RecommendedStreamer | null>(null);
  const [inlineToast, setInlineToast] = useState<string | null>(null);
  const [winRtToastEntry, setWinRtToastEntry] = useState<StreamerEntry | null>(null);
  const [logs, setLogs] = useState<LogLineEntry[]>([
    {
      id: 'boot-1',
      time: nowHms(),
      category: 'log',
      message: 'Single-instance lock acquired on 127.0.0.1:47831; started hidden in tray.',
    },
    {
      id: 'boot-2',
      time: nowHms(),
      category: 'server',
      message: 'ShooterGame.log server detected: aresriot.aws-us-east-1-ashburn-1 -> NA / US East (Virginia / Georgia / Illinois).',
    },
  ]);

  const toastTimerRef = useRef<number | null>(null);
  const winRtTimerRef = useRef<number | null>(null);
  const scanTimersRef = useRef<number[]>([]);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEYS.config, JSON.stringify(config));
    } catch {
      // ignore quota errors
    }
  }, [config]);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEYS.history, JSON.stringify(history));
    } catch {
      // ignore
    }
  }, [history]);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEYS.customDir, JSON.stringify(customDirectory));
    } catch {
      // ignore
    }
  }, [customDirectory]);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEYS.lastMatch, JSON.stringify(lastScannedMatch));
    } catch {
      // ignore
    }
  }, [lastScannedMatch]);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
      if (winRtTimerRef.current) window.clearTimeout(winRtTimerRef.current);
      scanTimersRef.current.forEach((id) => window.clearTimeout(id));
    };
  }, []);

  const pushLog = useCallback((category: LogLineEntry['category'], message: string) => {
    setLogs((prev) => [
      ...prev.slice(-120),
      {
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
        time: nowHms(),
        category,
        message,
      },
    ]);
  }, []);

  const showInlineToast = useCallback((msg: string) => {
    setInlineToast(msg);
    if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
    toastTimerRef.current = window.setTimeout(() => setInlineToast(null), 4000);
  }, []);

  const showWinRtToast = useCallback((entry: StreamerEntry) => {
    setWinRtToastEntry(entry);
    if (winRtTimerRef.current) window.clearTimeout(winRtTimerRef.current);
    winRtTimerRef.current = window.setTimeout(() => setWinRtToastEntry(null), 8000);
  }, []);

  const effectiveRegion = resolveEffectiveRegion(config, detectedRegion);

  const runSimulatedScan = useCallback(
    (trigger: 'Manual' | 'PostMatch') => {
      if (isScanning) return;
      if (!valorantRunning) {
        setStatus('Could not fetch match: Riot Client not running');
        pushLog('riot', 'Lockfile read failed: Riot Client not running.');
        showInlineToast('Riot Client lockfile not detected — start Valorant in Match Lab');
        return;
      }

      const scenario =
        SIM_SCENARIOS.find((s) => s.id === selectedScenarioId) ?? SIM_SCENARIOS[0];
      const regionForScan = resolveEffectiveRegion(config, scenario.serverRegion);

      const scannablePlayers = config.skip_party_members
        ? scenario.players.filter((p) => !p.isPartyMember)
        : scenario.players;
      const partySkipped = scenario.players.length - scannablePlayers.length;

      setIsScanning(true);
      setDetectedRegion(scenario.serverRegion);
      setStatus(`Syncing database & scanning last match (${regionForScan.label})...`);

      pushLog(
        'riot',
        `[${trigger}] Lockfile authenticated -> GET https://127.0.0.1:52194/entitlements/v1/token`
      );

      const t1 = window.setTimeout(() => {
        pushLog(
          'server',
          `Match server resolved: ${regionForScan.podExample} -> ${regionForScan.label} (endpoint pd.${regionForScan.apiRegion}.a.pvp.net)`
        );
      }, 450);

      const t2 = window.setTimeout(() => {
        pushLog(
          'scan',
          `Match details: ${scenario.map_name} (${scenario.queue_name}) · ${scenario.players.length} other players · Server filter: ${regionForScan.shortLabel}`
        );
        if (partySkipped > 0) {
          pushLog(
            'scan',
            `PARTY GUARD: Read partyId from match-details (0 extra Riot API calls) -> skipped ${partySkipped} party member(s), saving ~${partySkipped * 3} external API lookups`
          );
        }
        if (config.capture_in_game_chat && scenario.chatLog?.length) {
          pushLog(
            'chat',
            `GET https://127.0.0.1:52194/chat/v6/messages -> ${scenario.chatLog.length} match chat message(s) buffered`
          );
        }
        pushLog(
          'scan',
          `Synced ${customDirectory.length} custom + ${communityDirectory.length} community streamers.json entries`
        );
      }, 950);

      const t3 = window.setTimeout(() => {
        const now = new Date();
        const timestamp = formatRadarTimestamp(now);
        const isoNow = now.toISOString();
        const uniqueMatchId = `${scenario.match_id}-${Date.now().toString().slice(-4)}`;
        const foundEntries: StreamerEntry[] = [];
        const playerAudits: LastMatchPlayerAudit[] = [];

        // Record party-skipped players when party guard is enabled
        if (config.skip_party_members) {
          for (const p of scenario.players) {
            if (!p.isPartyMember) continue;
            playerAudits.push({
              riot_id: p.riot_id,
              team_relation: p.team_relation,
              agent_name: p.agent_name,
              outcome: 'Party Skipped',
              confidence_tier: '',
              confidence: 0,
              summary: 'Skipped automatically (same party as you)',
              steps: [
                {
                  stage: 'Party Guard',
                  outcome: 'Skipped',
                  detail: 'Matched your match-details partyId — saved YouTube, Twitch & Kick lookups (toggleable in Settings).',
                },
              ],
            });
          }
        }

        const chatByPlayer = new Map<string, string>();
        if (config.capture_in_game_chat && scenario.chatLog) {
          for (const line of scenario.chatLog) {
            chatByPlayer.set(line.riot_id.toLowerCase(), line.body);
          }
        }

        for (const p of scannablePlayers) {
          if (p.rejectedCrossRegion) {
            pushLog('scan', `SERVER GUARD [${p.riot_id}]: ${p.rejectedCrossRegion}`);
          }

          const chatBody = chatByPlayer.get(p.riot_id.toLowerCase());
          const chatPlug = chatBody ? extractChatStreamPlug(chatBody, p.riot_id) : null;
          const steps: LastMatchPlayerAudit['steps'] = [];
          const cleanName = p.riot_id.split('#')[0] || p.riot_id;

          if (p.detectionSource === 'directory' && p.liveStream) {
            steps.push({
              stage: 'Directory',
              outcome: `Live (${p.liveStream.confidenceTier})`,
              detail: `Matched '${p.liveStream.channel}' on ${p.liveStream.platform} -> ${p.liveStream.serverMatchStatus} (${p.liveStream.confidence}%).`,
            });
            steps.push({
              stage: 'ML Heuristic',
              outcome: 'Skipped',
              detail: 'Skipped heuristic lookup because a registered Directory channel is already live.',
            });
          } else {
            steps.push({
              stage: 'Directory',
              outcome: 'No Entry',
              detail: 'Riot ID not listed in community or custom streamer database.',
            });
            if (p.liveStream) {
              steps.push({
                stage: p.liveStream.platform,
                outcome: `${p.liveStream.confidenceTier} (${p.liveStream.confidence}%)`,
                detail: `${p.liveStream.channel} — ${p.liveStream.match_reason} (${p.liveStream.serverMatchStatus}).`,
              });
            } else if (p.rejectedCrossRegion) {
              steps.push({
                stage: 'Server Guard',
                outcome: 'Rejected',
                detail: p.rejectedCrossRegion,
              });
            } else {
              steps.push({
                stage: 'YouTube',
                outcome: 'No Match',
                detail: `Searched live streams for '${cleanName} Valorant' -> 0 candidates passed ML, server & broadcast filters.`,
              });
              steps.push({
                stage: 'Twitch',
                outcome: 'No Match',
                detail: `Batched login candidates for '${cleanName}' -> offline or not streaming VALORANT.`,
              });
              steps.push({
                stage: 'Kick',
                outcome: 'No Match',
                detail: `Checked Kick slug(s) for '${cleanName}' -> offline or no matching live VALORANT stream.`,
              });
            }
          }

          if (!config.capture_in_game_chat) {
            steps.push({
              stage: 'Match Chat',
              outcome: 'Disabled',
              detail: 'In-game chat capture is disabled in Settings.',
            });
          } else if (chatPlug) {
            steps.push({
              stage: 'Match Chat',
              outcome: 'Matched',
              detail: `Detected ${chatPlug.platform} self-plug in post-match chat: "${chatPlug.rawMessage}"`,
            });
          } else {
            steps.push({
              stage: 'Match Chat',
              outcome: 'No Plug',
              detail: 'No stream URLs or self-promo mentions found in match chat.',
            });
          }

          let playerTopEntry: StreamerEntry | null = null;

          if (p.liveStream) {
            const entry: StreamerEntry = {
              match_id: uniqueMatchId,
              riot_id: p.riot_id,
              platform: p.liveStream.platform,
              channel: p.liveStream.channel,
              title: p.liveStream.title,
              url: p.liveStream.url,
              timestamp,
              detected_at_iso: isoNow,
              team_relation: p.team_relation,
              map_name: scenario.map_name,
              agent_name: p.agent_name,
              queue_name: scenario.queue_name,
              chat_message:
                chatPlug && chatPlug.platform === p.liveStream.platform
                  ? chatPlug.rawMessage
                  : undefined,
              source: p.detectionSource === 'directory' ? 'Directory' : 'Heuristic',
              confidence: p.liveStream.confidence,
              confidence_tier: p.liveStream.confidenceTier,
              server_match_status: p.liveStream.serverMatchStatus,
              match_reason: p.liveStream.match_reason,
              server_region: regionForScan.shortLabel,
              candidates: p.liveStream.candidates,
            };
            foundEntries.push(entry);
            playerTopEntry = entry;
            pushLog(
              p.liveStream.confidenceTier === 'Unverified' ? 'scan' : 'hit',
              `${p.liveStream.confidenceTier.toUpperCase()} [${p.liveStream.platform} · ${p.liveStream.confidence}%] ${p.riot_id} (${p.liveStream.serverMatchStatus}) -> ${p.liveStream.url}`
            );
          }

          if (
            chatPlug &&
            (!p.liveStream || p.liveStream.platform !== chatPlug.platform)
          ) {
            const chatEntry: StreamerEntry = {
              match_id: uniqueMatchId,
              riot_id: p.riot_id,
              platform: chatPlug.platform,
              channel: chatPlug.channel,
              title:
                chatPlug.matchType === 'live-mention'
                  ? `Mentioned live on ${chatPlug.platform} in match chat`
                  : `Shared ${chatPlug.platform} stream link in match chat`,
              url: chatPlug.url,
              timestamp,
              detected_at_iso: isoNow,
              team_relation: p.team_relation,
              map_name: scenario.map_name,
              agent_name: p.agent_name,
              queue_name: scenario.queue_name,
              chat_message: chatPlug.rawMessage,
              source: 'Chat',
              confidence: 92,
              confidence_tier: 'Confirmed',
              server_match_status: `Self-reported in ${regionForScan.shortLabel} match chat`,
              match_reason: 'Extracted from post-match in-game chat',
              server_region: regionForScan.shortLabel,
            };
            foundEntries.push(chatEntry);
            if (!playerTopEntry || (chatEntry.confidence ?? 0) > (playerTopEntry.confidence ?? 0)) {
              playerTopEntry = chatEntry;
            }
            pushLog(
              'chat',
              `CHAT PLUG [${chatPlug.platform}] ${p.riot_id} said "${chatPlug.rawMessage}" -> extracted ${chatPlug.url}`
            );
          }

          if (playerTopEntry) {
            playerAudits.push({
              riot_id: p.riot_id,
              team_relation: p.team_relation,
              agent_name: p.agent_name,
              outcome: 'Detected',
              confidence_tier: playerTopEntry.confidence_tier ?? 'Unverified',
              confidence: playerTopEntry.confidence ?? 0,
              summary: `${playerTopEntry.channel} on ${playerTopEntry.platform} (${playerTopEntry.source ?? 'Heuristic'}) — ${playerTopEntry.match_reason ?? ''}`,
              steps,
            });
          } else {
            playerAudits.push({
              riot_id: p.riot_id,
              team_relation: p.team_relation,
              agent_name: p.agent_name,
              outcome: 'No Match',
              confidence_tier: '',
              confidence: 0,
              summary: p.rejectedCrossRegion
                ? `Cross-region false positive rejected (${regionForScan.shortLabel} server guard)`
                : 'Checked Directory, YouTube, Twitch, Kick & Match Chat — no live match',
              steps,
            });
          }
        }

        const outcomeRank = (o: LastMatchPlayerAudit['outcome']) =>
          o === 'Detected' ? 0 : o === 'No Match' ? 1 : o === 'Party Skipped' ? 2 : 3;
        playerAudits.sort(
          (a, b) =>
            outcomeRank(a.outcome) - outcomeRank(b.outcome) ||
            (b.confidence ?? 0) - (a.confidence ?? 0) ||
            a.riot_id.toLowerCase().localeCompare(b.riot_id.toLowerCase())
        );

        const playersChecked = scannablePlayers.length;
        setLastScannedMatch({
          match_id: uniqueMatchId,
          queue_name: scenario.queue_name,
          map_name: scenario.map_name,
          my_agent: 'Jett',
          players_checked: playersChecked,
          party_skipped: partySkipped,
          hidden_skipped: 0,
          streamers_found: foundEntries.length,
          scanned_at: timestamp,
          server_name: regionForScan.label,
          player_audits: playerAudits,
        });

        if (foundEntries.length > 0) {
          setHistory((prev) => [...foundEntries, ...prev].slice(0, 300));
          const notifiable = foundEntries.find(
            (e) => resolveEntryTier(e) !== 'Unverified'
          );
          setStatus(
            `Found ${foundEntries.length} stream match(es) in ${scenario.queue_name} on ${scenario.map_name} (${regionForScan.shortLabel})!`
          );
          if (notifiable) {
            showWinRtToast(notifiable);
            showInlineToast(
              `Found ${foundEntries.length} match(es) on ${regionForScan.shortLabel}${partySkipped > 0 ? ` (${partySkipped} party skipped)` : ''} — top URL copied!`
            );
            if (navigator.clipboard?.writeText) {
              navigator.clipboard.writeText(notifiable.url).catch(() => {});
            }
          } else {
            showInlineToast(
              `Recorded ${foundEntries.length} unverified match(es) — no desktop toast fired`
            );
          }
        } else {
          setStatus(
            `Scanned ${scenario.queue_name} on ${scenario.map_name} (${playersChecked} scanned, ${partySkipped} party skipped, ${regionForScan.shortLabel}) | No live streamers found`
          );
        }

        setIsScanning(false);
      }, 1700);

      scanTimersRef.current.push(t1, t2, t3);
    },
    [
      isScanning,
      valorantRunning,
      selectedScenarioId,
      config,
      customDirectory.length,
      communityDirectory.length,
      pushLog,
      showInlineToast,
      showWinRtToast,
    ]
  );

  const handleSimulateEnterMatch = () => {
    const scenario =
      SIM_SCENARIOS.find((s) => s.id === selectedScenarioId) ?? SIM_SCENARIOS[0];
    const reg = getServerRegionInfo(scenario.serverRegion);
    setInMatch(true);
    setDetectedRegion(scenario.serverRegion);
    setStatus(`In Match (${reg.shortLabel}) • Waiting for match end...`);
    pushLog('server', `ShooterGame.log: Game server ${reg.podExample} -> ${reg.label}`);
    pushLog('log', 'ShooterGame.log: Reconcile state transitioned -> InGame');
    showInlineToast(`Entered simulated ${scenario.map_name} match (${reg.label})`);
  };

  const handleSimulateEndMatch = () => {
    setInMatch(false);
    setStatus('Match ended • Waiting 4s for Riot match history...');
    pushLog(
      'log',
      'ShooterGame.log: Reconcile state transitioned InGame -> MainMenu (queuing PostMatch scan in 4s)'
    );
    const t = window.setTimeout(() => {
      runSimulatedScan('PostMatch');
    }, 1200);
    scanTimersRef.current.push(t);
  };

  const handleSaveStreamer = (
    entry: DirectoryStreamer,
    openGithub: boolean,
    original: DirectoryStreamer | null
  ) => {
    setCustomDirectory((prev) => {
      const filtered = prev.filter((item) => {
        if (
          original &&
          item.riot_id.toLowerCase() === original.riot_id.toLowerCase() &&
          item.platform === original.platform
        ) {
          return false;
        }
        return !(
          item.riot_id.toLowerCase() === entry.riot_id.toLowerCase() &&
          item.platform === entry.platform
        );
      });
      return [entry, ...filtered];
    });
    showInlineToast(
      openGithub
        ? `Saved ${entry.riot_id} locally & prepared moderated submission!`
        : `Saved ${entry.riot_id} to custom_streamers.json`
    );
    pushLog(
      'log',
      `Atomic write custom_streamers.json: saved ${entry.riot_id} (${entry.platform})`
    );
  };

  const handleDeleteCustomEntry = (riotId: string, platform: Platform) => {
    setCustomDirectory((prev) =>
      prev.filter(
        (x) =>
          !(x.riot_id.toLowerCase() === riotId.toLowerCase() && x.platform === platform)
      )
    );
    showInlineToast(`Removed ${riotId} (${platform})`);
  };

  const handleSyncDirectory = () => {
    setIsSyncingDirectory(true);
    pushLog('log', 'Syncing community streamers.json database...');
    window.setTimeout(() => {
      setCommunityDirectory(INITIAL_COMMUNITY_DIRECTORY);
      setIsSyncingDirectory(false);
      showInlineToast('Synced central streamers.json database!');
      pushLog('log', 'Updated directory_cache.json with validated entries.');
    }, 650);
  };

  const handleCopyLink = (url: string) => {
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(url).catch(() => {});
    }
    showInlineToast('Copied link to clipboard!');
  };

  const handleWatchStream = (entry: StreamerEntry) => {
    showInlineToast(`Opening ${entry.platform}: ${entry.channel} (${entry.url})`);
    pushLog('log', `open::that("${entry.url}")`);
  };

  const handleSaveConfig = (nextConfig: AppConfig) => {
    setConfig(nextConfig);
    showInlineToast('Settings saved to config.json!');
    pushLog(
      'log',
      `Saved config.json (server=${nextConfig.server_override}, skip_party=${nextConfig.skip_party_members}, recommendations=${nextConfig.enable_recommendations}, autostart=${nextConfig.start_with_windows})`
    );
  };

  const handleToggleRecommendationsImmediate = (enabled: boolean) => {
    setConfig((prev) => ({ ...prev, enable_recommendations: enabled }));
    showInlineToast(
      enabled
        ? 'Live Streamer Recommendations enabled'
        : 'Live Streamer Recommendations turned OFF (saves bandwidth)'
    );
    pushLog('log', `Live Streamer Recommendations toggled ${enabled ? 'ON' : 'OFF'}`);
  };

  const handlePreviewToast = () => {
    const sample =
      history.find((h) => resolveEntryTier(h) !== 'Unverified') ?? {
        match_id: 'preview-toast',
        riot_id: 'TylerFPS#NA1',
        platform: 'YouTube' as Platform,
        channel: 'TylerFPS',
        title: 'ROAD TO IMMORTAL 3 NA EAST | LIVE RANKED GRIND',
        url: 'https://www.youtube.com/watch?v=val_tyler_live_01',
        timestamp: formatRadarTimestamp(new Date()),
        team_relation: 'Enemy' as const,
        map_name: 'Ascent',
        agent_name: 'Jett',
        queue_name: 'Competitive',
        confidence_tier: 'Confirmed' as const,
      };
    showWinRtToast(sample);
  };

  const viewBreadcrumbs: Record<StudioView, string> = {
    simulator: 'Dashboard',
    lab: 'Match & Tray Simulator',
  };

  if (screenshotMode) {
    return (
      <div className="relative flex min-h-screen w-full items-center justify-center bg-[#060609] p-6 sm:p-10">
        <div className="w-full max-w-[1120px]">
          <DesktopWindow
            visible={windowVisible}
            activeTab={desktopTab}
            onChangeTab={setDesktopTab}
            status={status}
            isScanning={isScanning}
            isSyncingDirectory={isSyncingDirectory}
            valorantRunning={valorantRunning}
            detectedRegion={detectedRegion}
            config={config}
            history={history}
            lastScannedMatch={lastScannedMatch}
            customDirectory={customDirectory}
            communityDirectory={communityDirectory}
            toastMessage={inlineToast}
            onTriggerManualScan={() => runSimulatedScan('Manual')}
            onOpenAddModal={(entry) => {
              setPrefillRec(null);
              setEditingEntry(entry ?? null);
              setModalOpen(true);
            }}
            onOpenAddFromRecommendation={(rec) => {
              setEditingEntry(null);
              setPrefillRec(rec);
              setModalOpen(true);
            }}
            onDeleteCustomEntry={handleDeleteCustomEntry}
            onSyncDirectory={handleSyncDirectory}
            onClearHistory={() => {
              setHistory([]);
              showInlineToast('Cleared streamer_history.json');
            }}
            onCopyLink={handleCopyLink}
            onWatchStream={handleWatchStream}
            onSaveConfig={handleSaveConfig}
            onToggleRecommendationsImmediate={handleToggleRecommendationsImmediate}
            onHideToTray={() => {
              setWindowVisible(false);
              pushLog('log', 'Window close intercepted -> hidden to system tray.');
            }}
            onRestoreFromTray={() => setWindowVisible(true)}
          />
        </div>

        {/* Unobtrusive outside-frame toggle to return to full simulator lab */}
        <button
          type="button"
          onClick={() => setScreenshotMode(false)}
          className="fixed right-3 bottom-3 inline-flex items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#0B0B0E]/85 px-2.5 py-1.5 text-[11px] font-medium text-[#8E95A5] opacity-40 transition-opacity hover:border-[#FF5CA8]/50 hover:text-[#FFF5FA] hover:opacity-100"
          title="Return to full Match & Tray simulator controls"
        >
          <Monitor className="h-3 w-3" />
          Exit Screenshot View
        </button>

        <StreamerModal
          isOpen={modalOpen}
          editingEntry={editingEntry}
          prefillRecommendation={prefillRec}
          defaultServerRegion={effectiveRegion.code}
          onClose={() => {
            setModalOpen(false);
            setEditingEntry(null);
            setPrefillRec(null);
          }}
          onSave={handleSaveStreamer}
        />

        <WinRtToast
          entry={winRtToastEntry}
          useCustomAppId={true}
          onDismiss={() => setWinRtToastEntry(null)}
          onOpenStream={(entry) => {
            handleWatchStream(entry);
            setWinRtToastEntry(null);
          }}
        />
      </div>
    );
  }

  return (
    <div className="flex min-h-screen flex-col bg-[#000000] text-[#F4F4F6] lg:flex-row">
      {/* Sidebar Navigation Rail */}
      <aside className="w-full shrink-0 border-b border-[#1F1F26] bg-[#000000] lg:w-[240px] lg:border-r lg:border-b-0">
        <div className="flex h-full flex-col justify-between p-4">
          <div>
            <div className="flex items-center gap-2.5 px-2 py-1.5">
              <RadarIcon size={24} className="shrink-0" />
              <span
                className="text-lg font-bold tracking-wide whitespace-nowrap"
                style={{ fontFamily: 'var(--font-display)' }}
              >
                <span className="text-pink-white-gradient">VALORANT</span>{' '}
                <span className="text-[#FFF5FA]">RADAR</span>
              </span>
            </div>

            <nav className="mt-6 space-y-1" aria-label="Workspace navigation">
              {(
                [
                  ['simulator', 'Dashboard', Monitor],
                  ['lab', 'Match & Tray Lab', Activity],
                ] as const
              ).map(([id, label, IconComp]) => {
                const active = studioView === id;
                return (
                  <button
                    key={id}
                    type="button"
                    onClick={() => setStudioView(id)}
                    className={`flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-xs font-medium transition-colors whitespace-nowrap ${
                      active
                        ? 'border border-[#FF5CA8]/40 bg-[#0B0B0E] text-[#FFF5FA]'
                        : 'border border-transparent text-[#8E95A5] hover:bg-[#0B0B0E] hover:text-white'
                    }`}
                  >
                    <IconComp
                      className={`h-4 w-4 shrink-0 ${
                        active ? 'text-[#FF5CA8]' : 'text-[#5A6275]'
                      }`}
                    />
                    <span>{label}</span>
                  </button>
                );
              })}
            </nav>

            <div className="mt-6 border-t border-[#1F1F26] px-2 pt-5">
              <p className="text-[11px] font-semibold text-[#8E95A5]">
                Active Radar Profile
              </p>
              <dl className="mt-2.5 space-y-2 text-xs">
                <div>
                  <dt className="text-[11px] text-[#5A6275]">Active Server</dt>
                  <dd className="font-mono text-[11px] text-[#2DD4BF]">
                    {effectiveRegion.label}
                  </dd>
                </div>
                <div>
                  <dt className="text-[11px] text-[#5A6275]">Party Member Skip</dt>
                  <dd className="font-mono text-[11px] text-[#38BDF8]">
                    {config.skip_party_members ? 'ON (Saves API Calls)' : 'OFF'}
                  </dd>
                </div>
                <div>
                  <dt className="text-[11px] text-[#5A6275]">Recommendations</dt>
                  <dd className="font-mono text-[11px] text-[#F4F4F6]">
                    {config.enable_recommendations ? 'ON (Discover)' : 'OFF (Bandwidth Saver)'}
                  </dd>
                </div>
                <div>
                  <dt className="text-[11px] text-[#5A6275]">In-Game Chat Reader</dt>
                  <dd className="font-mono text-[11px] text-[#FFA6D2]">
                    {config.capture_in_game_chat ? 'ON (Post-Match)' : 'OFF'}
                  </dd>
                </div>
              </dl>

              <button
                type="button"
                onClick={() => {
                  setStudioView('simulator');
                  setDesktopTab('settings');
                  setWindowVisible(true);
                }}
                className="mt-4 inline-flex w-full items-center justify-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#0B0B0E] px-3 py-1.5 text-xs font-medium text-[#F4F4F6] transition-colors hover:border-[#FF5CA8]"
              >
                <Settings className="h-3.5 w-3.5 text-[#FFA6D2]" />
                Open Settings
              </button>
            </div>
          </div>

          <div className="mt-6 hidden border-t border-[#1F1F26] px-2 pt-3 text-[11px] text-[#5A6275] lg:block">
            <p className="font-mono">v{APP_VERSION} · Post-Match Only</p>
            <p className="mt-0.5 text-[10px] text-[#5A6275]">Not a stream sniping tool</p>
          </div>
        </div>
      </aside>

      {/* Main Content Area */}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between gap-8 border-b border-[#1F1F26] bg-[#000000] px-6 py-3.5">
          <div className="flex min-w-0 items-center gap-2 text-xs text-[#8E95A5] whitespace-nowrap">
            <span className="font-medium text-[#FFF5FA]">Valorant Streamer Radar</span>
            <span className="text-[#1F1F26]">/</span>
            <span className="truncate text-[#8E95A5]">{viewBreadcrumbs[studioView]}</span>
          </div>

          <div className="flex shrink-0 items-center gap-3">
            <button
              type="button"
              onClick={() => {
                setStudioView('simulator');
                setWindowVisible(true);
                setScreenshotMode(true);
              }}
              className="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#0B0B0E] px-3.5 py-2 text-xs font-semibold text-[#F4F4F6] transition-colors hover:border-[#FF5CA8]/50 hover:bg-[#121216] whitespace-nowrap"
            >
              <Camera className="h-3.5 w-3.5 text-[#FFA6D2]" />
              Screenshot View
            </button>
            <button
              type="button"
              onClick={handlePreviewToast}
              className="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-[#FF5CA8]/40 bg-[#121216] px-3.5 py-2 text-xs font-semibold text-[#FFF5FA] transition-colors hover:border-[#FF5CA8] hover:bg-[#1A1A20] whitespace-nowrap"
            >
              <Bell className="h-3.5 w-3.5 text-[#FFA6D2]" />
              Test Toast Notification
            </button>
          </div>
        </header>

        <main className="mx-auto w-full max-w-[1320px] flex-1 p-6">
          {studioView === 'simulator' && (
            <DesktopWindow
              visible={windowVisible}
              activeTab={desktopTab}
              onChangeTab={setDesktopTab}
              status={status}
              isScanning={isScanning}
              isSyncingDirectory={isSyncingDirectory}
              valorantRunning={valorantRunning}
              detectedRegion={detectedRegion}
              config={config}
              history={history}
              lastScannedMatch={lastScannedMatch}
              customDirectory={customDirectory}
              communityDirectory={communityDirectory}
              toastMessage={inlineToast}
              onTriggerManualScan={() => runSimulatedScan('Manual')}
              onOpenAddModal={(entry) => {
                setPrefillRec(null);
                setEditingEntry(entry ?? null);
                setModalOpen(true);
              }}
              onOpenAddFromRecommendation={(rec) => {
                setEditingEntry(null);
                setPrefillRec(rec);
                setModalOpen(true);
              }}
              onDeleteCustomEntry={handleDeleteCustomEntry}
              onSyncDirectory={handleSyncDirectory}
              onClearHistory={() => {
                setHistory([]);
                showInlineToast('Cleared streamer_history.json');
              }}
              onCopyLink={handleCopyLink}
              onWatchStream={handleWatchStream}
              onSaveConfig={handleSaveConfig}
              onToggleRecommendationsImmediate={handleToggleRecommendationsImmediate}
              onHideToTray={() => {
                setWindowVisible(false);
                pushLog('log', 'Window close intercepted -> hidden to system tray.');
              }}
              onRestoreFromTray={() => setWindowVisible(true)}
            />
          )}

          {studioView === 'lab' && (
            <MatchTrayLab
              scenarios={SIM_SCENARIOS}
              selectedScenarioId={selectedScenarioId}
              onSelectScenario={setSelectedScenarioId}
              valorantRunning={valorantRunning}
              inMatch={inMatch}
              isScanning={isScanning}
              windowVisible={windowVisible}
              skipPartyMembers={config.skip_party_members}
              effectiveRegion={effectiveRegion}
              logs={logs}
              onToggleValorant={() => {
                setValorantRunning((prev) => {
                  const next = !prev;
                  if (!next) {
                    setInMatch(false);
                    setStatus('Idle • Waiting for Riot Client lockfile');
                    pushLog('log', 'Riot Client lockfile removed.');
                  } else {
                    setStatus('Idle • Monitoring ShooterGame.log');
                    pushLog('log', 'Riot Client lockfile detected.');
                  }
                  return next;
                });
              }}
              onSimulateEnterMatch={handleSimulateEnterMatch}
              onSimulateEndMatch={handleSimulateEndMatch}
              onRunManualScan={() => runSimulatedScan('Manual')}
              onPreviewToast={handlePreviewToast}
              onOpenWindowFromTray={() => {
                setWindowVisible(true);
                setStudioView('simulator');
              }}
              onHideWindowToTray={() => {
                setWindowVisible(false);
                showInlineToast('Window hidden to Windows system tray');
              }}
              onClearLogs={() => setLogs([])}
            />
          )}
        </main>
      </div>

      {/* Add / Edit Streamer Modal */}
      <StreamerModal
        isOpen={modalOpen}
        editingEntry={editingEntry}
        prefillRecommendation={prefillRec}
        defaultServerRegion={effectiveRegion.code}
        onClose={() => {
          setModalOpen(false);
          setEditingEntry(null);
          setPrefillRec(null);
        }}
        onSave={handleSaveStreamer}
      />

      {/* Simulated Windows 11 WinRT Toast Notification */}
      <WinRtToast
        entry={winRtToastEntry}
        useCustomAppId={true}
        onDismiss={() => setWinRtToastEntry(null)}
        onOpenStream={(entry) => {
          handleWatchStream(entry);
          setWinRtToastEntry(null);
        }}
      />
    </div>
  );
}
