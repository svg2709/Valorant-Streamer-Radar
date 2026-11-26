import React, { useEffect, useState } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  ExternalLink,
  ShieldCheck,
  X,
  XCircle,
} from 'lucide-react';
import {
  DirectoryStreamer,
  Platform,
  RecommendedStreamer,
  SERVER_REGIONS,
  ServerRegionCode,
} from '../types/radar';
import {
  buildGithubIssueUrl,
  hostAllowed,
  hostOf,
  parseAliasesCsv,
  slugifyStreamerId,
  validateSubmission,
} from '../utils/validation';

interface StreamerModalProps {
  isOpen: boolean;
  editingEntry: DirectoryStreamer | null;
  prefillRecommendation?: RecommendedStreamer | null;
  defaultServerRegion: ServerRegionCode;
  onClose: () => void;
  onSave: (
    entry: DirectoryStreamer,
    openGithub: boolean,
    original: DirectoryStreamer | null
  ) => void;
}

type VerifyResult =
  | { kind: 'ok'; msg: string }
  | { kind: 'warn'; msg: string; suggestedName?: string; suggestedPlatform?: Platform }
  | { kind: 'err'; msg: string; suggestedPlatform?: Platform };

const PLATFORMS: Platform[] = ['YouTube', 'Twitch', 'Kick'];

function extractHandleFromUrl(url: string): string {
  const cleaned = url.split(/[?#]/)[0].replace(/\/+$/, '');
  const seg = cleaned.split('/').pop() || '';
  return seg.replace(/^@/, '');
}

function normalizeAlnum(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]/g, '');
}

