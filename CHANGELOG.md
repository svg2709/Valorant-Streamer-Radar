# Changelog

All notable changes to **Valorant Streamer Radar** are documented in this file.
GitHub Releases automatically pull the matching version notes from this file.

## [1.2.7] - 2026-11-25

### Changed
- **Auto-Hide Discover Tab When Recommendations Are Off:** The **Discover** sidebar tab is now hidden whenever **Live Streamer Recommendations** is turned off in **Settings** (its default state), and automatically appears as soon as recommendations are enabled. Turning recommendations off also cleanly returns to the **History** tab.

## [1.2.6] - 2026-11-25

### Changed
- **Default-On Custom Windows Toast App ID:** Hid the **Custom Windows Toast App ID** toggle from Settings and enforced it as always enabled by default so Windows desktop notifications always show `Valorant Streamer Radar` as the sender automatically.

## [1.2.5] - 2026-11-25

### Added
- **Toggleable Boot Update Check (GUI-Only, ON by Default):** Added a **Check for Updates on Boot** ON/OFF toggle inside **Settings -> App Updates & Version** (`check_updates_on_boot`, enabled by default). When enabled, the app silently checks GitHub Releases at startup without sending desktop popups or opening the window, displaying the update banner strictly when you open the GUI; when turned off, no startup update requests are made.

## [1.2.4] - 2026-11-25

### Fixed
- **Permanently Eliminated NVIDIA G-SYNC & Top-Right Overlay (`G-SYNC` / `NORMAL`):** Removed DirectX/DXGI (`wgpu`) and OpenGL (`glow`) swapchains completely and switched the Windows UI to a pure CPU software rasterizer (`egui_software_backend`) presenting via Win32 GDI (`softbuffer` `CreateDIBSection` + `BitBlt`). Because the executable no longer creates any 3D/DXGI/OpenGL swapchain, NVIDIA's driver cannot hook the window, activate G-SYNC, or draw the top-right `G-SYNC`/`NORMAL` indicator — while also reducing binary size and using zero GPU VRAM.

## [1.2.3] - 2026-11-25

### Fixed
- **Complete Post-Match Player Scanning:** Fixed match scanning leaving players out by reading the full roster directly from Riot's `match-details` payload, treating `name-service/v2/players` as best-effort enrichment (never dropping players on partial or failed responses), matching UUIDs case-insensitively, and hardening party exclusion to ignore nil/shared dummy `partyId` values, enemy players, and free-for-all modes.
- **Disabled NVIDIA G-SYNC Activation (`NORMAL` Overlay):** Switched the Windows desktop renderer from hardware OpenGL (`glow`) to DX12 software WARP (`Microsoft Basic Render Driver`) via `eframe` + `wgpu` fallback adapter selection so NVIDIA's display driver no longer triggers G-SYNC or displays the top-right `NORMAL` indicator on the app window.
- **Cleared Seeded Sample Streamers:** Emptied `streamers.json` and added automatic startup/sync cleanup to purge legacy seeded sample streamer IDs from local `directory_cache.json`.
- **Consistent "Get Twitch Keys" Button Styling:** Updated the Twitch API key button in Settings to match the AMOLED Black & Pink-to-Pearl secondary button theme used across the app.

### Changed
- **Desktop-to-Preview AMOLED Black Parity:** Unified the native Windows sidebar navigation (active `#121216` fill, 2px `#FF5CA8` left indicator, vector tab icons), surface hierarchy, and button styling to mirror the AMOLED Black preview.
- **Minimal Collapsed Cards & Click-Anywhere Details:** Simplified collapsed detection cards to show only Riot ID, Platform, Channel, and Confidence tier; moved stream title, team, agent, map, queue, server, timestamp, chat quotes, and ranked candidates into the expandable Details drawer, which now opens by clicking anywhere on the card outside action buttons.
- **Icon-Only Copy Button:** Replaced the text `Copy` button on detection cards with a compact square copy-icon button with a hover tooltip.
- **Unified Executable, Tray & In-App Icon:** Rebuilt the multi-resolution Windows `.exe` icon (`16px`–`256px` with anti-aliased transparent rounded corners), runtime window/tray/toast icons, and in-app emblem from a single rounded-square AMOLED Black + Pink-to-Pearl `V` design.

## [1.2.2] - 2026-11-25

### Added
- **Discover Server Selector:** Added a dedicated server picker directly in the **Discover** tab so users can browse live Valorant streams for any supported server without changing their in-game detection server in Settings.
- **Improved API Key Setup Guide:** Expanded the collapsible **Settings** guide with detailed step-by-step instructions, common pitfalls, and local storage notes for both YouTube Data API v3 and Twitch Helix API.

### Fixed
- **Accurate "Valorant Active" Detection:** Switched Windows status checks to inspect active processes (`VALORANT-Win64-Shipping.exe` vs. `RiotClientServices.exe`), eliminating false "Valorant Active" reports when Riot Client is idling in the tray, another Riot game is running, or a stale lockfile remains.
- **Centered Top-Left App Logo:** Centered the brand emblem inside its frame and aligned it vertically within the custom title bar.

## [1.2.1] - 2026-11-25

### Changed
- **Custom Frameless Windows Title Bar:** Replaced the native Windows title bar with a thin, draggable AMOLED Black custom title bar featuring Minimize and Close-to-tray controls only, plus edge/corner window resizing.
- **Decluttered AMOLED Black UI & Left Sidebar Navigation:** Switched the theme to True AMOLED Black (`#000000`) with a dedicated left sidebar navigation rail (`History`, `Directory`, `Discover`, `Settings`) and anchored quick actions (`Check Last Match`, `+ Add Streamer`, live Riot/Server status).
- **Collapsible History Filters & Streamlined Cards:** Moved History filter controls into a collapsible `Filters` popover with active count badge, simplified detection cards with unboxed inline metadata and an expandable `Details` drawer, and kept `Latest Match` cleanly separated from `Previous History`.
- **Larger Typography & Expanded Window Size:** Increased font sizes across the interface for sharper readability and expanded the default desktop window to `1120x740` (minimum `860x560`) so all controls fit comfortably.

### Fixed
- **Map Name & Build Reliability:** Corrected the Valorant map name from `Plummet` to `Summit` and resolved Rust Windows build compilation issues.

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
