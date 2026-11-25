export type Platform = 'YouTube' | 'Twitch' | 'Kick';

export type ApiRegionCode = 'ap' | 'na' | 'eu' | 'kr';

export type ServerRegionCode =
  | 'ap-mumbai'
  | 'ap-singapore'
  | 'ap-tokyo'
  | 'ap-manila'
  | 'ap-hongkong'
  | 'ap-sydney'
  | 'kr-seoul'
  | 'na-austin'
  | 'na-us-east'
  | 'na-us-west'
  | 'latam-mexico'
  | 'br-saopaulo'
  | 'eu-london'
  | 'eu-frankfurt'
  | 'eu-paris'
  | 'eu-stockholm'
  | 'eu-warsaw'
  | 'eu-madrid'
  | 'eu-istanbul'
  | 'eu-bahrain'
  | 'eu-capetown';

export type ServerRegionOverride = 'auto' | ServerRegionCode;

export type ConfidenceTier = 'Confirmed' | 'Probable' | 'Unverified';

export type DateRangeFilter = 'all' | 'today' | '7d' | '30d';

export type StatusFilter = 'all' | 'Confirmed' | 'Probable' | 'Unverified' | 'Chat';

export interface ServerRegionInfo {
  code: ServerRegionCode;
  apiRegion: ApiRegionCode;
  label: string;
  shortLabel: string;
  podExample: string;
}

export const COMMUNITY_REPO_OWNER = 'svg2709';
export const COMMUNITY_REPO_NAME = 'Valorant-Streamer-Radar';
export const APP_VERSION = '1.2.2';

export const SERVER_REGIONS: ServerRegionInfo[] = [
  {
    code: 'ap-mumbai',
    apiRegion: 'ap',
    label: 'AP / Mumbai (India)',
    shortLabel: 'Mumbai',
    podExample: 'aresriot.aws-ap-south-1-mumbai-1',
  },
  {
    code: 'ap-singapore',
    apiRegion: 'ap',
    label: 'AP / Singapore (SEA)',
    shortLabel: 'Singapore',
    podExample: 'aresriot.aws-ap-southeast-1-singapore-1',
  },
  {
    code: 'ap-tokyo',
    apiRegion: 'ap',
    label: 'AP / Tokyo (Japan)',
    shortLabel: 'Tokyo',
    podExample: 'aresriot.aws-ap-northeast-1-tokyo-1',
  },
  {
    code: 'ap-manila',
    apiRegion: 'ap',
    label: 'AP / Manila (Philippines)',
    shortLabel: 'Manila',
    podExample: 'aresriot.aws-ap-southeast-1-manila-1',
  },
  {
    code: 'ap-hongkong',
    apiRegion: 'ap',
    label: 'AP / Hong Kong',
    shortLabel: 'Hong Kong',
    podExample: 'aresriot.aws-ap-east-1-hongkong-1',
  },
  {
    code: 'ap-sydney',
    apiRegion: 'ap',
    label: 'AP / Sydney (OCE)',
    shortLabel: 'Sydney',
    podExample: 'aresriot.aws-ap-southeast-2-sydney-1',
  },
  {
    code: 'kr-seoul',
    apiRegion: 'kr',
    label: 'KR / Seoul (Korea)',
    shortLabel: 'Seoul',
    podExample: 'aresriot.aws-ap-northeast-2-seoul-1',
  },
  {
    code: 'na-austin',
    apiRegion: 'na',
    label: 'NA / Austin & Dallas (Texas)',
    shortLabel: 'Austin',
    podExample: 'aresriot.aws-us-central-1-dallas-austin-1',
  },
  {
    code: 'na-us-east',
    apiRegion: 'na',
    label: 'NA / US East (Virginia / Georgia / Illinois)',
    shortLabel: 'US East',
    podExample: 'aresriot.aws-us-east-1-ashburn-1',
  },
  {
    code: 'na-us-west',
    apiRegion: 'na',
    label: 'NA / US West (California / Oregon)',
    shortLabel: 'US West',
    podExample: 'aresriot.aws-us-west-2-oregon-1',
  },
  {
    code: 'latam-mexico',
    apiRegion: 'na',
    label: 'LATAM / Mexico, Bogota & Santiago',
    shortLabel: 'LATAM',
    podExample: 'aresriot.aws-latam-mexico-1',
  },
  {
    code: 'br-saopaulo',
    apiRegion: 'na',
    label: 'BR / Sao Paulo (Brazil)',
    shortLabel: 'Brazil',
    podExample: 'aresriot.aws-sa-east-1-saopaulo-1',
  },
  {
    code: 'eu-london',
    apiRegion: 'eu',
    label: 'EU / London (UK)',
    shortLabel: 'London',
    podExample: 'aresriot.aws-eu-west-2-london-1',
  },
  {
    code: 'eu-frankfurt',
    apiRegion: 'eu',
    label: 'EU / Frankfurt (Germany / Central)',
    shortLabel: 'Frankfurt',
    podExample: 'aresriot.aws-eu-central-1-frankfurt-1',
  },
  {
    code: 'eu-paris',
    apiRegion: 'eu',
    label: 'EU / Paris (France)',
    shortLabel: 'Paris',
    podExample: 'aresriot.aws-eu-west-3-paris-1',
  },
  {
    code: 'eu-stockholm',
    apiRegion: 'eu',
    label: 'EU / Stockholm (Nordics)',
    shortLabel: 'Stockholm',
    podExample: 'aresriot.aws-eu-north-1-stockholm-1',
  },
  {
    code: 'eu-warsaw',
    apiRegion: 'eu',
    label: 'EU / Warsaw (Poland)',
    shortLabel: 'Warsaw',
    podExample: 'aresriot.aws-eu-central-1-warsaw-1',
  },
  {
    code: 'eu-madrid',
    apiRegion: 'eu',
    label: 'EU / Madrid (Spain)',
    shortLabel: 'Madrid',
    podExample: 'aresriot.aws-eu-south-2-madrid-1',
  },
  {
    code: 'eu-istanbul',
    apiRegion: 'eu',
    label: 'EU / Istanbul (Turkey)',
    shortLabel: 'Istanbul',
    podExample: 'aresriot.aws-eu-istanbul-1',
  },
  {
    code: 'eu-bahrain',
    apiRegion: 'eu',
    label: 'EU / Bahrain (Middle East)',
    shortLabel: 'Bahrain',
    podExample: 'aresriot.aws-me-south-1-bahrain-1',
  },
  {
    code: 'eu-capetown',
    apiRegion: 'eu',
    label: 'EU / Cape Town (South Africa)',
    shortLabel: 'Cape Town',
    podExample: 'aresriot.aws-af-south-1-capetown-1',
  },
];

