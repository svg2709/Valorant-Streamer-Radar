import React, { useEffect, useState } from 'react';
import { ExternalLink, X } from 'lucide-react';
import { DirectoryStreamer, Platform } from '../types/radar';
import { buildGithubIssueUrl, validateSubmission } from '../utils/validation';

interface StreamerModalProps {
  isOpen: boolean;
  editingEntry: DirectoryStreamer | null;
  githubOwner: string;
  githubRepo: string;
  onClose: () => void;
  onSave: (
    entry: DirectoryStreamer,
    openGithub: boolean,
    original: DirectoryStreamer | null
  ) => void;
}

const PLATFORMS: Platform[] = ['YouTube', 'Twitch', 'Kick'];

export const StreamerModal: React.FC<StreamerModalProps> = ({
  isOpen,
  editingEntry,
  githubOwner,
  githubRepo,
  onClose,
  onSave,
}) => {
  const [riotId, setRiotId] = useState('');
  const [platform, setPlatform] = useState<Platform>('YouTube');
  const [channelName, setChannelName] = useState('');
  const [channelUrl, setChannelUrl] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [submittedIssueUrl, setSubmittedIssueUrl] = useState<string | null>(null);

  useEffect(() => {
    if (isOpen) {
      if (editingEntry) {
        setRiotId(editingEntry.riot_id);
        setPlatform(editingEntry.platform);
        setChannelName(editingEntry.channel_name);
        setChannelUrl(editingEntry.channel_url);
      } else {
        setRiotId('');
        setPlatform('YouTube');
        setChannelName('');
        setChannelUrl('');
      }
      setError(null);
      setSubmittedIssueUrl(null);
    }
  }, [isOpen, editingEntry]);

  if (!isOpen) return null;

  const handleAction = (openGithub: boolean) => {
    const rid = riotId.trim();
    const cname = channelName.trim();
    const curl = channelUrl.trim();

    const validationError = validateSubmission(rid, platform, cname, curl);
    if (validationError) {
      setError(validationError);
      return;
    }

    const entry: DirectoryStreamer = {
      riot_id: rid,
      platform,
      channel_name: cname,
      channel_url: curl,
    };

    if (openGithub) {
      const issueUrl = buildGithubIssueUrl(githubOwner, githubRepo, entry);
      setSubmittedIssueUrl(issueUrl);
      onSave(entry, true, editingEntry);
      return;
    }

    onSave(entry, false, editingEntry);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="streamer-modal-title"
    >
      <div className="w-full max-w-[440px] rounded-[10px] border-[1.5px] border-[#FF4655] bg-[#182430] p-5 text-[#ECE8E1] shadow-2xl">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2
              id="streamer-modal-title"
              className="text-base font-semibold text-white"
            >
              {editingEntry
                ? 'Edit Custom Streamer Entry'
                : 'Register Streamer to Directory'}
            </h2>
            <p className="mt-0.5 text-xs text-[#94A3B8]">
              Link a Valorant Riot ID to a verified stream channel
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded p-1 text-[#94A3B8] transition-colors hover:bg-[#21303F] hover:text-white"
            aria-label="Close modal"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {submittedIssueUrl ? (
          <div className="mt-4 space-y-4">
            <div className="rounded-md border border-[#22C55E]/40 bg-[#0F1923] p-3.5 text-xs text-[#ECE8E1]">
              <p className="font-semibold text-[#22C55E]">
                Saved locally to custom_streamers.json
              </p>
              <p className="mt-1.5 text-[#94A3B8]">
                In the native Windows app, your browser automatically opens the
                pre-filled GitHub Issue below for{' '}
                <span className="font-mono text-[#ECE8E1]">
                  {githubOwner}/{githubRepo}
                </span>
                .
              </p>
              <a
                href={submittedIssueUrl}
                target="_blank"
                rel="noopener noreferrer"
                className="mt-3 inline-flex items-center gap-1.5 rounded bg-[#FF4655] px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-[#e53a49]"
              >
                Open Pre-filled GitHub Issue
                <ExternalLink className="h-3.5 w-3.5" />
              </a>
            </div>
            <div className="flex justify-end">
              <button
                type="button"
                onClick={onClose}
                className="rounded-md border border-[#2B3A48] bg-[#21303F] px-4 py-2 text-xs font-medium text-[#ECE8E1] hover:bg-[#2a3c4e]"
              >
                Done
              </button>
            </div>
          </div>
        ) : (
          <div className="mt-4 space-y-3">
            <div>
              <label className="mb-1 block text-xs font-semibold text-[#ECE8E1]">
                Riot ID (IGN#TAG)
              </label>
              <input
                type="text"
                value={riotId}
                onChange={(e) => {
                  setRiotId(e.target.value);
                  setError(null);
                }}
                placeholder="e.g. RohanYT#IND"
                className="w-full rounded border border-[#2B3A48] bg-[#0F1923] px-3 py-2 font-mono text-xs text-white placeholder-[#64748B] focus:border-[#FF4655] focus:outline-none"
              />
            </div>

            <div>
              <label className="mb-1 block text-xs font-semibold text-[#ECE8E1]">
                Platform
              </label>
              <div className="flex items-center gap-1.5 rounded-md bg-[#0F1923] p-1">
                {PLATFORMS.map((plat) => (
                  <button
                    key={plat}
                    type="button"
                    onClick={() => {
                      setPlatform(plat);
                      setError(null);
                    }}
                    className={`flex-1 rounded px-3 py-1.5 text-xs font-medium transition-colors whitespace-nowrap ${
                      platform === plat
                        ? 'bg-[#21303F] text-white'
                        : 'text-[#94A3B8] hover:text-white'
                    }`}
                  >
                    {plat}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <label className="mb-1 block text-xs font-semibold text-[#ECE8E1]">
                Channel Display Name
              </label>
              <input
                type="text"
                value={channelName}
                onChange={(e) => {
                  setChannelName(e.target.value);
                  setError(null);
                }}
                placeholder="e.g. Rohan Gaming"
                className="w-full rounded border border-[#2B3A48] bg-[#0F1923] px-3 py-2 text-xs text-white placeholder-[#64748B] focus:border-[#FF4655] focus:outline-none"
              />
            </div>

            <div>
              <label className="mb-1 block text-xs font-semibold text-[#ECE8E1]">
                Channel URL
              </label>
              <input
                type="url"
                value={channelUrl}
                onChange={(e) => {
                  setChannelUrl(e.target.value);
                  setError(null);
                }}
                placeholder={
                  platform === 'YouTube'
                    ? 'https://www.youtube.com/@RohanGaming'
                    : platform === 'Twitch'
                    ? 'https://www.twitch.tv/vexfps'
                    : 'https://kick.com/kairoclutch'
                }
                className="w-full rounded border border-[#2B3A48] bg-[#0F1923] px-3 py-2 font-mono text-xs text-white placeholder-[#64748B] focus:border-[#FF4655] focus:outline-none"
              />
            </div>

            {error && (
              <p className="text-xs font-medium text-[#FF4655]" role="alert">
                {error}
              </p>
            )}

            <div className="flex flex-wrap items-center gap-2 pt-2">
              <button
                type="button"
                onClick={() => handleAction(true)}
                className="rounded-md bg-[#FF4655] px-4 py-2 text-xs font-semibold text-white transition-colors hover:bg-[#e53a49] whitespace-nowrap"
              >
                Save &amp; Submit to GitHub
              </button>
              <button
                type="button"
                onClick={() => handleAction(false)}
                className="rounded-md border border-[#2B3A48] bg-[#21303F] px-3.5 py-2 text-xs font-medium text-[#ECE8E1] transition-colors hover:bg-[#2a3c4e] whitespace-nowrap"
              >
                Save Local
              </button>
              <button
                type="button"
                onClick={onClose}
                className="ml-auto rounded-md px-3 py-2 text-xs font-medium text-[#94A3B8] transition-colors hover:text-white whitespace-nowrap"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
