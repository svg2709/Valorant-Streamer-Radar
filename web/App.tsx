import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Activity,
  Bell,
  Monitor,
  Settings,
} from 'lucide-react';
import {
  AppConfig,
  DEFAULT_CONFIG,
  DesktopTab,
  DirectoryStreamer,
  LastMatchSummary,
  LogLineEntry,
  Platform,
  SHARD_LABELS,
  ShardCode,
  StreamerEntry,
  StudioView,
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
  resolveEffectiveShard,
} from './utils/validation';
import { RadarIcon } from './components/RadarIcon';
import { DesktopWindow } from './components/DesktopWindow';
import { StreamerModal } from './components/StreamerModal';
import { MatchTrayLab } from './components/MatchTrayLab';
import { WinRtToast } from './components/WinRtToast';

const STORAGE_KEYS = {
  config: 'vsr_v050_config',
  history: 'vsr_v050_history',
  customDir: 'vsr_v050_custom_dir',
  lastMatch: 'vsr_v050_last_match',
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

  const [config, setConfig] = useState<AppConfig>(() => ({
    ...DEFAULT_CONFIG,
    ...loadStored<Partial<AppConfig>>(STORAGE_KEYS.config, DEFAULT_CONFIG),
  }));
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
  const [detectedShard, setDetectedShard] = useState<ShardCode>('ap');
  const [status, setStatus] = useState<string>('Idle | Monitoring ShooterGame.log');
  const [selectedScenarioId, setSelectedScenarioId] = useState<string>(SIM_SCENARIOS[0].id);
  const [lastScannedMatch, setLastScannedMatch] = useState<LastMatchSummary | null>(() =>
    loadStored<LastMatchSummary | null>(STORAGE_KEYS.lastMatch, {
      match_id: INITIAL_HISTORY[0]?.match_id ?? 'match-seed-01',
      queue_name: INITIAL_HISTORY[0]?.queue_name ?? 'Competitive',
      map_name: INITIAL_HISTORY[0]?.map_name ?? 'Ascent',
      my_agent: 'Omen',
      players_checked: 9,
      streamers_found: INITIAL_HISTORY.filter((h) => h.match_id === INITIAL_HISTORY[0]?.match_id).length,
      scanned_at: INITIAL_HISTORY[0]?.timestamp ?? formatRadarTimestamp(new Date()),
      shard: SHARD_LABELS.ap,
    })
  );

  // Modals & Toasts
  const [modalOpen, setModalOpen] = useState<boolean>(false);
  const [editingEntry, setEditingEntry] = useState<DirectoryStreamer | null>(null);
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
      category: 'shard',
      message: 'Initial ShooterGame.log scan detected shard https://pd.ap.a.pvp.net (AP / Mumbai).',
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

  const effectiveShard = resolveEffectiveShard(config, detectedShard);

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
      const shardForScan = resolveEffectiveShard(config, scenario.shard);

      setIsScanning(true);
      setDetectedShard(scenario.shard);
      setStatus(
        `Syncing directory & scanning last match (${SHARD_LABELS[shardForScan]})...`
      );

      pushLog(
        'riot',
        `[${trigger}] Lockfile authenticated -> GET https://127.0.0.1:52194/entitlements/v1/token`
      );

      const t1 = window.setTimeout(() => {
        pushLog(
          'riot',
          `GET https://pd.${shardForScan}.a.pvp.net/match-history/v1/history/{puuid} -> MatchID ${scenario.match_id}`
        );
      }, 450);

      const t2 = window.setTimeout(() => {
        pushLog(
          'scan',
          `Match details: ${scenario.map_name} (${scenario.queue_name}) · 9 players resolved via name-service/v2`
        );
        if (config.capture_in_game_chat && scenario.chatLog?.length) {
          pushLog(
            'chat',
            `GET https://127.0.0.1:52194/chat/v6/messages -> ${scenario.chatLog.length} match chat message(s) buffered`
          );
        }
        pushLog(
          'scan',
          `Merged ${customDirectory.length} custom + ${communityDirectory.length} community directory entries (${config.github_owner}/${config.github_repo})`
        );
      }, 950);

      const t3 = window.setTimeout(() => {
        const timestamp = formatRadarTimestamp(new Date());
        const uniqueMatchId = `${scenario.match_id}-${Date.now().toString().slice(-4)}`;
        const foundEntries: StreamerEntry[] = [];

        const chatByPlayer = new Map<string, string>();
        if (config.capture_in_game_chat && scenario.chatLog) {
          for (const line of scenario.chatLog) {
            chatByPlayer.set(line.riot_id.toLowerCase(), line.body);
          }
        }

        for (const p of scenario.players) {
          const chatBody = chatByPlayer.get(p.riot_id.toLowerCase());
          const chatPlug = chatBody ? extractChatStreamPlug(chatBody, p.riot_id) : null;

          if (p.liveStream) {
            const entry: StreamerEntry = {
              match_id: uniqueMatchId,
              riot_id: p.riot_id,
              platform: p.liveStream.platform,
              channel: p.liveStream.channel,
              title: p.liveStream.title,
              url: p.liveStream.url,
              timestamp,
              team_relation: p.team_relation,
              map_name: scenario.map_name,
              agent_name: p.agent_name,
              queue_name: scenario.queue_name,
              chat_message:
                chatPlug && chatPlug.platform === p.liveStream.platform
                  ? chatPlug.rawMessage
                  : undefined,
              source: p.detectionSource === 'directory' ? 'Directory' : 'Heuristic',
            };
            foundEntries.push(entry);
            pushLog(
              'hit',
              `LIVE HIT [${p.liveStream.platform}] ${p.riot_id} (${p.team_relation}, ${p.agent_name}) via ${p.detectionSource} -> ${p.liveStream.url}`
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
              team_relation: p.team_relation,
              map_name: scenario.map_name,
              agent_name: p.agent_name,
              queue_name: scenario.queue_name,
              chat_message: chatPlug.rawMessage,
              source: 'Chat',
            };
            foundEntries.push(chatEntry);
            pushLog(
              'chat',
              `CHAT PLUG [${chatPlug.platform}] ${p.riot_id} said "${chatPlug.rawMessage}" -> extracted ${chatPlug.url}`
            );
          }
        }

        const playersChecked = scenario.players.length;
        setLastScannedMatch({
          match_id: uniqueMatchId,
          queue_name: scenario.queue_name,
          map_name: scenario.map_name,
          my_agent: 'Jett',
          players_checked: playersChecked,
          streamers_found: foundEntries.length,
          scanned_at: timestamp,
          shard: SHARD_LABELS[shardForScan],
        });

        if (foundEntries.length > 0) {
          setHistory((prev) => [...foundEntries, ...prev].slice(0, 300));
          setStatus(
            `Found ${foundEntries.length} live stream(s) in ${scenario.queue_name} on ${scenario.map_name}!`
          );
          showWinRtToast(foundEntries[0]);
          showInlineToast(
            `Found ${foundEntries.length} live stream(s) in ${scenario.queue_name} (${scenario.map_name}) — first URL copied!`
          );
          if (navigator.clipboard?.writeText) {
            navigator.clipboard.writeText(foundEntries[0].url).catch(() => {});
          }
        } else {
          setStatus(
            `Scanned ${scenario.queue_name} on ${scenario.map_name} (${playersChecked} players) | No live streamers found`
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
    setInMatch(true);
    setDetectedShard(scenario.shard);
    setStatus('In Match • Waiting for match end...');
    pushLog(
      'shard',
      `ShooterGame.log: Request URL https://pd.${scenario.shard}.a.pvp.net/core-game/v1/matches`
    );
    pushLog('log', 'ShooterGame.log: Reconcile state transitioned -> InGame');
    showInlineToast(`Entered simulated ${scenario.map_name} match (${SHARD_LABELS[scenario.shard]})`);
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
        ? `Saved ${entry.riot_id} locally & prepared GitHub Issue!`
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
    pushLog(
      'log',
      `Fetching https://raw.githubusercontent.com/${config.github_owner}/${config.github_repo}/main/streamers.json`
    );
    window.setTimeout(() => {
      setCommunityDirectory(INITIAL_COMMUNITY_DIRECTORY);
      setIsSyncingDirectory(false);
      showInlineToast(
        `Synced directory cache for ${config.github_owner}/${config.github_repo}`
      );
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
      `Saved config.json (shard=${nextConfig.shard_override}, repo=${nextConfig.github_owner}/${nextConfig.github_repo}, autostart=${nextConfig.start_with_windows}, customToastAppId=${nextConfig.use_custom_toast_app_id})`
    );
  };

  const handlePreviewToast = () => {
    const sample = history[0] ?? {
      match_id: 'preview-toast',
      riot_id: 'RohanYT#IND',
      platform: 'YouTube',
      channel: 'Rohan Gaming',
      title: 'ROAD TO IMMORTAL 3 MUMBAI SERVER | LIVE RANKED GRIND',
      url: 'https://www.youtube.com/watch?v=val_rohan_live_01',
      timestamp: formatRadarTimestamp(new Date()),
      team_relation: 'Enemy',
      map_name: 'Ascent',
      agent_name: 'Jett',
      queue_name: 'Competitive',
    };
    showWinRtToast(sample);
  };

  const viewBreadcrumbs: Record<StudioView, string> = {
    simulator: 'Dashboard',
    lab: 'Match & Tray Simulator',
  };

  return (
    <div className="flex min-h-screen flex-col bg-[#090D14] text-[#F3EFF4] lg:flex-row">
      {/* Sidebar Navigation Rail */}
      <aside className="w-full shrink-0 border-b border-[#273144] bg-[#070A10] lg:w-[248px] lg:border-r lg:border-b-0">
        <div className="flex h-full flex-col justify-between p-4">
          <div>
            <div className="flex items-center gap-2.5 px-2 py-1.5">
              <RadarIcon size={26} className="shrink-0" />
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
                        ? 'border border-[#FF5CA8]/40 bg-[#141B27] text-[#FFF5FA]'
                        : 'border border-transparent text-[#98A2B8] hover:bg-[#0D111A] hover:text-white'
                    }`}
                  >
                    <IconComp
                      className={`h-4 w-4 shrink-0 ${
                        active ? 'text-[#FF5CA8]' : 'text-[#64748B]'
                      }`}
                    />
                    <span>{label}</span>
                  </button>
                );
              })}
            </nav>

            <div className="mt-6 border-t border-[#273144] px-2 pt-5">
              <p className="text-[11px] font-semibold text-[#98A2B8]">
                Active Radar Profile
              </p>
              <dl className="mt-2.5 space-y-2 text-xs">
                <div>
                  <dt className="text-[11px] text-[#64748B]">Directory Repo</dt>
                  <dd className="truncate font-mono text-[11px] text-[#F3EFF4]">
                    {config.github_owner}/{config.github_repo}
                  </dd>
                </div>
                <div>
                  <dt className="text-[11px] text-[#64748B]">Effective Shard</dt>
                  <dd className="font-mono text-[11px] text-[#F3EFF4]">
                    {SHARD_LABELS[effectiveShard]}
                  </dd>
                </div>
                <div>
                  <dt className="text-[11px] text-[#64748B]">In-Game Chat Reader</dt>
                  <dd className="font-mono text-[11px] text-[#FFA6D2]">
                    {config.capture_in_game_chat ? 'Enabled (Post-Match)' : 'Disabled'}
                  </dd>
                </div>
                <div>
                  <dt className="text-[11px] text-[#64748B]">Toast Branding</dt>
                  <dd className="font-mono text-[11px] text-[#F3EFF4]">
                    {config.use_custom_toast_app_id
                      ? 'Valorant Streamer Radar'
                      : 'Windows PowerShell'}
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
                className="mt-4 inline-flex w-full items-center justify-center gap-1.5 rounded-md border border-[#273144] bg-[#141B27] px-3 py-1.5 text-xs font-medium text-[#F3EFF4] transition-colors hover:border-[#FF5CA8]"
              >
                <Settings className="h-3.5 w-3.5 text-[#FFA6D2]" />
                Open Settings
              </button>
            </div>
          </div>

          <div className="mt-6 hidden border-t border-[#273144] px-2 pt-3 text-[11px] text-[#64748B] lg:block">
            <p className="font-mono">v1.0.0 · Post-Match Only</p>
          </div>
        </div>
      </aside>

      {/* Main Content Area */}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between gap-8 border-b border-[#273144] bg-[#090D14] px-6 py-3.5">
          <div className="flex min-w-0 items-center gap-2 text-xs text-[#98A2B8] whitespace-nowrap">
            <span className="font-medium text-[#FFF5FA]">Valorant Streamer Radar</span>
            <span className="text-[#273144]">/</span>
            <span className="truncate text-[#98A2B8]">{viewBreadcrumbs[studioView]}</span>
          </div>

          <div className="flex shrink-0 items-center gap-3">
            <button
              type="button"
              onClick={handlePreviewToast}
              className="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-[#FF5CA8]/40 bg-[#1D2636] px-3.5 py-2 text-xs font-semibold text-[#FFF5FA] transition-colors hover:border-[#FF5CA8] hover:bg-[#253044] whitespace-nowrap"
            >
              <Bell className="h-3.5 w-3.5 text-[#FFA6D2]" />
              Test Toast Notification
            </button>
          </div>
        </header>

        <main className="mx-auto w-full max-w-[1160px] flex-1 p-6">
          {studioView === 'simulator' && (
            <DesktopWindow
              visible={windowVisible}
              activeTab={desktopTab}
              onChangeTab={setDesktopTab}
              status={status}
              isScanning={isScanning}
              isSyncingDirectory={isSyncingDirectory}
              valorantRunning={valorantRunning}
              detectedShard={detectedShard}
              config={config}
              history={history}
              lastScannedMatch={lastScannedMatch}
              customDirectory={customDirectory}
              communityDirectory={communityDirectory}
              toastMessage={inlineToast}
              onTriggerManualScan={() => runSimulatedScan('Manual')}
              onOpenAddModal={(entry) => {
                setEditingEntry(entry ?? null);
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
              onHideToTray={() => {
                setWindowVisible(false);
                pushLog('log', 'Window close intercepted -> hidden to system tray (0% idle CPU).');
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
              effectiveShard={effectiveShard}
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
        githubOwner={config.github_owner}
        githubRepo={config.github_repo}
        onClose={() => {
          setModalOpen(false);
          setEditingEntry(null);
        }}
        onSave={handleSaveStreamer}
      />

      {/* Simulated Windows 11 WinRT Toast Notification */}
      <WinRtToast
        entry={winRtToastEntry}
        useCustomAppId={config.use_custom_toast_app_id}
        onDismiss={() => setWinRtToastEntry(null)}
        onOpenStream={(entry) => {
          handleWatchStream(entry);
          setWinRtToastEntry(null);
        }}
      />
    </div>
  );
}
