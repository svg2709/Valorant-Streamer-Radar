import React, { useState } from 'react';
import {
  Bell,
  ChevronUp,
  Play,
  Power,
  Radio,
  RotateCcw,
  Square,
} from 'lucide-react';
import {
  LogLineEntry,
  SHARD_LABELS,
  ShardCode,
  SimMatchScenario,
} from '../types/radar';
import { RadarIcon } from './RadarIcon';

interface MatchTrayLabProps {
  scenarios: SimMatchScenario[];
  selectedScenarioId: string;
  onSelectScenario: (id: string) => void;
  valorantRunning: boolean;
  inMatch: boolean;
  isScanning: boolean;
  windowVisible: boolean;
  effectiveShard: ShardCode;
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
  log: '#94A3B8',
  shard: '#38BDF8',
  riot: '#A78BFA',
  scan: '#ECE8E1',
  hit: '#22C55E',
};

export const MatchTrayLab: React.FC<MatchTrayLabProps> = ({
  scenarios,
  selectedScenarioId,
  onSelectScenario,
  valorantRunning,
  inMatch,
  isScanning,
  windowVisible,
  effectiveShard,
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

  const selectedScenario =
    scenarios.find((s) => s.id === selectedScenarioId) ?? scenarios[0];

  return (
    <div className="space-y-6">
      <div className="grid gap-6 xl:grid-cols-12">
        {/* Left: Interactive Pipeline Controls */}
        <div className="space-y-4 xl:col-span-7">
          <div className="rounded-lg border border-[#2B3A48] bg-[#182430] p-5">
            <h3 className="text-sm font-semibold text-white">
              Post-Match Detection Pipeline Simulator
            </h3>
            <p className="mt-1 text-xs leading-relaxed text-[#94A3B8]">
              Step through the exact user-space lifecycle of{' '}
              <span className="font-mono text-[#ECE8E1]">val-streamer-notifier</span>:
              lockfile detection, passive shard discovery from{' '}
              <span className="font-mono text-[#ECE8E1]">ShooterGame.log</span>,
              <span className="font-mono text-[#ECE8E1]"> InGame &rarr; MainMenu</span>{' '}
              transition, concurrent 9-player scan, and WinRT toast notification.
            </p>

            {/* Scenario picker */}
            <div className="mt-4">
              <label className="mb-1.5 block text-xs font-medium text-[#ECE8E1]">
                Simulated Match Scenario
              </label>
              <div className="grid gap-2 sm:grid-cols-3">
                {scenarios.map((sc) => {
                  const active = sc.id === selectedScenario.id;
                  return (
                    <button
                      key={sc.id}
                      type="button"
                      disabled={isScanning}
                      onClick={() => onSelectScenario(sc.id)}
                      className={`rounded-md border px-3 py-2 text-left transition-colors ${
                        active
                          ? 'border-[#FF4655] bg-[#21303F] text-white'
                          : 'border-[#2B3A48] bg-[#0F1923] text-[#94A3B8] hover:text-white'
                      }`}
                    >
                      <p className="font-mono text-[11px] font-semibold uppercase text-[#FF4655]">
                        {SHARD_LABELS[sc.shard]}
                      </p>
                      <p className="mt-0.5 text-xs font-medium text-white">
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
                    ? 'border-[#22C55E]/60 bg-[#0F1923] text-[#22C55E]'
                    : 'border-[#2B3A48] bg-[#21303F] text-[#ECE8E1] hover:bg-[#2a3c4e]'
                }`}
              >
                <Power className="h-3.5 w-3.5" />
                {valorantRunning ? 'Valorant Lockfile Active' : 'Start Valorant Client'}
              </button>

              <button
                type="button"
                disabled={!valorantRunning || inMatch || isScanning}
                onClick={onSimulateEnterMatch}
                className="inline-flex items-center gap-1.5 rounded-md border border-[#2B3A48] bg-[#21303F] px-3.5 py-2 text-xs font-semibold text-[#ECE8E1] transition-colors hover:bg-[#2a3c4e] disabled:cursor-not-allowed disabled:opacity-45 whitespace-nowrap"
              >
                <Play className="h-3.5 w-3.5" />
                1. Enter Match (InGame)
              </button>

              <button
                type="button"
                disabled={!valorantRunning || !inMatch || isScanning}
                onClick={onSimulateEndMatch}
                className="inline-flex items-center gap-1.5 rounded-md bg-[#FF4655] px-4 py-2 text-xs font-semibold text-white transition-colors hover:bg-[#e53a49] disabled:cursor-not-allowed disabled:opacity-45 whitespace-nowrap"
              >
                <Square className="h-3.5 w-3.5" />
                2. End Match (MainMenu &rarr; Scan)
              </button>

              <button
                type="button"
                onClick={onPreviewToast}
                className="inline-flex items-center gap-1.5 rounded-md border border-[#2B3A48] bg-[#0F1923] px-3.5 py-2 text-xs font-medium text-[#ECE8E1] transition-colors hover:border-[#FF4655] whitespace-nowrap"
              >
                <Bell className="h-3.5 w-3.5" />
                Test WinRT Toast
              </button>
            </div>
          </div>

          {/* Scenario Roster Preview */}
          <div className="rounded-lg border border-[#2B3A48] bg-[#182430] p-5">
            <div className="flex items-center justify-between gap-2">
              <h4 className="text-xs font-semibold text-white">
                Post-Match Lobby Roster ({selectedScenario.map_name} ·{' '}
                {selectedScenario.queue_name})
              </h4>
              <span className="font-mono text-[11px] text-[#94A3B8]">
                9 other players · Semaphore(4)
              </span>
            </div>

            <div className="mt-3 overflow-x-auto">
              <table className="w-full text-left text-xs">
                <thead>
                  <tr className="border-b border-[#2B3A48] text-[11px] text-[#94A3B8]">
                    <th className="py-2 pr-3 font-medium">Riot ID</th>
                    <th className="py-2 pr-3 font-medium">Team</th>
                    <th className="py-2 pr-3 font-medium">Agent</th>
                    <th className="py-2 font-medium">Expected Detection</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[#2B3A48]/60">
                  {selectedScenario.players.map((p) => (
                    <tr key={p.riot_id} className="hover:bg-[#0F1923]/50">
                      <td className="py-2 pr-3 font-mono text-[#ECE8E1]">{p.riot_id}</td>
                      <td
                        className="py-2 pr-3 font-medium"
                        style={{
                          color: p.team_relation === 'Ally' ? '#2DD4BF' : '#FB923C',
                        }}
                      >
                        {p.team_relation}
                      </td>
                      <td className="py-2 pr-3 text-[#94A3B8]">{p.agent_name}</td>
                      <td className="py-2 font-mono text-[11px]">
                        {p.liveStream ? (
                          <span className="text-[#22C55E]">
                            LIVE ({p.liveStream.platform} · {p.detectionSource})
                          </span>
                        ) : (
                          <span className="text-[#64748B]">Not live</span>
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
          <div className="rounded-lg border border-[#2B3A48] bg-[#182430] p-5">
            <h3 className="text-sm font-semibold text-white">
              Windows System Tray Simulator
            </h3>
            <p className="mt-1 text-xs text-[#94A3B8]">
              Event-driven <span className="font-mono text-[#ECE8E1]">tray-icon 0.19</span>{' '}
              menu. Left-click restores the hidden window; right-click opens the tray menu.
            </p>

            <div className="relative mt-4">
              {trayMenuOpen && (
                <div className="mb-2 ml-auto w-56 rounded-lg border border-[#2B3A48] bg-[#0B131B] p-1.5 shadow-2xl">
                  <button
                    type="button"
                    onClick={() => {
                      onOpenWindowFromTray();
                      setTrayMenuOpen(false);
                    }}
                    className="flex w-full items-center justify-between rounded px-3 py-2 text-left text-xs text-[#ECE8E1] hover:bg-[#21303F]"
                  >
                    <span>Open Streamer History</span>
                    <span className="font-mono text-[10px] text-[#94A3B8]">Visible</span>
                  </button>
                  <button
                    type="button"
                    disabled={isScanning}
                    onClick={() => {
                      onRunManualScan();
                      setTrayMenuOpen(false);
                    }}
                    className="flex w-full items-center justify-between rounded px-3 py-2 text-left text-xs text-[#ECE8E1] hover:bg-[#21303F] disabled:opacity-45"
                  >
                    <span>Check Last Match Now</span>
                    <Radio className="h-3.5 w-3.5 text-[#FF4655]" />
                  </button>
                  <div className="my-1 border-t border-[#2B3A48]" />
                  <button
                    type="button"
                    onClick={() => {
                      onHideWindowToTray();
                      setTrayMenuOpen(false);
                    }}
                    className="flex w-full items-center justify-between rounded px-3 py-2 text-left text-xs text-[#94A3B8] hover:bg-[#21303F] hover:text-white"
                  >
                    <span>{windowVisible ? 'Hide Window to Tray' : 'Window Already Hidden'}</span>
                  </button>
                </div>
              )}

              {/* Simulated Windows 11 Taskbar Corner */}
              <div className="flex items-center justify-between rounded-md border border-[#2B3A48] bg-[#080E14] px-4 py-2.5">
                <div className="text-[11px] text-[#94A3B8]">
                  Active Shard:{' '}
                  <span className="font-mono text-[#ECE8E1]">
                    {SHARD_LABELS[effectiveShard]}
                  </span>
                </div>
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={() => setTrayMenuOpen((prev) => !prev)}
                    className="rounded p-1.5 text-[#94A3B8] transition-colors hover:bg-[#182430] hover:text-white"
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
                    className="rounded border border-[#2B3A48] bg-[#0F1923] p-1 transition-colors hover:border-[#FF4655]"
                  >
                    <RadarIcon size={18} />
                  </button>
                  <button
                    type="button"
                    onClick={() => setTrayMenuOpen((prev) => !prev)}
                    className="rounded border border-[#2B3A48] bg-[#21303F] px-2.5 py-1 text-[11px] font-medium text-[#ECE8E1] hover:bg-[#2a3c4e]"
                  >
                    Tray Menu
                  </button>
                </div>
              </div>
            </div>
          </div>

          {/* Live ShooterGame.log & radar.log Trace */}
          <div className="rounded-lg border border-[#2B3A48] bg-[#0B131B]">
            <div className="flex items-center justify-between border-b border-[#2B3A48] px-4 py-3">
              <span className="font-mono text-xs font-semibold text-[#ECE8E1]">
                ShooterGame.log &amp; radar.log Trace
              </span>
              <button
                type="button"
                onClick={onClearLogs}
                className="inline-flex items-center gap-1 text-[11px] text-[#94A3B8] hover:text-white"
              >
                <RotateCcw className="h-3 w-3" />
                Clear
              </button>
            </div>
            <div className="h-[340px] overflow-y-auto p-3.5 font-mono text-[11px] leading-relaxed">
              {logs.length === 0 ? (
                <p className="text-[#64748B]">
                  No log events yet. Start Valorant and trigger a match transition to watch
                  the background pipeline.
                </p>
              ) : (
                <div className="space-y-1.5">
                  {logs.map((line) => (
                    <div key={line.id} className="flex gap-2">
                      <span className="shrink-0 text-[#64748B] tabular-nums">
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
