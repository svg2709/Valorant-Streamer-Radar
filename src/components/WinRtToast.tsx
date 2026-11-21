import React from 'react';
import { ExternalLink, Terminal, X } from 'lucide-react';
import { StreamerEntry } from '../types/radar';
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

  return (
    <div
      className="fixed right-5 bottom-5 z-40 w-[364px] rounded-lg border border-[#2B3A48] bg-[#16202A] p-4 shadow-2xl transition-all duration-200"
      role="status"
      aria-live="polite"
    >
      <div className="flex items-center justify-between gap-2 pb-2.5 border-b border-[#2B3A48]/70">
        <div className="flex items-center gap-2 min-w-0">
          {useCustomAppId ? (
            <>
              <RadarIcon size={18} className="shrink-0" />
              <span className="truncate text-xs font-semibold text-[#ECE8E1]">
                Valorant Streamer Radar
              </span>
            </>
          ) : (
            <>
              <span className="flex h-[18px] w-[18px] items-center justify-center rounded-sm bg-[#012456] text-white shrink-0">
                <Terminal className="h-3 w-3" />
              </span>
              <span className="truncate text-xs font-semibold text-[#94A3B8]">
                Windows PowerShell
              </span>
            </>
          )}
        </div>
        <button
          type="button"
          onClick={onDismiss}
          className="rounded p-0.5 text-[#94A3B8] hover:bg-[#21303F] hover:text-white"
          aria-label="Dismiss Windows notification"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>

      <button
        type="button"
        onClick={() => onOpenStream(entry)}
        className="mt-2.5 block w-full text-left group"
      >
        <p className="font-mono text-xs font-semibold text-white group-hover:text-[#FF4655] transition-colors">
          [{entry.platform}] LIVE: {entry.riot_id}
          {relationSuffix}
        </p>
        <p className="mt-1 line-clamp-2 text-xs text-[#ECE8E1]">
          {entry.title} ({entry.channel})
        </p>
        <div className="mt-2 flex items-center justify-between text-[11px] text-[#94A3B8]">
          <span>Click to open stream · link copied to clipboard</span>
          <ExternalLink className="h-3 w-3 shrink-0 text-[#FF4655]" />
        </div>
      </button>
    </div>
  );
};
