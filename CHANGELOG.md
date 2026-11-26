# Changelog

All notable changes to **Valorant Streamer Radar** are documented in this file.
GitHub Releases automatically pull the matching version notes from this file.

## [1.2.11] - 2026-11-26

### Added
- **Pure-Rust Embedded ML + Fuzzy/Phonetic Detection Engine (100% Keyless & Offline):** Replaced rigid exact/substring heuristics with a calibrated logistic regression classifier (`sigmoid`) combining Jaro-Winkler similarity, character bigram/trigram Sørensen–Dice overlap, gaming phonetic encoding (`KrixVal` ↔ `KryxVal`), leet-speak unfolding (`z3r0_fps` ↔ `ZeroFPS Live`), repeated-character collapsing (`kairooo` ↔ `kairo`), channel affix stripping (`KairoFPS Official` ↔ `KairoFPS`), and token-order overlap — adding zero external crates, zero API keys, and 0 MB GPU VRAM overhead while keeping all heuristics strictly post-match and capped at `Probable`/`Unverified`.
- **Non-Gameplay Broadcast & False-Positive Classifier:** Automatically detects and suppresses VCT watch parties, co-streams, 24/7 reruns, rebroadcasts, clip/highlight compilations, and non-Valorant stream titles while keeping leet-disguised generic IGNs (e.g., `Sh4d0w`) blocked.
- **Post-Match Context Correlation:** Feeds the finished match's `agent_name`, `map_name`, `queue_name`, and player Riot tag (`#TTV`, `#YT`, `#KICK`, `#LIVE`) into the ML scorer to boost confidence when live stream metadata aligns with the match you just finished.
- **Leet-Speak Candidate Expansion for Twitch & Kick:** Automatically includes leet-unfolded and repeat-collapsed login variants inside Twitch's single batched lookup (and Kick's fallback slug check) so stylized IGNs are actually discovered live.
- **Weighted Post-Match Chat Intent Classifier:** Recognizes casual and Hinglish self-plugs (`"yt same name"`, `"yt pe live hu"`, `"live rn on twitch"`) while rejecting negations, questions directed at others (`"are u live?"`, `"im not live"`), and stream-sniping remarks.

## [1.2.10] - 2026-11-26

### Fixed
- **Full Competitive Lobby Player Resolution & Hidden Name Transparency:** Replaced rigid `name-service/v2/players` deserialization with a null-tolerant parser (supporting `PascalCase`, `camelCase`, and `DisplayName` fallbacks), added automatic retries + a targeted second pass for unresolved PUUIDs, recovered names/stream plugs from post-match chat by PUUID, checked party exclusion before blank/invisible-character names (e.g., Hangul Filler `U+3164`), and surfaced any remaining hidden/Streamer Mode players transparently in the `LATEST MATCH` summary and status bar (`X scanned, Y party skipped, Z hidden`).
- **Auto-Shard Discovery for Non-AP Regions (NA / EU / KR / LATAM / BR):** When **Active Server** is set to **Auto** and the initial default shard has no match history on first boot, the post-match scanner now automatically discovers your active Riot shard instead of failing with "No recent match found on ap server region", and extends post-match propagation polling to better handle slower Competitive match writes.
- **Long-Session Log Tail & Mid-Match Launch Detection:** Upgraded startup `ShooterGame.log` inspection to read both the head (64 KB) and tail (512 KB) instead of only the first 256 KB—fixing stale server-pod detection in long sessions and ensuring post-match auto-scans still trigger when the app is launched mid-match while Valorant is already running.
- **YouTube EU/UK GDPR Consent Bypass & Live-Only Filtering:** Added the `SOCS=CAI` cookie on keyless YouTube requests so European/UK users are no longer blocked by consent redirects, verified live badges/canonical `/watch?v=` URLs to reject upcoming premieres, ended VODs, and offline sidebar recommendations, and allowed exact-handle keyless YouTube Valorant streams with unknown server regions to surface cleanly as `Unverified` instead of being silently dropped.
- **Faster Batched Twitch Detection & Leading IGN Tag Stripping:** Updated IGN cleaning to strip leading platform prefixes (`TTV `, `YT_`, `twitch.tv/`, `Kick-`) in addition to trailing suffixes, and batched Twitch candidate checks (`base`, raw login, `tv`, `_tv`, `ttv`) into a single Helix or aliased GraphQL HTTP request per player.
- **Cross-Platform Directory Fallback, Resilient Chat Parsing & Offline Version Fallback:** Players registered in the streamer directory who are offline on their listed platform now still get checked on other platforms; `/chat/v6/messages` parsing now tolerates `null` fields, excludes `ares-parties`/`ares-lobby` chat from leaking into match scans, and preserves `ares-pregame` stream plugs; `client_version` now falls back to `CI server version` in `ShooterGame.log` if `valorant-api.com` is temporarily unreachable.

### Changed
- **Dependency Updates & Clean Compiler Build:** Updated Rust crates (`reqwest 0.13` with native Windows TLS, `tray-icon 0.27`, `sha2 0.11`, `base64 0.23`, `tauri-winrt-notification 0.8`, `winreg 0.56`), synced GitHub Actions workflows to `actions/checkout@v6`, and explicitly typed all `Stroke::new` width literals as `f32` to eliminate Rust's future-compatibility warnings (`issue #154024`).

## [1.2.9] - 2026-11-25

### Fixed
- **Immediate Discover Tab Toggle in Settings:** Turning **Live Streamer Recommendations** ON or OFF in **Settings** now saves and applies immediately instead of waiting for **Save Settings**, so the **Discover** sidebar tab appears or hides right away.
- **Restored Top Pink-to-Pearl Gradient Bar:** Painted the curved top accent hairline on the foreground layer directly inside the 1px rounded window border (and removed the titlebar's redundant top border) so it stays crisp and clearly visible on Windows 10/11 curved windows.
- **Unified AMOLED Dark Theme Across Controls:** Fixed washed-out gray `ComboBox` dropdowns (such as **Active Server**), replaced bright neon-pink `selectable_value` blocks in **Directory**, **Filters**, **Discover**, and the **Add Streamer** modal with matching dark rounded segmented tab switchers, rounded all text inputs (`6px` corners with `#08080B` fill), added proper spacing between the History search box and **Filters** button, and updated API key sub-labels and guide headings to use embedded Plus Jakarta Sans weights.

## [1.2.8] - 2026-11-25

### Changed
- **Curved Window Corners:** Applied native Windows 11 DWM rounded window corners (`DWMWA_WINDOW_CORNER_PREFERENCE` + `#1F1F26` rounded border) with an 8px-radius Windows 10 GDI fallback so the frameless AMOLED desktop window curves smoothly like the preview.
- **Exact Preview Fonts & True Font Weights:** Embedded **Plus Jakarta Sans** (Regular, Medium, SemiBold, Bold), **Barlow Condensed** (Bold for the `VALORANT RADAR` brand lockup), and **JetBrains Mono** (Regular & SemiBold for monospace stats, scores, and metadata) directly into the Windows executable, replacing egui's single-weight default font so headings, the **LATEST MATCH** summary strip, detection cards, sidebar items, and buttons match the exact typography and boldness of the preview.

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