export function getServerRegionInfo(code: string): ServerRegionInfo {
  const norm = code.trim().toLowerCase();
  const exact = SERVER_REGIONS.find(
    (r) => r.code === norm || r.shortLabel.toLowerCase() === norm
  );
  if (exact) return exact;
  if (norm === 'japan') return SERVER_REGIONS.find((r) => r.code === 'ap-tokyo')!;
  if (norm === 'na' || norm === 'austin / tx') return SERVER_REGIONS.find((r) => r.code === 'na-austin')!;
  if (norm === 'eu') return SERVER_REGIONS.find((r) => r.code === 'eu-frankfurt')!;
  if (norm === 'kr') return SERVER_REGIONS.find((r) => r.code === 'kr-seoul')!;
  return SERVER_REGIONS[0];
}

export type TeamRelation = 'Ally' | 'Enemy' | '';

export interface AppConfig {
  youtube_api_key: string;
  twitch_client_id: string;
  twitch_client_secret: string;
  start_with_windows: boolean;
  server_override: ServerRegionOverride;
  use_custom_toast_app_id: boolean;
  capture_in_game_chat: boolean;
  enable_recommendations: boolean;
  skip_party_members: boolean;
}

export interface DirectoryStreamer {
  id?: string;
  display_name?: string;
  riot_id: string;
  aliases?: string[];
  platform: Platform;
  channel_id?: string;
  channel_name: string;
  channel_url: string;
  server_region?: string;
  verified?: boolean;
  submitted_at?: string;
  source?: string;
}

export interface LikelyStreamerCandidate {
  platform: Platform;
  channel: string;
  title: string;
  url: string;
  confidence: number;
  confidence_tier?: ConfidenceTier;
  server_match_status?: string;
  region_hint?: string;
  match_reason: string;
}

export interface StreamerEntry {
  match_id: string;
  riot_id: string;
  platform: Platform;
  channel: string;
  title: string;
  url: string;
  timestamp: string;
  detected_at_iso?: string;
  team_relation: TeamRelation;
  map_name: string;
  agent_name: string;
  queue_name: string;
  chat_message?: string;
  source?: 'Directory' | 'Heuristic' | 'Chat';
  confidence?: number;
  confidence_tier?: ConfidenceTier;
  server_match_status?: string;
  match_reason?: string;
  server_region?: string;
  candidates?: LikelyStreamerCandidate[];
}

export interface RecommendedStreamer {
  id: string;
  platform: 'Twitch' | 'YouTube';
  channel: string;
  title: string;
  url: string;
  viewers: number;
  region_label: string;
  server_region: ServerRegionCode;
}

export interface LastMatchSummary {
  match_id: string;
  queue_name: string;
  map_name: string;
  my_agent?: string;
  players_checked: number;
  party_skipped: number;
  streamers_found: number;
  scanned_at: string;
  server_name: string;
}

export type DesktopTab = 'history' | 'directory' | 'discover' | 'settings';
export type StudioView = 'simulator' | 'lab';

export interface SimChatLine {
  riot_id: string;
  body: string;
}

export interface SimMatchPlayer {
  riot_id: string;
  team_relation: 'Ally' | 'Enemy';
  agent_name: string;
  isPartyMember?: boolean;
  detectionSource: 'directory' | 'heuristic' | 'chat' | 'none';
  rejectedCrossRegion?: string;
  liveStream?: {
    platform: Platform;
    channel: string;
    title: string;
    url: string;
    confidence: number;
    confidenceTier: ConfidenceTier;
    serverMatchStatus: string;
    match_reason: string;
    candidates?: LikelyStreamerCandidate[];
  };
}

export interface SimMatchScenario {
  id: string;
  label: string;
  serverRegion: ServerRegionCode;
  map_name: string;
  queue_name: string;
  match_id: string;
  players: SimMatchPlayer[];
  chatLog?: SimChatLine[];
}

export interface LogLineEntry {
  id: string;
  time: string;
  category: 'log' | 'server' | 'riot' | 'chat' | 'scan' | 'hit';
  message: string;
}

export const DEFAULT_CONFIG: AppConfig = {
  youtube_api_key: '',
  twitch_client_id: '',
  twitch_client_secret: '',
  start_with_windows: true,
  server_override: 'auto',
  use_custom_toast_app_id: true,
  capture_in_game_chat: true,
  enable_recommendations: false,
  skip_party_members: true,
};
