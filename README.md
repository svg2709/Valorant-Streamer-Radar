# Valorant Streamer Radar (`val-streamer-notifier` v0.5.0)

A lightweight, native Windows tray application written in pure Rust (`eframe/egui 0.29`, `tray-icon 0.19`, `tokio`, `tauri-winrt-notification`). It runs silently in the background (~5 MB idle RAM, 0% idle CPU when hidden) and, **strictly after a Valorant match ends**, checks whether any of the other 9 players were live streaming on **YouTube**, **Twitch**, or **Kick**.

## Safety & Compliance
- **100% post-match and user-space only.** Never reads game memory, injects DLLs, hooks processes, or inspects pre-game / live agent-select lobbies.
- Scans trigger only after `ShooterGame.log` records an `InGame -> MainMenu` transition (or when you click **Check Last Match**).
- Authenticates locally via Riot Client's `lockfile` and dynamically fetches `riotClientVersion` from `valorant-api.com`.
- All community directory URLs are treated as untrusted input and strictly host-validated (`youtube.com`, `youtu.be`, `twitch.tv`, `kick.com`, no userinfo/ports).

## What's New in v0.5.0
- **In-App Settings UI**: Manage optional API keys, GitHub directory repository, Riot shard, Windows autostart, and Toast AppID directly inside the app (saved atomically to `config.json`).
- **Directory Manager**: Browse, search, add, edit, and delete entries in `custom_streamers.json`, inspect the cached community directory, and trigger manual GitHub refreshes.
- **Multi-Region Shard Auto-Detection**: Automatically parses `pd.<shard>.a.pvp.net` from `ShooterGame.log` (`ap`, `na`, `eu`, `kr`) with manual override in Settings.
- **Rich Match Context**: Detection cards show **Ally / Enemy**, **Map**, **Agent**, and **Queue** (backwards-compatible with v0.4.0 `streamer_history.json` files).
- **Custom Windows Toast AppUserModelID**: Registers `HKCU\Software\Classes\AppUserModelId\ValorantStreamerRadar` with a procedurally generated `.ico` icon so Windows toasts display **Valorant Streamer Radar** instead of **Windows PowerShell**.

---

## Configuration (`%APPDATA%\ValorantStreamerRadar\config.json`)

The app creates `config.json` automatically on first run (and works completely keyless out of the box):

```json
{
  "youtube_api_key": "",
  "twitch_client_id": "",
  "twitch_client_secret": "",
  "start_with_windows": true,
  "shard_override": "auto",
  "github_owner": "your-github-username",
  "github_repo": "valorant-streamers-db",
  "use_custom_toast_app_id": true
}
```

| Field | Description |
| :--- | :--- |
| `youtube_api_key` | Optional YouTube Data API v3 key. Falls back to keyless YouTube live search scraping on empty/quota errors. |
| `twitch_client_id` / `twitch_client_secret` | Optional Twitch Helix OAuth2 credentials. Falls back to parameterized public Twitch GQL if omitted. |
| `start_with_windows` | Writes/removes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\ValorantStreamerRadar`. |
| `shard_override` | `"auto"` (detect from `ShooterGame.log`, fallback `"ap"`), or force `"ap"`, `"na"`, `"eu"`, `"kr"`. |
| `github_owner` / `github_repo` | GitHub repository hosting `main/streamers.json` for authoritative Riot ID -> channel lookups. |
| `use_custom_toast_app_id` | Registers `ValorantStreamerRadar` in `HKCU` for branded Windows Toasts (falls back to PowerShell ID on error). |

---

## Community Directory Schema (`streamers.json`)

Host a `streamers.json` file in the `main` branch of your configured GitHub repository:

```json
[
  {
    "riot_id": "Name#TAG",
    "platform": "YouTube",
    "channel_name": "Channel Display Name",
    "channel_url": "https://www.youtube.com/@ChannelHandle"
  }
]
```

- `platform`: `"YouTube"`, `"Twitch"`, or `"Kick"`
- `channel_url`: Must be a plain `https://` URL matching the platform host (`youtube.com`/`youtu.be`, `twitch.tv`, `kick.com`).

---

## Building on Windows

### Local Build (Rust 1.80+)
```powershell
cargo build --release
# Output: target\release\val-streamer-notifier.exe
```

### Automated Cloud Build (GitHub Actions)
Push this crate to GitHub—the included `.github/workflows/build-windows.yml` workflow automatically compiles `val-streamer-notifier.exe` on `windows-latest` and uploads it as a build artifact (or attaches it to a GitHub Release whenever you push a `v*` tag).

---

## Known Limitations
- **Kick / Cloudflare**: Kick sits behind Cloudflare and may return HTTP `403` periodically; those responses are treated as "not live".
- **Heuristic False Positives**: Keyless IGN heuristics can occasionally match unrelated channels—prefer adding known creators to the community `streamers.json` directory or your local `custom_streamers.json`.
- **First Scan After Launch**: On the very first post-match scan after launching the app, there is no prior `MatchID` in memory to compare against.
- **Custom Toast AppID Fallback**: If `use_custom_toast_app_id` is disabled or Windows Registry writes are blocked by policy, toasts fall back to the `"Windows PowerShell"` AppUserModelID.
- **Multi-Shard Coverage**: Supports `ap` (Mumbai / Asia-Pacific), `na` (NA / LATAM / BR), `eu` (Europe), and `kr` (Korea) via `ShooterGame.log` auto-detection or Settings override.
