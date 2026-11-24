# Changelog

All notable changes to **Valorant Streamer Radar** are documented in this file.
GitHub Releases automatically pull the matching version notes from this file.

## [1.2.0] - 2026-11-24

### Added
- **Automated Changelog-Backed Update Notifications:** Update checks compare the installed version against the latest published GitHub Release, display the version number and matching release changelog directly in the update banner, hide automatically when up to date (no manual dismissal required), prevent duplicate notifications, and handle network errors, missing releases (HTTP 404), and API rate limits without falsely reporting "up to date."
- **Confidence-Tiered, Server-Aware Streamer Detection:** Reworked detection into three explicit tiers — **Confirmed**, **Probable**, and **Unverified** — using multi-signal scoring (verified database Riot ID and alias match, platform account association, actual in-game server vs. streaming region vs. inferred location, live Valorant category, and match chat links). Cross-region conflicts (such as North American creators when playing on the Mumbai server) are rejected, and unknown regions are marked **Unverified** rather than triggering confirmed alerts or Windows toast popups.
- **Structured Community Streamer Database (`streamers.json`):** Upgraded the centralized database schema with unique IDs, display names, Riot IDs, multiple known aliases, platform channel identifiers, verified in-game server regions, and moderation metadata, backed by a moderated GitHub submission workflow (`add-streamer.yml` + `merge-approved-streamer.yml`) with duplicate prevention and zero embedded credentials.
- **Party Member Exclusion (API Optimization):** Reads each player's `partyId` directly from Riot's post-match payload (zero extra Riot API calls) and skips party members from external YouTube/Twitch/Kick lookups by default (toggleable in Settings). Automatically adapts when players join or leave your party across matches and displays skipped party counts in the Latest Match summary.
- **Optional Live Twitch & YouTube Recommendations (Discover Tab):** Added an opt-in regional recommendation subsystem — strictly **OFF by default** with zero background network requests while disabled, response caching (5-minute TTL) to respect quotas, clear bandwidth usage notice, and strict separation from confirmed match detections.
- **Latest Match Subsection & Multi-Criteria History Filters:** Separated the **Latest Match** into a dedicated subsection with a visual divider above **Previous History**, preserving chronological ordering and existing saved data. Added combinable filters for **Match Type**, **Server**, **Date Range** (All Time, Today, Last 7 Days, Last 30 Days), **Detection Status** (Confirmed, Probable, Unverified, Chat Link), **Team**, and free-text search, plus a one-click **Reset Filters** action and clear empty state.
- **Expanded Server Support (Manila, Austin, Tokyo & More):** Added verified Riot pod and routing mappings for **Manila**, **Austin**, **Tokyo**, and 18+ regional Valorant servers across detection, filtering, and streamer database matching.
- **Left-to-Right ON/OFF Toggle Switches:** Replaced boolean checkboxes/tick marks across Settings with accessible, animated left-to-right ON/OFF sliding switches while preserving existing configuration compatibility.
- **Honest README & Documentation:** Rewrote `README.md` to explain how the app works, explicitly state that **this is not a stream sniping tool**, prominently document detection limitations (not 100% accurate), list all supported servers, and document honest memory/CPU expectations and troubleshooting.
- **Automated Test Suite:** Added unit tests covering semver update checks, confidence tiering, Mumbai vs. NA region conflict rejection, alias matching, party member exclusion, server pod parsing (Manila, Austin, Tokyo), database validation/deduplication, and history filtering.

### Changed
- **Replaced "Shards" Terminology with "Servers":** Replaced user-facing "Shard" labels, configuration keys (`server_override` with backward-compatible `shard_override` alias), dropdowns, tooltips, and documentation with **Servers** terminology.
- **Hidden Repository Management Controls:** Removed user-facing GitHub repository owner/name inputs from Settings; the community streamer database now syncs automatically with the official repository.
- **Repository Cleanup:** Removed unused web dependencies (`@google/genai`, `express`, `dotenv`, `motion`, `autoprefixer`, `@types/express`, `tsx`) and cleaned dead code.

## [1.0.1] - 2026-06-15

### Fixed
- Restored system tray left-click/double-click and menu activation to reliably reopen the hidden window on Windows.
- Improved GitHub Release publishing workflow for tagged Windows x64 builds and SHA-256 checksum verification.

## [1.0.0] - 2026-06-01

### Added
- Initial native Windows Rust release with post-match live streamer detection across YouTube, Twitch, and Kick.
- In-game text chat stream plug capture (buffered silently during matches, processed strictly post-match).
- Native Windows toast notifications, custom local streamer directory, and optional YouTube/Twitch API key support.
