import React, { useMemo, useState } from 'react';
import {
  Bell,
  ChevronUp,
  ExternalLink,
  MessageSquareQuote,
  Play,
  Power,
  Radio,
  RotateCcw,
  Square,
  Users,
} from 'lucide-react';
import {
  LogLineEntry,
  ServerRegionInfo,
  SimMatchScenario,
  getServerRegionInfo,
} from '../types/radar';
import { extractChatStreamPlug } from '../utils/validation';
import { RadarIcon } from './RadarIcon';

interface MatchTrayLabProps {
  scenarios: SimMatchScenario[];
  selectedScenarioId: string;
  onSelectScenario: (id: string) => void;
  valorantRunning: boolean;
  inMatch: boolean;
  isScanning: boolean;
  windowVisible: boolean;
  skipPartyMembers: boolean;
  effectiveRegion: ServerRegionInfo;
  logs: LogLineEntry[];
  onToggleValorant: () => void;
  onSimulateEnterMatch: () => void;
  onSimulateEndMatch: () => void;
  onRunManualScan: () => void;
  onPreviewToast: () => void;
  onOpenWindowFromTray: () => void;
  onHideWindowToTray: () => void;
  onClearLogs: () => void;
}

const CATEGORY_COLORS: Record<LogLineEntry['category'], string> = {
  log: '#8E95A5',
  server: '#38BDF8',
  riot: '#A770FF',
  chat: '#FFA6D2',
  scan: '#F4F4F6',
  hit: '#22C55E',
};

const SAMPLE_CHAT_LINES = [
  'gg guys im live on yt https://youtube.com/@RohanGaming',
  'yo im live on yt @NehaValPlays rn',
  'streaming this ranked game on ttv/vexfps',
  'im live on yt come watch',
];

