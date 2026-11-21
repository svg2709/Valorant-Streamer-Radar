export type Platform = 'YouTube' | 'Twitch' | 'Kick';

export type ShardCode = 'ap' | 'na' | 'eu' | 'kr';
export type ShardOverride = 'auto' | ShardCode;

export type TeamRelation = 'Ally' | 'Enemy' | '';

export interface AppConfig {
  youtube_api_key: string;
  twitch_client_id: string;
  twitch_client_secret: string;
  start_with_windows: boolean;
  shard_override: ShardOverride;
  github_owner: string;
  github_repo: string;
  use_custom_toast_app_id: boolean;
}

export interface DirectoryStreamer {
  riot_id: string;
  platform: Platform;
  channel_name: string;
  channel_url: string;
}

export interface StreamerEntry {
  match_id: string;
  riot_id: string;
  platform: Platform;
  channel: string;
  title: string;
  url: string;
  timestamp: string;
  team_relation: TeamRelation;
  map_name: string;
  agent_name: string;
  queue_name: string;
}

export type DesktopTab = 'history' | 'directory' | 'settings';
export type StudioView = 'simulator' | 'lab';

export interface SimMatchPlayer {
  riot_id: string;
  team_relation: 'Ally' | 'Enemy';
  agent_name: string;
  detectionSource: 'directory' | 'heuristic' | 'none';
  liveStream?: {
    platform: Platform;
    channel: string;
    title: string;
    url: string;
  };
}

export interface SimMatchScenario {
  id: string;
  label: string;
  shard: ShardCode;
  map_name: string;
  queue_name: string;
  match_id: string;
  players: SimMatchPlayer[];
}

export interface LogLineEntry {
  id: string;
  time: string;
  category: 'log' | 'shard' | 'riot' | 'scan' | 'hit';
  message: string;
}

export const SHARD_LABELS: Record<ShardCode, string> = {
  ap: 'AP / Mumbai',
  na: 'NA / Americas',
  eu: 'EU / Europe',
  kr: 'KR / Korea',
};

export const DEFAULT_CONFIG: AppConfig = {
  youtube_api_key: '',
  twitch_client_id: '',
  twitch_client_secret: '',
  start_with_windows: true,
  shard_override: 'auto',
  github_owner: 'your-github-username',
  github_repo: 'valorant-streamers-db',
  use_custom_toast_app_id: true,
};