export const StreamerModal: React.FC<StreamerModalProps> = ({
  isOpen,
  editingEntry,
  prefillRecommendation,
  defaultServerRegion,
  onClose,
  onSave,
}) => {
  const [riotId, setRiotId] = useState('');
  const [aliasesInput, setAliasesInput] = useState('');
  const [platform, setPlatform] = useState<Platform>('YouTube');
  const [channelName, setChannelName] = useState('');
  const [channelUrl, setChannelUrl] = useState('');
  const [serverRegion, setServerRegion] = useState<string>('');
  const [error, setError] = useState<string | null>(null);
  const [verifyResult, setVerifyResult] = useState<VerifyResult | null>(null);
  const [submittedIssueUrl, setSubmittedIssueUrl] = useState<string | null>(null);

  useEffect(() => {
    if (isOpen) {
      if (editingEntry) {
        setRiotId(editingEntry.riot_id);
        setAliasesInput((editingEntry.aliases ?? []).join(', '));
        setPlatform(editingEntry.platform);
        setChannelName(editingEntry.channel_name);
        setChannelUrl(editingEntry.channel_url);
        setServerRegion(editingEntry.server_region ?? '');
      } else if (prefillRecommendation) {
        setRiotId('');
        setAliasesInput('');
        setPlatform(prefillRecommendation.platform);
        setChannelName(prefillRecommendation.channel);
        setChannelUrl(prefillRecommendation.url);
        setServerRegion(prefillRecommendation.server_region);
      } else {
        setRiotId('');
        setAliasesInput('');
        setPlatform('YouTube');
        setChannelName('');
        setChannelUrl('');
        setServerRegion(defaultServerRegion);
      }
      setError(null);
      setVerifyResult(null);
      setSubmittedIssueUrl(null);
    }
  }, [isOpen, editingEntry, prefillRecommendation, defaultServerRegion]);

  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const trimmedUrl = channelUrl.trim();
  const parsedHost = hostOf(trimmedUrl);
  const canOpenUrl = Boolean(parsedHost && hostAllowed(platform, parsedHost));

  const handleVerify = () => {
    const rid = riotId.trim();
    const cname = channelName.trim();
    const curl = channelUrl.trim();
    setError(null);

    const hashIdx = rid.indexOf('#');
    if (hashIdx === -1) {
      setVerifyResult({
        kind: 'err',
        msg: 'Riot ID must include "#" (example: RohanYT#IND).',
      });
      return;
    }
    const namePart = rid.slice(0, hashIdx).trim();
    const tagPart = rid.slice(hashIdx + 1);
    if (
      namePart.length < 3 ||
      namePart.length > 16 ||
      tagPart.length < 3 ||
      tagPart.length > 5 ||
      !/^[a-zA-Z0-9]+$/.test(tagPart)
    ) {
      setVerifyResult({
        kind: 'err',
        msg: 'Riot ID looks invalid (name 3-16 chars, tag 3-5 letters/digits).',
      });
      return;
    }

    const host = hostOf(curl);
    if (!host) {
      setVerifyResult({
        kind: 'err',
        msg: 'Channel URL must start with https:// and be a valid link.',
      });
      return;
    }
    if (!hostAllowed(platform, host)) {
      const matched = PLATFORMS.find((p) => p !== platform && hostAllowed(p, host));
      if (matched) {
        setVerifyResult({
          kind: 'err',
          msg: `URL belongs to ${matched}, not ${platform}. Switch Platform to ${matched}.`,
          suggestedPlatform: matched,
        });
      } else {
        setVerifyResult({
          kind: 'err',
          msg: `URL host "${host}" is not an allowed ${platform} domain.`,
        });
      }
      return;
    }

    const handle = extractHandleFromUrl(curl);
    if (!handle || handle.length < 3 || handle.includes('.')) {
      setVerifyResult({
        kind: 'err',
        msg: `Channel URL is missing a valid ${platform} channel handle/slug at the end.`,
      });
      return;
    }

    if (!cname) {
      setVerifyResult({
        kind: 'warn',
        msg: `Link format verified for ${platform} handle "${handle}". Click below to fill Channel Display Name.`,
        suggestedName: handle,
      });
      return;
    }

    const normEntered = normalizeAlnum(cname);
    const normHandle = normalizeAlnum(handle);
    const roughMatch =
      normEntered === normHandle ||
      normEntered.includes(normHandle) ||
      normHandle.includes(normEntered);

    if (roughMatch) {
      setVerifyResult({
        kind: 'ok',
        msg: `Verified! Riot ID format, ${platform} link, and channel name "${cname}" align.`,
      });
    } else {
      setVerifyResult({
        kind: 'warn',
        msg: `Riot ID & ${platform} URL are valid, but URL handle is "${handle}" while display name is "${cname}".`,
        suggestedName: handle,
      });
    }
  };

  const handleAction = (openGithub: boolean) => {
    const rid = riotId.trim();
    const cname = channelName.trim();
    const curl = channelUrl.trim();

    const validationError = validateSubmission(rid, platform, cname, curl);
    if (validationError) {
      setError(validationError);
      return;
    }

    const aliases = parseAliasesCsv(aliasesInput, rid);
    const entry: DirectoryStreamer = {
      id: editingEntry?.id || slugifyStreamerId(rid, platform),
      display_name: cname,
      riot_id: rid,
      aliases,
      platform,
      channel_id: extractHandleFromUrl(curl) || undefined,
      channel_name: cname,
      channel_url: curl,
      server_region: serverRegion || undefined,
      verified: editingEntry?.verified ?? false,
      submitted_at: editingEntry?.submitted_at ?? new Date().toISOString(),
      source: openGithub ? 'community-submission' : 'local-custom',
    };

    if (openGithub) {
      const issueUrl = buildGithubIssueUrl(entry);
      setSubmittedIssueUrl(issueUrl);
      onSave(entry, true, editingEntry);
      return;
    }

    onSave(entry, false, editingEntry);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="streamer-modal-title"
    >
      <div className="w-full max-w-[490px] overflow-hidden rounded-lg border border-[#FF5CA8]/80 bg-[#0B0B0E] text-[#F4F4F6] shadow-2xl">
        <div className="h-[2px] w-full bg-pink-white-strip" />
        <div className="max-h-[90vh] overflow-y-auto p-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2
                id="streamer-modal-title"
                className="text-base font-semibold text-[#FFF5FA]"
              >
                {editingEntry
                  ? 'Edit Custom Streamer Entry'
                  : 'Submit Streamer to Community Database'}
              </h2>
              <p className="mt-0.5 text-xs text-[#8E95A5]">
                Submissions are reviewed before becoming verified in the shared streamer database
              </p>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="rounded p-1 text-[#8E95A5] transition-colors hover:bg-[#1A1A20] hover:text-white"
              aria-label="Close modal"
            >
              <X className="h-4 w-4" />
            </button>
          </div>

          {submittedIssueUrl ? (
            <div className="mt-4 space-y-4">
              <div className="rounded-md border border-[#22C55E]/40 bg-[#000000] p-3.5 text-xs text-[#F4F4F6]">
                <p className="font-semibold text-[#22C55E]">
                  Saved locally &amp; ready for moderated review
                </p>
                <p className="mt-1.5 text-[#8E95A5]">
                  In the Windows app, your browser opens the pre-filled submission form automatically.
                  User submissions are never treated as verified until a maintainer reviews and approves them.
                </p>
                <a
                  href={submittedIssueUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="mt-3 inline-flex items-center gap-1.5 rounded bg-pink-white-gradient bg-pink-white-gradient-hover px-3 py-1.5 text-xs font-bold text-[#000000]"
                >
                  Open Submission Form
                  <ExternalLink className="h-3.5 w-3.5" />
                </a>
              </div>
              <div className="flex justify-end">
                <button
                  type="button"
                  onClick={onClose}
                  className="rounded-md border border-[#1F1F26] bg-[#121216] px-4 py-2 text-xs font-medium text-[#F4F4F6] hover:bg-[#1A1A20]"
                >
                  Done
                </button>
              </div>
            </div>
          ) : (
            <div className="mt-4 space-y-3">
              <div>
                <label className="mb-1 block text-xs font-semibold text-[#F4F4F6]">
                  Primary Riot ID (IGN#TAG)
                </label>
                <input
                  type="text"
                  value={riotId}
                  onChange={(e) => {
                    setRiotId(e.target.value);
                    setError(null);
                    setVerifyResult(null);
                  }}
                  placeholder="e.g. RohanYT#IND"
                  className="w-full rounded border border-[#1F1F26] bg-[#000000] px-3 py-2 font-mono text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
                />
              </div>

              <div>
                <label className="mb-1 block text-xs font-semibold text-[#F4F4F6]">
                  Known Aliases / Alt Riot IDs{' '}
                  <span className="font-normal text-[#8E95A5]">(Optional, comma-separated)</span>
                </label>
                <input
                  type="text"
                  value={aliasesInput}
                  onChange={(e) => setAliasesInput(e.target.value)}
                  placeholder="e.g. RohanAlt#MUM, RohanLive#IND"
                  className="w-full rounded border border-[#1F1F26] bg-[#000000] px-3 py-2 font-mono text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
                />
              </div>

              <div>
                <label className="mb-1 block text-xs font-semibold text-[#F4F4F6]">
                  Platform
                </label>
                <div className="flex items-center gap-1.5 rounded-md border border-[#1F1F26] bg-[#000000] p-1">
                  {PLATFORMS.map((plat) => (
                    <button
                      key={plat}
                      type="button"
                      onClick={() => {
                        setPlatform(plat);
                        setError(null);
                        setVerifyResult(null);
                      }}
                      className={`flex-1 rounded px-3 py-1.5 text-xs font-medium transition-colors whitespace-nowrap ${
                        platform === plat
                          ? 'border border-[#FF5CA8]/60 bg-[#121216] text-[#FFF5FA]'
                          : 'border border-transparent text-[#8E95A5] hover:text-white'
                      }`}
                    >
                      {plat}
                    </button>
                  ))}
                </div>
              </div>

              <div>
                <label className="mb-1 block text-xs font-semibold text-[#F4F4F6]">
                  Primary Valorant Server{' '}
                  <span className="font-normal text-[#8E95A5]">
                    (Prevents cross-server false positives)
                  </span>
                </label>
                <select
                  value={serverRegion}
                  onChange={(e) => setServerRegion(e.target.value)}
                  className="w-full rounded border border-[#1F1F26] bg-[#000000] px-3 py-2 text-xs text-white focus:border-[#FF5CA8] focus:outline-none"
                >
                  <option value="">Unknown / Unverified Server</option>
                  {SERVER_REGIONS.map((reg) => (
                    <option key={reg.code} value={reg.code}>
                      {reg.label}
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label className="mb-1 block text-xs font-semibold text-[#F4F4F6]">
                  Channel Display Name{' '}
                  <span className="font-normal text-[#8E95A5]">
                    (or leave blank &amp; click Verify)
                  </span>
                </label>
                <input
                  type="text"
                  value={channelName}
                  onChange={(e) => {
                    setChannelName(e.target.value);
                    setError(null);
                    setVerifyResult(null);
                  }}
                  placeholder="e.g. Rohan Gaming"
                  className="w-full rounded border border-[#1F1F26] bg-[#000000] px-3 py-2 text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
                />
              </div>

              <div>
                <label className="mb-1 block text-xs font-semibold text-[#F4F4F6]">
                  Channel URL
                </label>
                <input
                  type="url"
                  value={channelUrl}
                  onChange={(e) => {
                    setChannelUrl(e.target.value);
                    setError(null);
                    setVerifyResult(null);
                  }}
                  placeholder={
                    platform === 'YouTube'
                      ? 'https://www.youtube.com/@RohanGaming'
                      : platform === 'Twitch'
                      ? 'https://www.twitch.tv/vexfps'
                      : 'https://kick.com/kairoclutch'
                  }
                  className="w-full rounded border border-[#1F1F26] bg-[#000000] px-3 py-2 font-mono text-xs text-white placeholder-[#5A6275] focus:border-[#FF5CA8] focus:outline-none"
                />
              </div>

              <div className="flex flex-wrap items-center gap-2 pt-0.5">
                <button
                  type="button"
                  onClick={handleVerify}
                  className="inline-flex items-center gap-1.5 rounded-md border border-[#FFA6D2]/50 bg-[#121216] px-3 py-1.5 text-xs font-semibold text-[#FFF5FA] transition-colors hover:border-[#FF5CA8] hover:bg-[#1A1A20] whitespace-nowrap"
                >
                  <ShieldCheck className="h-3.5 w-3.5 text-[#FFA6D2]" />
                  Verify Link &amp; Name
                </button>
                {canOpenUrl ? (
                  <a
                    href={trimmedUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1 rounded-md border border-[#1F1F26] bg-[#121216] px-2.5 py-1.5 text-xs font-medium text-[#F4F4F6] transition-colors hover:bg-[#1A1A20] whitespace-nowrap"
                  >
                    Open Link
                    <ExternalLink className="h-3 w-3" />
                  </a>
                ) : (
                  <span className="inline-flex cursor-not-allowed items-center gap-1 rounded-md border border-[#1F1F26]/60 bg-[#000000] px-2.5 py-1.5 text-xs text-[#5A6275] whitespace-nowrap">
                    Open Link
                    <ExternalLink className="h-3 w-3" />
                  </span>
                )}
              </div>

              {verifyResult && (
                <div
                  className={`rounded-md border px-3 py-2 text-xs ${
                    verifyResult.kind === 'ok'
                      ? 'border-[#22C55E]/40 bg-[#22C55E]/10 text-[#22C55E]'
                      : verifyResult.kind === 'warn'
                      ? 'border-[#FB923C]/40 bg-[#FB923C]/10 text-[#FB923C]'
                      : 'border-[#FF668A]/40 bg-[#FF668A]/10 text-[#FF668A]'
                  }`}
                >
                  <div className="flex items-start gap-2">
                    {verifyResult.kind === 'ok' ? (
                      <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    ) : verifyResult.kind === 'warn' ? (
                      <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    ) : (
                      <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    )}
                    <div className="space-y-1.5">
                      <p>{verifyResult.msg}</p>
                      {verifyResult.kind !== 'ok' && verifyResult.suggestedPlatform && (
                        <button
                          type="button"
                          onClick={() => {
                            setPlatform(verifyResult.suggestedPlatform!);
                            setVerifyResult(null);
                          }}
                          className="rounded border border-[#FF5CA8] bg-[#121216] px-2 py-0.5 text-[11px] font-semibold text-[#FFF5FA] hover:bg-[#1A1A20]"
                        >
                          Switch Platform to {verifyResult.suggestedPlatform}
                        </button>
                      )}
                      {verifyResult.kind === 'warn' && verifyResult.suggestedName && (
                        <button
                          type="button"
                          onClick={() => {
                            setChannelName(verifyResult.suggestedName!);
                            setVerifyResult({
                              kind: 'ok',
                              msg: `Channel name updated to "${verifyResult.suggestedName}" and verified.`,
                            });
                          }}
                          className="rounded border border-[#FB923C] bg-[#121216] px-2 py-0.5 text-[11px] font-semibold text-white hover:bg-[#1A1A20]"
                        >
                          Use detected name: {verifyResult.suggestedName}
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              )}

              {error && (
                <p className="text-xs font-medium text-[#FF668A]" role="alert">
                  {error}
                </p>
              )}

              <div className="flex flex-wrap items-center gap-2 pt-2">
                <button
                  type="button"
                  onClick={() => handleAction(true)}
                  className="rounded-md bg-pink-white-gradient bg-pink-white-gradient-hover px-4 py-2 text-xs font-bold text-[#000000] whitespace-nowrap"
                >
                  Save &amp; Submit for Review
                </button>
                <button
                  type="button"
                  onClick={() => handleAction(false)}
                  className="rounded-md border border-[#1F1F26] bg-[#121216] px-3.5 py-2 text-xs font-medium text-[#F4F4F6] transition-colors hover:bg-[#1A1A20] whitespace-nowrap"
                >
                  Save Local
                </button>
                <button
                  type="button"
                  onClick={onClose}
                  className="ml-auto rounded-md px-3 py-2 text-xs font-medium text-[#8E95A5] transition-colors hover:text-white whitespace-nowrap"
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