export const MatchTrayLab: React.FC<MatchTrayLabProps> = ({
  scenarios,
  selectedScenarioId,
  onSelectScenario,
  valorantRunning,
  inMatch,
  isScanning,
  windowVisible,
  skipPartyMembers,
  effectiveRegion,
  logs,
  onToggleValorant,
  onSimulateEnterMatch,
  onSimulateEndMatch,
  onRunManualScan,
  onPreviewToast,
  onOpenWindowFromTray,
  onHideWindowToTray,
  onClearLogs,
}) => {
  const [trayMenuOpen, setTrayMenuOpen] = useState(false);
  const [sampleChat, setSampleChat] = useState(SAMPLE_CHAT_LINES[0]);
  const [sampleSender, setSampleSender] = useState('RohanYT#IND');

  const selectedScenario =
    scenarios.find((s) => s.id === selectedScenarioId) ?? scenarios[0];

  const partyCount = selectedScenario.players.filter((p) => p.isPartyMember).length;
  const scannableCount = skipPartyMembers
    ? selectedScenario.players.length - partyCount
    : selectedScenario.players.length;

  const extractedChatPlug = useMemo(
    () => extractChatStreamPlug(sampleChat, sampleSender),
    [sampleChat, sampleSender]
  );

  return (
    <div className="space-y-6">
      <div className="grid gap-6 xl:grid-cols-12">
        {/* Left: Interactive Pipeline Controls */}
        <div className="space-y-4 xl:col-span-7">
          <div className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-5">
            <h3 className="text-sm font-semibold text-[#FFF5FA]">
              Post-Match Server-Aware Detection Simulator
            </h3>
            <p className="mt-1 text-xs leading-relaxed text-[#8E95A5]">
              Step through the post-match lifecycle of{' '}
              <span className="font-mono text-[#F4F4F6]">ValorantStreamerRadar</span>:
              exact server detection (Mumbai, Tokyo, Manila, Austin), party-member
              skipping from Riot&apos;s <span className="font-mono text-[#F4F4F6]">partyId</span>,
              confidence-tiered matching (Confirmed / Probable / Unverified), cross-server
              false-positive rejection, and post-match chat stream plug extraction.
            </p>

            {/* Scenario picker */}
            <div className="mt-4">
              <label className="mb-1.5 block text-xs font-medium text-[#F4F4F6]">
                Simulated Match Server &amp; Scenario
              </label>
              <div className="grid gap-2 sm:grid-cols-2">
                {scenarios.map((sc) => {
                  const active = sc.id === selectedScenario.id;
                  const reg = getServerRegionInfo(sc.serverRegion);
                  return (
                    <button
                      key={sc.id}
                      type="button"
                      disabled={isScanning}
                      onClick={() => onSelectScenario(sc.id)}
                      className={`rounded-md border px-3 py-2 text-left transition-colors ${
                        active
                          ? 'border-[#FF5CA8] bg-[#121216] text-[#FFF5FA]'
                          : 'border-[#1F1F26] bg-[#000000] text-[#8E95A5] hover:border-[#2C2C36] hover:text-white'
                      }`}
                    >
                      <p className="font-mono text-[11px] font-semibold uppercase text-[#FFA6D2]">
                        {reg.label}
                      </p>
                      <p className="mt-0.5 text-xs font-medium text-[#FFF5FA]">
                        {sc.map_name} · {sc.queue_name}
                      </p>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Step controls */}
            <div className="mt-5 flex flex-wrap items-center gap-2.5">
              <button
                type="button"
                onClick={onToggleValorant}
                className={`inline-flex items-center gap-1.5 rounded-md border px-3.5 py-2 text-xs font-semibold transition-colors whitespace-nowrap ${
                  valorantRunning
                    ? 'border-[#22C55E]/50 bg-[#000000] text-[#22C55E]'
                    : 'border-[#1F1F26] bg-[#121216] text-[#F4F4F6] hover:bg-[#1A1A20]'
                }`}
              >
                <Power className="h-3.5 w-3.5" />
                {valorantRunning ? 'Valorant Lockfile Active' : 'Start Valorant Client'}
              </button>

              <button
                type="button"
                disabled={!valorantRunning || inMatch || isScanning}
                onClick={onSimulateEnterMatch}
                className="inline-flex items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#121216] px-3.5 py-2 text-xs font-semibold text-[#F4F4F6] transition-colors hover:bg-[#1A1A20] disabled:cursor-not-allowed disabled:opacity-45 whitespace-nowrap"
              >
                <Play className="h-3.5 w-3.5 text-[#FFA6D2]" />
                1. Enter Match (InGame)
              </button>

              <button
                type="button"
                disabled={!valorantRunning || !inMatch || isScanning}
                onClick={onSimulateEndMatch}
                className="inline-flex items-center gap-1.5 rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-4 py-2 text-xs font-bold text-[#000000] disabled:cursor-not-allowed disabled:opacity-45 whitespace-nowrap"
              >
                <Square className="h-3.5 w-3.5" />
                2. End Match (MainMenu &rarr; Scan)
              </button>

              <button
                type="button"
                onClick={onPreviewToast}
                className="inline-flex items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#000000] px-3.5 py-2 text-xs font-medium text-[#F4F4F6] transition-colors hover:border-[#FF5CA8] whitespace-nowrap"
              >
                <Bell className="h-3.5 w-3.5 text-[#FFA6D2]" />
                Test WinRT Toast
              </button>
            </div>
          </div>

          {/* Interactive In-Game Chat Stream Link Extractor */}
          <div className="rounded-lg border border-[#FF5CA8]/35 bg-[#0B0B0E] p-5">
            <div className="flex items-center gap-2">
              <MessageSquareQuote className="h-4 w-4 text-[#FF5CA8]" />
              <h4 className="text-xs font-semibold text-[#FFF5FA]">
                Post-Match Text Chat Stream Link Extractor
              </h4>
            </div>
            <p className="mt-1 text-xs text-[#8E95A5]">
              Test how <span className="font-mono text-[#F4F4F6]">extract_chat_stream_plug()</span> parses
              post-match chat plugs like &ldquo;im live on yt&rdquo;, handles, and stream URLs.
            </p>

            <div className="mt-3 grid gap-2.5 sm:grid-cols-3">
              <div>
                <label className="mb-1 block text-[11px] text-[#8E95A5]">Sender Riot ID</label>
                <input
                  type="text"
                  value={sampleSender}
                  onChange={(e) => setSampleSender(e.target.value)}
                  className="w-full rounded border border-[#1F1F26] bg-[#000000] px-2.5 py-1.5 font-mono text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                />
              </div>
              <div className="sm:col-span-2">
                <label className="mb-1 block text-[11px] text-[#8E95A5]">In-Game Chat Message</label>
                <input
                  type="text"
                  value={sampleChat}
                  onChange={(e) => setSampleChat(e.target.value)}
                  placeholder="Type a chat message like 'im live on yt @RohanGaming'"
                  className="w-full rounded border border-[#1F1F26] bg-[#000000] px-2.5 py-1.5 text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                />
              </div>
            </div>

            <div className="mt-2 flex flex-wrap gap-1.5">
              {SAMPLE_CHAT_LINES.map((preset) => (
                <button
                  key={preset}
                  type="button"
                  onClick={() => setSampleChat(preset)}
                  className="rounded border border-[#1F1F26] bg-[#000000] px-2 py-1 text-[10.5px] text-[#8E95A5] transition-colors hover:border-[#FF5CA8]/60 hover:text-[#FFF5FA]"
                >
                  {preset}
                </button>
              ))}
            </div>

            <div className="mt-3 rounded-md border border-[#1F1F26] bg-[#000000] p-3 text-xs">
              {extractedChatPlug ? (
                <div className="space-y-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-mono text-[11px] font-semibold text-[#FF5CA8]">
                      {extractedChatPlug.platform}
                    </span>
                    <span className="text-[#1F1F26]">/</span>
                    <span className="font-mono text-[11px] text-[#22C55E]">
                      Captured ({extractedChatPlug.matchType})
                    </span>
                    <span className="text-[#1F1F26]">/</span>
                    <span className="font-medium text-[#FFA6D2]">
                      {extractedChatPlug.channel}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-2 pt-1">
                    <span className="truncate font-mono text-[11px] text-[#F4F4F6]">
                      {extractedChatPlug.url}
                    </span>
                    <a
                      href={extractedChatPlug.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex shrink-0 items-center gap-1 text-[11px] font-semibold text-[#FFA6D2] hover:text-[#FFF5FA]"
                    >
                      Open
                      <ExternalLink className="h-3 w-3" />
                    </a>
                  </div>
                </div>
              ) : (
                <p className="text-[#5A6275]">
                  No stream link or &ldquo;live on YT / Twitch / Kick&rdquo; mention detected in this message.
                </p>
              )}
            </div>
          </div>

          {/* Scenario Roster Preview */}
          <div className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h4 className="text-xs font-semibold text-[#FFF5FA]">
                Post-Match Lobby Roster ({selectedScenario.map_name} ·{' '}
                {selectedScenario.queue_name})
              </h4>
              <div className="flex flex-wrap items-center gap-2 font-mono text-[11px] tabular-nums">
                <span className="text-[#2DD4BF]">
                  {getServerRegionInfo(selectedScenario.serverRegion).shortLabel} Server
                </span>
                <span className="text-[#1F1F26]">|</span>
                <span className="text-[#8E95A5]">
                  {scannableCount} scanned
                  {skipPartyMembers && partyCount > 0 ? ` · ${partyCount} party skipped` : ''}
                </span>
              </div>
            </div>

            {selectedScenario.chatLog && selectedScenario.chatLog.length > 0 && (
              <div className="mt-3 rounded-md border border-[#FF5CA8]/30 bg-[#000000] p-3">
                <p className="text-[10.5px] font-semibold tracking-wider text-[#FFA6D2] uppercase">
                  Buffered Match Text Chat ({selectedScenario.chatLog.length} messages)
                </p>
                <div className="mt-1.5 space-y-1 font-mono text-[11px]">
                  {selectedScenario.chatLog.map((c, idx) => (
                    <div key={`${c.riot_id}-${idx}`} className="text-[#8E95A5]">
                      <span className="text-[#FFF5FA]">{c.riot_id}:</span>{' '}
                      <span className="text-[#F4F4F6]">{c.body}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            <div className="mt-3 overflow-x-auto">
              <table className="w-full text-left text-xs">
                <thead>
                  <tr className="border-b border-[#1F1F26] text-[11px] text-[#8E95A5]">
                    <th className="py-2 pr-3 font-medium">Riot ID</th>
                    <th className="py-2 pr-3 font-medium">Team</th>
                    <th className="py-2 pr-3 font-medium">Agent</th>
                    <th className="py-2 font-medium">Expected Pipeline Outcome</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[#1F1F26]/70">
                  {selectedScenario.players.map((p) => (
                    <tr key={p.riot_id} className="hover:bg-[#121216]/60">
                      <td className="py-2 pr-3 font-mono text-[#F4F4F6]">
                        <div className="flex items-center gap-1.5">
                          <span>{p.riot_id}</span>
                          {p.isPartyMember && (
                            <span className="inline-flex items-center gap-0.5 text-[10px] font-semibold text-[#38BDF8]">
                              <Users className="h-2.5 w-2.5" />
                              Party
                            </span>
                          )}
                        </div>
                      </td>
                      <td
                        className="py-2 pr-3 font-medium"
                        style={{
                          color: p.team_relation === 'Ally' ? '#2DD4BF' : '#FB923C',
                        }}
                      >
                        {p.team_relation}
                      </td>
                      <td className="py-2 pr-3 text-[#8E95A5]">{p.agent_name}</td>
                      <td className="py-2 font-mono text-[11px]">
                        {p.isPartyMember && skipPartyMembers ? (
                          <span className="text-[#38BDF8]">
                            SKIPPED (Party member · 0 external API calls)
                          </span>
                        ) : p.liveStream ? (
                          <span
                            style={{
                              color:
                                p.liveStream.confidenceTier === 'Confirmed'
                                  ? '#22C55E'
                                  : p.liveStream.confidenceTier === 'Probable'
                                  ? '#FB923C'
                                  : '#8E95A5',
                            }}
                          >
                            {p.liveStream.confidenceTier.toUpperCase()} ({p.liveStream.platform} ·{' '}
                            {p.liveStream.confidence}%)
                          </span>
                        ) : p.detectionSource === 'chat' ? (
                          <span className="text-[#FFA6D2]">
                            CHAT PLUG (extracted post-match)
                          </span>
                        ) : (
                          <span className="text-[#5A6275]">Not live</span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </div>

        {/* Right: Simulated Windows System Tray & Live Trace Log */}
        <div className="space-y-4 xl:col-span-5">
          {/* Simulated Windows Taskbar System Tray */}
          <div className="rounded-lg border border-[#1F1F26] bg-[#0B0B0E] p-5">
            <h3 className="text-sm font-semibold text-[#FFF5FA]">
              Windows System Tray Simulator
            </h3>
            <p className="mt-1 text-xs text-[#8E95A5]">
              Event-driven <span className="font-mono text-[#F4F4F6]">tray-icon 0.19</span>{' '}
              menu. Left-click restores the hidden window; right-click opens the tray menu.
            </p>

            <div className="relative mt-4">
              {trayMenuOpen && (
                <div className="mb-2 ml-auto w-56 rounded-lg border border-[#1F1F26] bg-[#000000] p-1.5 shadow-2xl">
                  <button
                    type="button"
                    onClick={() => {
                      onOpenWindowFromTray();
                      setTrayMenuOpen(false);
                    }}
                    className="flex w-full items-center justify-between rounded px-3 py-2 text-left text-xs text-[#F4F4F6] hover:bg-[#1A1A20]"
                  >
                    <span>Open Streamer History</span>
                    <span className="font-mono text-[10px] text-[#8E95A5]">Visible</span>
                  </button>
                  <button
                    type="button"
                    disabled={isScanning}
                    onClick={() => {
                      onRunManualScan();
                      setTrayMenuOpen(false);
                    }}
                    className="flex w-full items-center justify-between rounded px-3 py-2 text-left text-xs text-[#F4F4F6] hover:bg-[#1A1A20] disabled:opacity-45"
                  >
                    <span>Check Last Match Now</span>
                    <Radio className="h-3.5 w-3.5 text-[#FF5CA8]" />
                  </button>
                  <div className="my-1 border-t border-[#1F1F26]" />
                  <button
                    type="button"
                    onClick={() => {
                      onHideWindowToTray();
                      setTrayMenuOpen(false);
                    }}
                    className="flex w-full items-center justify-between rounded px-3 py-2 text-left text-xs text-[#8E95A5] hover:bg-[#1A1A20] hover:text-white"
                  >
                    <span>{windowVisible ? 'Hide Window to Tray' : 'Window Already Hidden'}</span>
                  </button>
                </div>
              )}

              {/* Simulated Windows 11 Taskbar Corner */}
              <div className="flex items-center justify-between rounded-md border border-[#1F1F26] bg-[#000000] px-4 py-2.5">
                <div className="text-[11px] text-[#8E95A5]">
                  Active Server:{' '}
                  <span className="font-mono text-[#2DD4BF]">
                    {effectiveRegion.shortLabel}
                  </span>
                </div>
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={() => setTrayMenuOpen((prev) => !prev)}
                    className="rounded p-1.5 text-[#8E95A5] transition-colors hover:bg-[#1A1A20] hover:text-white"
                    title="Toggle tray context menu"
                  >
                    <ChevronUp
                      className={`h-3.5 w-3.5 transition-transform ${
                        trayMenuOpen ? 'rotate-180' : ''
                      }`}
                    />
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      onOpenWindowFromTray();
                      setTrayMenuOpen(false);
                    }}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      setTrayMenuOpen((prev) => !prev);
                    }}
                    title="Valorant Streamer Radar (Left-click: open window, Right-click: menu)"
                    className="rounded border border-[#1F1F26] bg-[#0B0B0E] p-1 transition-colors hover:border-[#FF5CA8]"
                  >
                    <RadarIcon size={18} />
                  </button>
                  <button
                    type="button"
                    onClick={() => setTrayMenuOpen((prev) => !prev)}
                    className="rounded border border-[#1F1F26] bg-[#121216] px-2.5 py-1 text-[11px] font-medium text-[#F4F4F6] hover:bg-[#1A1A20]"
                  >
                    Tray Menu
                  </button>
                </div>
              </div>
            </div>
          </div>

          {/* Live ShooterGame.log & radar.log Trace */}
          <div className="rounded-lg border border-[#1F1F26] bg-[#000000]">
            <div className="flex items-center justify-between border-b border-[#1F1F26] px-4 py-3">
              <span className="font-mono text-xs font-semibold text-[#F4F4F6]">
                ShooterGame.log &amp; radar.log Trace
              </span>
              <button
                type="button"
                onClick={onClearLogs}
                className="inline-flex items-center gap-1 text-[11px] text-[#8E95A5] hover:text-white"
              >
                <RotateCcw className="h-3 w-3" />
                Clear
              </button>
            </div>
            <div className="h-[380px] overflow-y-auto p-3.5 font-mono text-[11px] leading-relaxed">
              {logs.length === 0 ? (
                <p className="text-[#5A6275]">
                  No log events yet. Start Valorant and trigger a match transition to watch
                  the background pipeline.
                </p>
              ) : (
                <div className="space-y-1.5">
                  {logs.map((line) => (
                    <div key={line.id} className="flex gap-2">
                      <span className="shrink-0 text-[#5A6275] tabular-nums">
                        [{line.time}]
                      </span>
                      <span
                        className="break-all"
                        style={{ color: CATEGORY_COLORS[line.category] }}
                      >
                        {line.message}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
