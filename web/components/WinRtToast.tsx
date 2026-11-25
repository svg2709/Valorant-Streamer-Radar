import React from 'react';
import { ExternalLink, Terminal, X } from 'lucide-react';
import { StreamerEntry } from '../types/radar';
import { resolveEntryTier } from '../utils/validation';
import { RadarIcon } from './RadarIcon';

interface WinRtToastProps {
  entry: StreamerEntry | null;
  useCustomAppId: boolean;
  onDismiss: () => void;
  onOpenStream: (entry: StreamerEntry) => void;
}

/**
 * Interactive preview of the Windows 11 WinRT Toast notification triggered by
 * tauri-winrt-notification, demonstrating the HKCU AppUserModelID upgrade vs PowerShell fallback.
 */
export const WinRtToast: React.FC<WinRtToastProps> = ({
  entry,
  useCustomAppId,
  onDismiss,
  onOpenStream,
}) => {
  if (!entry) return null;

  const relationSuffix = entry.team_relation ? ` (${entry.team_relation})` : '';
  const tier = resolveEntryTier(entry);
  const tag =
    entry.source === 'Chat'
      ? 'CHAT LIVE'
      : tier === 'Confirmed'
      ? 'CONFIRMED LIVE'
      : 'PROBABLE LIVE';

  return (
    <div
      className="fixed right-5 bottom-5 z-40 w-[372px] overflow-hidden rounded-lg border border-[#FF5CA8]/45 bg-[#0B0B0E] shadow-2xl transition-all duration-200"
      role="status"
      aria-live="polite"
    >
      <div className="h-[2px] w-full bg-pink-white-strip" />
      <div className="p-4">
        <div className="flex items-center justify-between gap-2 border-b border-[#1F1F26] pb-2.5">
          <div className="flex min-w-0 items-center gap-2">
            {useCustomAppId ? (
              <>
                <RadarIcon size={18} className="shrink-0" />
                <span className="truncate text-xs font-semibold text-[#FFF5FA]">
                  Valorant Streamer Radar
                </span>
              </>
            ) : (
              <>
                <span className="flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-sm bg-[#012456] text-white">
                  <Terminal className="h-3 w-3" />
                </span>
                <span className="truncate text-xs font-semibold text-[#8E95A5]">
                  Windows PowerShell
                </span>
              </>
            )}
          </div>
          <button
            type="button"
            onClick={onDismiss}
            className="rounded p-0.5 text-[#8E95A5] hover:bg-[#1A1A20] hover:text-white"
            aria-label="Dismiss Windows notification"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>

        <button
          type="button"
          onClick={() => onOpenStream(entry)}
          className="group mt-2.5 block w-full text-left"
        >
          <p className="font-mono text-xs font-semibold text-[#FFF5FA] transition-colors group-hover:text-[#FFA6D2]">
            [{entry.platform}] {tag}: {entry.riot_id}
            {relationSuffix}
          </p>
          {entry.chat_message && entry.source === 'Chat' ? (
            <p className="mt-1 line-clamp-2 text-xs text-[#FFA6D2]">
              Chat: &ldquo;{entry.chat_message}&rdquo;
            </p>
          ) : (
            <p className="mt-1 line-clamp-2 text-xs text-[#F4F4F6]">
              {entry.title} ({entry.channel})
            </p>
          )}
          <div className="mt-2 flex items-center justify-between text-[11px] text-[#8E95A5]">
            <span>Click to open stream · link copied to clipboard</span>
            <ExternalLink className="h-3 w-3 shrink-0 text-[#FF5CA8]" />
          </div>
        </button>
      </div>
    </div>
  );
};
