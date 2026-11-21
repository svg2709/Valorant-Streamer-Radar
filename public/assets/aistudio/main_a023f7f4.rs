#![windows_subsystem = "windows"]

use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Local;
use eframe::egui::{self, Color32, FontId, Frame, Margin, RichText, Rounding, Stroke, Vec2};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

// ============================================================================
// CONFIGURATION & THEME CONSTANTS
// ============================================================================

const GITHUB_OWNER: &str = "svg2709";
const GITHUB_REPO: &str = "valorant-streamers-db";

const SINGLE_INSTANCE_PORT: u16 = 47831;
const MAX_HISTORY: usize = 300;
const TOAST_TTL: Duration = Duration::from_secs(4);

const BG_DARK: Color32 = Color32::from_rgb(15, 25, 35);
const HEADER_BG: Color32 = Color32::from_rgb(11, 19, 27);
const CARD_BG: Color32 = Color32::from_rgb(24, 36, 48);
const CARD_BORDER: Color32 = Color32::from_rgb(43, 58, 72);
const BTN_SECONDARY: Color32 = Color32::from_rgb(33, 48, 63);
const ACCENT_RED: Color32 = Color32::from_rgb(255, 70, 85);
const TWITCH_PURPLE: Color32 = Color32::from_rgb(145, 70, 255);
const KICK_GREEN: Color32 = Color32::from_rgb(83, 252, 24);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(236, 232, 225);
const TEXT_MUTED: Color32 = Color32::from_rgb(148, 163, 184);
const LIVE_GREEN: Color32 = Color32::from_rgb(34, 197, 94);

const RIOT_CLIENT_PLATFORM: &str = "ew0KCSJwbGF0Zm9ybVR5cGUiOiAiUEMiLA0KCSJwbGF0Zm9ybU9TIjogIldpbmRvd3MiLA0KCSJwbGF0Zm9ybU9TVmVyc2lvbiI6ICIxMC4wLjE5MDQyLjEuMjU2LjY0Yml0IiwNCgkicGxhdGZvcm1DaGlwc2V0IjogIlVua25vd24iDQp9";

const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Window visibility is shared with tray handlers (which run off the UI thread).
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(false);
static QUIT: AtomicBool = AtomicBool::new(false);

// ============================================================================
// SMALL UTILITIES (locking, logging, atomic file IO)
// ============================================================================

/// Never panics on a poisoned mutex: a crash in one task must not brick the whole app.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn exe_dir() -> PathBuf {
    let mut path = env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    path.pop();
    path
}

/// `%APPDATA%\ValorantStreamerRadar` (writable even if the exe sits in Program Files).
fn data_dir() -> PathBuf {
    static DIR: LazyLock<PathBuf> = LazyLock::new(|| {
        let base = env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(exe_dir);
        let dir = base.join("ValorantStreamerRadar");
        let _ = fs::create_dir_all(&dir);
        dir
    });
    DIR.clone()
}

/// Windows subsystem apps have no console, so keep a small rotating log for diagnosis.
fn log(msg: impl AsRef<str>) {
    let dir = data_dir();
    let path = dir.join("radar.log");
    if fs::metadata(&path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = fs::rename(&path, dir.join("radar.old.log"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg.as_ref());
    }
}

/// Write-then-rename so a crash mid-write can never leave a truncated JSON file.
fn write_atomic(path: &Path, contents: &str) {
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, contents).is_ok() {
        if fs::rename(&tmp, path).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
}

fn save_json<T: Serialize + ?Sized>(name: &str, value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => write_atomic(&data_dir().join(name), &json),
        Err(e) => log(format!("Failed to serialize {name}: {e}")),
    }
}

/// Reads from the data dir, falling back to the legacy next-to-exe location.
fn load_json<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    let content = fs::read_to_string(data_dir().join(name))
        .or_else(|_| fs::read_to_string(exe_dir().join(name)));
    match content {
        Ok(c) => serde_json::from_str(&c).unwrap_or_else(|e| {
            log(format!("{name} is corrupt ({e}); starting empty"));
            let _ = fs::copy(data_dir().join(name), data_dir().join(format!("{name}.bak")));
            T::default()
        }),
        Err(_) => T::default(),
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

fn repaint(ctx: &egui::Context) {
    // Don't wake the UI loop at all while the window is hidden in the tray.
    if WINDOW_VISIBLE.load(Ordering::Relaxed) {
        ctx.request_repaint();
    }
}

async fn retry<T, F, Fut>(attempts: usize, delay: Duration, mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut last = anyhow!("no attempts made");
    for i in 0..attempts {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = e;
                if i + 1 < attempts {
                    sleep(delay).await;
                }
            }
        }
    }
    Err(last)
}

// ============================================================================
// DATA MODELS & STORAGE
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    #[serde(default)]
    youtube_api_key: String,
    #[serde(default)]
    twitch_client_id: String,
    #[serde(default)]
    twitch_client_secret: String,
    #[serde(default = "default_true")]
    start_with_windows: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            youtube_api_key: String::new(),
            twitch_client_id: String::new(),
            twitch_client_secret: String::new(),
            start_with_windows: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DirectoryStreamer {
    riot_id: String,
    platform: String,
    channel_name: String,
    channel_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamerEntry {
    match_id: String,
    riot_id: String,
    #[serde(default = "default_platform")]
    platform: String,
    channel: String,
    title: String,
    url: String,
    timestamp: String,
}

fn default_platform() -> String {
    "YouTube".to_string()
}

struct AppState {
    status: String,
    is_scanning: bool,
    valorant_running: bool,
    history: Vec<StreamerEntry>,
    local_directory: Vec<DirectoryStreamer>,
    toast: Option<(String, Instant)>,
    last_scanned_match: Option<String>,
}

fn set_toast(state: &Arc<Mutex<AppState>>, msg: impl Into<String>) {
    lock(state).toast = Some((msg.into(), Instant::now()));
}

fn set_status(state: &Arc<Mutex<AppState>>, ctx: &egui::Context, msg: impl Into<String>) {
    lock(state).status = msg.into();
    repaint(ctx);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanTrigger {
    Manual,
    PostMatch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntitlementsResponse {
    access_token: String,
    token: String,
    subject: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RiotNameRecord {
    #[serde(default)]
    subject: String,
    #[serde(default)]
    game_name: String,
    #[serde(default)]
    tag_line: String,
}

struct RiotAuth {
    access_token: String,
    token: String,
    subject: String,
    version: String,
}

// ============================================================================
// URL / PLATFORM VALIDATION (directory data is crowd-sourced = untrusted)
// ============================================================================

fn canonical_platform(p: &str) -> Option<&'static str> {
    match p.trim().to_ascii_lowercase().as_str() {
        "youtube" | "yt" => Some("YouTube"),
        "twitch" => Some("Twitch"),
        "kick" => Some("Kick"),
        _ => None,
    }
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return None; // rejects user@host tricks, ports, spaces, etc.
    }
    Some(host)
}

fn host_allowed(platform: &str, host: &str) -> bool {
    let h = host.trim_start_matches("www.").trim_start_matches("m.");
    match platform {
        "YouTube" => h == "youtube.com" || h == "youtu.be",
        "Twitch" => h == "twitch.tv",
        "Kick" => h == "kick.com",
        _ => false,
    }
}

fn strip_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

fn last_path_segment(url: &str) -> String {
    strip_query(url).trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string()
}

/// Returns a cleaned entry only if platform + URL host are consistent and safe.
fn sanitize_directory_entry(d: &DirectoryStreamer) -> Option<DirectoryStreamer> {
    let platform = canonical_platform(&d.platform)?;
    let url = d.channel_url.trim();
    if !host_allowed(platform, &host_of(url)?) || !d.riot_id.contains('#') {
        return None;
    }
    Some(DirectoryStreamer {
        riot_id: d.riot_id.trim().to_string(),
        platform: platform.to_string(),
        channel_name: short(d.channel_name.trim(), 64),
        channel_url: url.to_string(),
    })
}

fn validate_submission(rid: &str, platform: &str, cname: &str, curl: &str) -> Result<(), String> {
    let (name, tag) = rid.split_once('#').ok_or("Riot ID must look like Name#TAG.")?;
    let name_len = name.trim().chars().count();
    if !(3..=16).contains(&name_len) || !(3..=5).contains(&tag.len()) || !tag.chars().all(|c| c.is_alphanumeric()) {
        return Err("Riot ID looks invalid (name 3-16 chars, tag 3-5 letters/digits).".into());
    }
    if cname.is_empty() || cname.chars().count() > 64 {
        return Err("Enter a channel display name (max 64 characters).".into());
    }
    let host = host_of(curl).ok_or("Channel URL must start with https:// and be a plain link.")?;
    if !host_allowed(platform, &host) {
        return Err(format!("That URL is not a {platform} link."));
    }
    Ok(())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

// ============================================================================
// RIOT LOCAL CLIENT & AP (MUMBAI) MATCH ENDPOINTS
// ============================================================================

fn read_riot_lockfile() -> Result<(String, String)> {
    let local_app_data = env::var_os("LOCALAPPDATA").context("LOCALAPPDATA not set")?;
    let lockfile_path = PathBuf::from(local_app_data)
        .join("Riot Games")
        .join("Riot Client")
        .join("Config")
        .join("lockfile");

    let content = fs::read_to_string(lockfile_path).context("Riot Client not running")?;
    let parts: Vec<&str> = content.trim().split(':').collect();
    if parts.len() < 5 {
        return Err(anyhow!("Invalid lockfile format"));
    }
    Ok((parts[2].to_string(), parts[3].to_string()))
}

static CLIENT_VERSION: Mutex<Option<(String, Instant)>> = Mutex::new(None);

async fn client_version(client: &Client) -> Result<String> {
    let cached = lock(&CLIENT_VERSION).clone();
    if let Some((v, at)) = &cached {
        if at.elapsed() < Duration::from_secs(3600) {
            return Ok(v.clone());
        }
    }
    let fetched: Result<String> = async {
        let resp: Value = client
            .get("https://valorant-api.com/v1/version")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        resp["data"]["riotClientVersion"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Missing riotClientVersion"))
    }
    .await;

    match (fetched, cached) {
        (Ok(v), _) => {
            *lock(&CLIENT_VERSION) = Some((v.clone(), Instant::now()));
            Ok(v)
        }
        // valorant-api.com hiccup: a stale version is far better than no scan at all.
        (Err(e), Some((stale, _))) => {
            log(format!("Version refresh failed ({e}); using cached {stale}"));
            Ok(stale)
        }
        (Err(e), None) => Err(e),
    }
}

fn riot_headers(rb: reqwest::RequestBuilder, a: &RiotAuth) -> reqwest::RequestBuilder {
    rb.header("Authorization", format!("Bearer {}", a.access_token))
        .header("X-Riot-Entitlements-JWT", &a.token)
        .header("X-Riot-ClientPlatform", RIOT_CLIENT_PLATFORM)
        .header("X-Riot-ClientVersion", &a.version)
}

async fn riot_json(rb: reqwest::RequestBuilder) -> Result<Value> {
    let resp = rb.send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("Riot API returned HTTP {status}"));
    }
    Ok(resp.json().await?)
}

async fn riot_authenticate(local: &Client, public: &Client) -> Result<RiotAuth> {
    let (port, password) = read_riot_lockfile()?;
    let auth_header = format!("Basic {}", STANDARD.encode(format!("riot:{password}")));
    let ent: EntitlementsResponse = local
        .get(format!("https://127.0.0.1:{port}/entitlements/v1/token"))
        .header("Authorization", auth_header)
        .send()
        .await
        .context("Riot Client local API unreachable")?
        .error_for_status()
        .context("Entitlements request rejected")?
        .json()
        .await
        .context("Unexpected entitlements response")?;
    let version = client_version(public).await?;
    Ok(RiotAuth { access_token: ent.access_token, token: ent.token, subject: ent.subject, version })
}

async fn latest_match_id(public: &Client, auth: &RiotAuth) -> Result<String> {
    let url = format!(
        "https://pd.ap.a.pvp.net/match-history/v1/history/{}?startIndex=0&endIndex=1",
        auth.subject
    );
    let v = riot_json(riot_headers(public.get(url), auth)).await?;
    v["History"][0]["MatchID"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("No recent match found"))
}

async fn fetch_players(public: &Client, auth: &RiotAuth, match_id: &str) -> Result<Vec<RiotNameRecord>> {
    let url = format!("https://pd.ap.a.pvp.net/match-details/v1/matches/{match_id}");
    let details = riot_json(riot_headers(public.get(url), auth)).await?;
    let puuids: Vec<String> = details["players"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| p["subject"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    if puuids.is_empty() {
        return Err(anyhow!("Match details contained no players yet"));
    }
    let names = riot_json(riot_headers(
        public.put("https://pd.ap.a.pvp.net/name-service/v2/players").json(&puuids),
        auth,
    ))
    .await?;
    serde_json::from_value(names).context("Unexpected name-service response")
}

/// For post-match scans the latest MatchID may still be the *previous* match while history
/// propagates, so poll until it changes instead of trusting a fixed sleep.
async fn fetch_match_and_names(
    local: &Client,
    public: &Client,
    previous: Option<&str>,
    wait_for_new: bool,
) -> Result<(String, String, Vec<RiotNameRecord>)> {
    let auth = riot_authenticate(local, public).await?;
    let attempts = if wait_for_new { 6 } else { 2 };
    let match_id = retry(attempts, Duration::from_secs(3), || async {
        let id = latest_match_id(public, &auth).await?;
        if wait_for_new && Some(id.as_str()) == previous {
            return Err(anyhow!("Match history has not updated yet"));
        }
        Ok(id)
    })
    .await?;
    let players = retry(3, Duration::from_secs(3), || fetch_players(public, &auth, &match_id)).await?;
    Ok((auth.subject, match_id, players))
}

// ============================================================================
// STREAMING PLATFORM DETECTORS (YOUTUBE, TWITCH, KICK)
// ============================================================================

static SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Separator-delimited suffixes ("Foo_TV", "Foo Gaming") plus unambiguous glued ones ("FooYT").
    // Avoids mangling normal names such as "Desktop" or "Alive".
    Regex::new(r"(?i)(?:[\s._-]+(?:yt|youtube|live|ttv|tv|gaming|plays|op)|(?:yt|ttv))$").unwrap()
});
static YT_DATA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"var ytInitialData = (\{.*?\});</script>").unwrap());
static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<title>(.*?) - YouTube</title>").unwrap());
static CANON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<link rel="canonical" href="([^"]+)""#).unwrap());

fn clean_ign(game_name: &str) -> String {
    SUFFIX_RE.replace(game_name.trim(), "").trim().to_string()
}

fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

async fn http_get_text(client: &Client, url: &str) -> Option<String> {
    let resp = client
        .get(url)
        .header("User-Agent", BROWSER_UA)
        .header("Accept-Language", "en-IN,en;q=0.9")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

/// Directory-linked YouTube channel: check its `/live` endpoint directly.
async fn check_youtube_channel_live(client: &Client, channel_url: &str) -> Option<(String, String)> {
    let live_tab = format!("{}/live", strip_query(channel_url).trim_end_matches('/'));
    let html = http_get_text(client, &live_tab).await?;
    if !html.contains(r#""isLive":true"#) {
        return None;
    }
    let title = TITLE_RE
        .captures(&html)
        .map(|c| decode_entities(&c[1]))
        .unwrap_or_else(|| "Live Stream".to_string());
    // Prefer the concrete watch URL so the link survives the stream ending.
    let url = CANON_RE
        .captures(&html)
        .map(|c| c[1].to_string())
        .filter(|u| u.starts_with("https://www.youtube.com/watch"))
        .unwrap_or(live_tab);
    Some((title, url))
}

/// Heuristic: channel name must contain the cleaned IGN (title-only matches are too noisy).
async fn check_youtube_live(client: &Client, game_name: &str, api_key: &str) -> Option<(String, String, String)> {
    let cleaned = clean_ign(game_name);
    let name_norm = normalize(&cleaned);
    if name_norm.chars().count() < 4 {
        return None;
    }
    let query = format!("{cleaned} Valorant");
    let matches = |channel: &str| normalize(channel).contains(&name_norm);

    if !api_key.trim().is_empty() {
        match client
            .get("https://www.googleapis.com/youtube/v3/search")
            .query(&[
                ("part", "snippet"),
                ("q", query.as_str()),
                ("type", "video"),
                ("eventType", "live"),
                ("regionCode", "IN"),
                ("maxResults", "5"),
                ("key", api_key.trim()),
            ])
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<Value>().await {
                    if let Some(items) = data["items"].as_array() {
                        for item in items {
                            let video_id = item["id"]["videoId"].as_str().unwrap_or("");
                            let title = item["snippet"]["title"].as_str().unwrap_or("");
                            let channel = item["snippet"]["channelTitle"].as_str().unwrap_or("");
                            if !video_id.is_empty() && matches(channel) {
                                return Some((
                                    channel.to_string(),
                                    decode_entities(title),
                                    format!("https://www.youtube.com/watch?v={video_id}"),
                                ));
                            }
                        }
                        return None;
                    }
                }
            }
            // Quota exhausted / bad key: fall through to the keyless path.
            Ok(resp) => log(format!("YouTube API returned {}; using keyless fallback", resp.status())),
            Err(e) => log(format!("YouTube API request failed: {e}")),
        }
    }

    let url = format!(
        "https://www.youtube.com/results?search_query={}&sp=EgJAAQ%3D%3D",
        urlencode(&query)
    );
    let html = http_get_text(client, &url).await?;
    let caps = YT_DATA_RE.captures(&html)?;
    let data: Value = serde_json::from_str(&caps[1]).ok()?;
    let sections = data["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]
        ["sectionListRenderer"]["contents"]
        .as_array()?;

    for section in sections {
        let Some(items) = section["itemSectionRenderer"]["contents"].as_array() else { continue };
        for item in items.iter().take(6) {
            let Some(video) = item.get("videoRenderer") else { continue };
            let video_id = video["videoId"].as_str().unwrap_or("");
            let title = video["title"]["runs"][0]["text"].as_str().unwrap_or("");
            let channel = video["ownerText"]["runs"][0]["text"].as_str().unwrap_or("");
            if !video_id.is_empty() && matches(channel) {
                return Some((
                    channel.to_string(),
                    title.to_string(),
                    format!("https://www.youtube.com/watch?v={video_id}"),
                ));
            }
        }
    }
    None
}

static TWITCH_TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// OAuth2 client_credentials with an in-memory cache that refreshes shortly before expiry.
async fn get_twitch_app_token(client: &Client, client_id: &str, client_secret: &str) -> Option<String> {
    if client_id.trim().is_empty() || client_secret.trim().is_empty() {
        return None;
    }
    let cached = lock(&TWITCH_TOKEN).clone();
    if let Some((token, expires)) = cached {
        if Instant::now() < expires {
            return Some(token);
        }
    }
    let resp = client
        .post("https://id.twitch.tv/oauth2/token")
        .form(&[
            ("client_id", client_id.trim()),
            ("client_secret", client_secret.trim()),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        log(format!("Twitch token request failed: HTTP {}", resp.status()));
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let token = v["access_token"].as_str()?.to_string();
    let ttl = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(300);
    *lock(&TWITCH_TOKEN) = Some((token.clone(), Instant::now() + Duration::from_secs(ttl)));
    Some(token)
}

async fn check_twitch_live(
    client: &Client,
    login_or_ign: &str,
    is_exact_login: bool,
    client_id: &str,
    bearer_token: Option<&str>,
) -> Option<(String, String, String)> {
    let login = if is_exact_login {
        login_or_ign.trim().to_lowercase()
    } else {
        clean_ign(login_or_ign).to_lowercase().replace(' ', "")
    };
    let min_len = if is_exact_login { 3 } else { 4 };
    if login.len() < min_len || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }

    // 1. Official Helix API
    if let Some(token) = bearer_token {
        match client
            .get("https://api.twitch.tv/helix/streams")
            .header("Client-Id", client_id.trim())
            .header("Authorization", format!("Bearer {token}"))
            .query(&[("user_login", login.as_str())])
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<Value>().await {
                    if let Some(stream) = data["data"].as_array().and_then(|a| a.first()) {
                        let game = stream["game_name"].as_str().unwrap_or("");
                        if is_exact_login || game.eq_ignore_ascii_case("VALORANT") {
                            let display = stream["user_name"].as_str().unwrap_or(&login).to_string();
                            let title = stream["title"].as_str().unwrap_or("Live on Twitch").to_string();
                            return Some((display, title, format!("https://www.twitch.tv/{login}")));
                        }
                    }
                    return None;
                }
            }
            Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
                *lock(&TWITCH_TOKEN) = None; // force refresh next scan, fall back to GQL now
            }
            _ => {}
        }
    }

    // 2. Keyless public web GraphQL fallback (variables, not string interpolation)
    let body = serde_json::json!({
        "query": "query($login: String!) { user(login: $login) { displayName stream { title game { name } } } }",
        "variables": { "login": login }
    });
    let resp = client
        .post("https://gql.twitch.tv/gql")
        .header("Client-Id", "kimne78kx3ncx6brgo4mv6wki5h1ko")
        .json(&body)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let stream = &v["data"]["user"]["stream"];
    if stream.is_object() {
        let game = stream["game"]["name"].as_str().unwrap_or("");
        if is_exact_login || game.eq_ignore_ascii_case("VALORANT") {
            let display = v["data"]["user"]["displayName"].as_str().unwrap_or(&login).to_string();
            let title = stream["title"].as_str().unwrap_or("Live on Twitch").to_string();
            return Some((display, title, format!("https://www.twitch.tv/{login}")));
        }
    }
    None
}

/// Note: Kick sits behind Cloudflare and may answer 403 at times; that simply yields "not live".
async fn check_kick_live(client: &Client, slug_or_ign: &str, is_exact_slug: bool) -> Option<(String, String, String)> {
    let slug = if is_exact_slug {
        slug_or_ign.trim().to_lowercase()
    } else {
        clean_ign(slug_or_ign).to_lowercase().replace(' ', "_")
    };
    let min_len = if is_exact_slug { 3 } else { 4 };
    if slug.len() < min_len || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }

    let resp = client
        .get(format!("https://kick.com/api/v2/channels/{slug}"))
        .header("User-Agent", BROWSER_UA)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    if v["livestream"].is_object() {
        let title = v["livestream"]["session_title"].as_str().unwrap_or("Live on Kick").to_string();
        let channel = v["user"]["username"].as_str().unwrap_or(&slug).to_string();
        return Some((channel, title, format!("https://kick.com/{slug}")));
    }
    None
}

// ============================================================================
// DIRECTORY, NOTIFICATIONS & SCANNER
// ============================================================================

async fn load_directory(public: &Client, local: Vec<DirectoryStreamer>) -> Vec<DirectoryStreamer> {
    let gh_url = format!(
        "https://raw.githubusercontent.com/{GITHUB_OWNER}/{GITHUB_REPO}/main/streamers.json"
    );
    let remote: Option<Vec<DirectoryStreamer>> = async {
        let resp = public.get(&gh_url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        // Parse leniently: one malformed community entry must not discard the whole directory.
        let raw: Vec<Value> = resp.json().await.ok()?;
        Some(raw.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
    }
    .await;

    let remote = match remote {
        Some(list) => {
            save_json("directory_cache.json", &list);
            list
        }
        None => {
            log("GitHub directory unavailable; using cached copy");
            load_json::<Vec<DirectoryStreamer>>("directory_cache.json")
        }
    };

    let mut seen = HashSet::new();
    local
        .iter()
        .chain(remote.iter())
        .filter_map(sanitize_directory_entry)
        .filter(|d| seen.insert((d.riot_id.to_lowercase(), d.platform.clone(), d.channel_url.to_lowercase())))
        .collect()
}

#[cfg(windows)]
fn show_toast(entry: &StreamerEntry) {
    use tauri_winrt_notification::{Duration as ToastDuration, Toast};
    let url = entry.url.clone();
    let title = format!("[{}] LIVE: {}", entry.platform, entry.riot_id);
    let line1 = format!("{} ({})", short(&entry.title, 90), entry.channel);
    let result = Toast::new(Toast::POWERSHELL_APP_ID)
        .title(&title)
        .text1(&line1)
        .text2("Click to open stream • link copied to clipboard")
        .duration(ToastDuration::Long)
        .on_activated(move |_| {
            let _ = open::that(&url);
            Ok(())
        })
        .show();
    if let Err(e) = result {
        log(format!("Toast failed: {e}"));
    }
}

#[cfg(not(windows))]
fn show_toast(entry: &StreamerEntry) {
    log(format!("LIVE: {} -> {}", entry.riot_id, entry.url));
}

/// Returns true if this detection was new (deduplicated by match_id + riot_id + platform).
fn record_and_notify(state: &Arc<Mutex<AppState>>, ctx: &egui::Context, entry: StreamerEntry, copy: bool) {
    let is_new = {
        let mut s = lock(state);
        let exists = s
            .history
            .iter()
            .any(|e| e.match_id == entry.match_id && e.riot_id == entry.riot_id && e.platform == entry.platform);
        if !exists {
            s.history.insert(0, entry.clone());
            s.history.truncate(MAX_HISTORY);
            save_json("streamer_history.json", &s.history);
        }
        !exists
    };
    repaint(ctx);

    if copy {
        if let Ok(mut cb) = Clipboard::new() {
            let _ = cb.set_text(entry.url.clone());
        }
    }
    if is_new {
        show_toast(&entry);
    }
}

async fn scan_player(
    client: Client,
    config: AppConfig,
    directory: Arc<Vec<DirectoryStreamer>>,
    twitch_token: Option<String>,
    match_id: String,
    player: RiotNameRecord,
) -> Vec<StreamerEntry> {
    let riot_id = format!("{}#{}", player.game_name, player.tag_line);
    let timestamp = Local::now().format("%d %b %Y • %I:%M %p").to_string();
    let make = |platform: &str, channel: String, title: String, url: String| StreamerEntry {
        match_id: match_id.clone(),
        riot_id: riot_id.clone(),
        platform: platform.to_string(),
        channel,
        title,
        url,
        timestamp: timestamp.clone(),
    };

    // Phase 1: exact Riot ID mapping (authoritative; skips noisy heuristics when present)
    let linked: Vec<&DirectoryStreamer> =
        directory.iter().filter(|d| d.riot_id.eq_ignore_ascii_case(&riot_id)).collect();
    if !linked.is_empty() {
        let mut out = Vec::new();
        for d in linked {
            let handle = last_path_segment(&d.channel_url);
            let hit = match d.platform.as_str() {
                "Twitch" => check_twitch_live(&client, &handle, true, &config.twitch_client_id, twitch_token.as_deref())
                    .await
                    .map(|(_, t, u)| (t, u)),
                "Kick" => check_kick_live(&client, &handle, true).await.map(|(_, t, u)| (t, u)),
                _ => check_youtube_channel_live(&client, &d.channel_url).await,
            };
            if let Some((title, url)) = hit {
                out.push(make(&d.platform, d.channel_name.clone(), title, url));
            }
        }
        return out;
    }

    // Phase 2: concurrent heuristic fallback
    let (yt, tw, kick) = tokio::join!(
        check_youtube_live(&client, &player.game_name, &config.youtube_api_key),
        check_twitch_live(&client, &player.game_name, false, &config.twitch_client_id, twitch_token.as_deref()),
        check_kick_live(&client, &player.game_name, false),
    );
    let mut out = Vec::new();
    if let Some((c, t, u)) = yt {
        out.push(make("YouTube", c, t, u));
    }
    if let Some((c, t, u)) = tw {
        out.push(make("Twitch", c, t, u));
    }
    if let Some((c, t, u)) = kick {
        out.push(make("Kick", c, t, u));
    }
    out
}

async fn scan_last_match(
    local_client: Client,
    public_client: Client,
    config: AppConfig,
    state: Arc<Mutex<AppState>>,
    ctx: egui::Context,
    trigger: ScanTrigger,
) {
    set_status(&state, &ctx, "Syncing GitHub directory & scanning last match...");

    let previous = lock(&state).last_scanned_match.clone();
    let fetched = fetch_match_and_names(
        &local_client,
        &public_client,
        previous.as_deref(),
        trigger == ScanTrigger::PostMatch,
    )
    .await;

    let (my_puuid, match_id, players) = match fetched {
        Ok(v) => v,
        Err(e) => {
            log(format!("Match fetch failed: {e:#}"));
            set_status(&state, &ctx, format!("Could not fetch match: {}", short(&e.to_string(), 70)));
            return;
        }
    };
    lock(&state).last_scanned_match = Some(match_id.clone());

    let local_dir = lock(&state).local_directory.clone();
    let directory = Arc::new(load_directory(&public_client, local_dir).await);
    let twitch_token =
        get_twitch_app_token(&public_client, &config.twitch_client_id, &config.twitch_client_secret).await;

    let sem = Arc::new(Semaphore::new(4));
    let mut set = JoinSet::new();
    let mut seen = HashSet::new();
    for player in players {
        if player.subject == my_puuid || player.game_name.trim().is_empty() || !seen.insert(player.subject.clone()) {
            continue;
        }
        let (client, cfg, dir, tok, mid, sem) = (
            public_client.clone(),
            config.clone(),
            directory.clone(),
            twitch_token.clone(),
            match_id.clone(),
            sem.clone(),
        );
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            scan_player(client, cfg, dir, tok, mid, player).await
        });
    }

    let mut found = 0usize;
    while let Some(res) = set.join_next().await {
        match res {
            Ok(entries) => {
                for entry in entries {
                    record_and_notify(&state, &ctx, entry, found == 0);
                    found += 1;
                }
            }
            Err(e) => log(format!("Player scan task failed: {e}")),
        }
    }

    let status = if found > 0 {
        format!("Found {found} live stream(s) in last match!")
    } else {
        format!("Last checked at {} • No live streamers found", Local::now().format("%I:%M %p"))
    };
    set_status(&state, &ctx, status);
}

// ============================================================================
// LOG WATCHER
// ============================================================================

/// Reads only *complete* lines appended after `start`. A half-written trailing line is left for
/// the next poll (the original code lost the tail of such lines). Invalid UTF-8 is tolerated.
fn read_new_reconcile_lines(path: &Path, start: u64) -> std::io::Result<(Vec<String>, u64)> {
    const CAP: u64 = 8 * 1024 * 1024;
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.take(CAP).read_to_end(&mut buf)?;

    let end = match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => i + 1,
        None if buf.len() as u64 >= CAP => buf.len(), // pathological no-newline blob: skip it
        None => 0,
    };
    let lines = String::from_utf8_lossy(&buf[..end])
        .lines()
        .filter(|l| l.contains("Reconcile"))
        .map(String::from)
        .collect();
    Ok((lines, start + end as u64))
}

async fn watch_log(
    log_path: PathBuf,
    state: Arc<Mutex<AppState>>,
    ctx: egui::Context,
    tx: mpsc::UnboundedSender<ScanTrigger>,
) {
    let mut in_match = false;
    let initial = fs::metadata(&log_path).ok();
    let mut last_pos = initial.as_ref().map(|m| m.len()).unwrap_or(0);
    let mut last_created = initial.and_then(|m| m.created().ok());
    let mut last_running = None;

    loop {
        let running = read_riot_lockfile().is_ok();
        if last_running != Some(running) {
            last_running = Some(running);
            lock(&state).valorant_running = running;
            repaint(&ctx);
        }

        let Ok(meta) = fs::metadata(&log_path) else {
            // Log not there (Valorant never launched / being rotated): restart from the top later.
            last_pos = 0;
            last_created = None;
            in_match = false;
            sleep(Duration::from_secs(5)).await;
            continue;
        };

        let created = meta.created().ok();
        let len = meta.len();
        if len < last_pos || (created.is_some() && created != last_created) {
            // New game session replaced the log.
            last_pos = 0;
            in_match = false;
        }
        last_created = created;

        if len > last_pos {
            match read_new_reconcile_lines(&log_path, last_pos) {
                Ok((lines, new_pos)) => {
                    last_pos = new_pos;
                    for line in lines {
                        if line.contains("InGame") {
                            if !in_match {
                                in_match = true;
                                set_status(&state, &ctx, "In Match • Waiting for match end...");
                            }
                        } else if in_match && line.contains("MainMenu") {
                            in_match = false;
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                sleep(Duration::from_secs(4)).await; // let match history propagate
                                let _ = tx.send(ScanTrigger::PostMatch);
                            });
                        }
                    }
                }
                Err(e) => log(format!("Log read failed: {e}")),
            }
        }
        sleep(Duration::from_secs(1)).await;
    }
}

// ============================================================================
// SYSTEM TRAY, AUTOSTART & EGUI DESKTOP UI
// ============================================================================

fn create_tray_icon() -> Icon {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let in_border = x < 2 || x >= size - 2 || y < 2 || y >= size - 2;
            let dx = (x as i32 - 16).abs();
            let in_v = (7..=24).contains(&y) && (dx - ((24 - y as i32) / 2)).abs() <= 2;
            let px: [u8; 4] = if in_border || in_v { [255, 70, 85, 255] } else { [15, 25, 35, 255] };
            rgba[idx..idx + 4].copy_from_slice(&px);
        }
    }
    Icon::from_rgba(rgba, size, size).expect("Failed to create tray icon")
}

#[cfg(windows)]
fn set_autostart(enable: bool) {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok((key, _)) = hkcu.create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") else {
        return;
    };
    if enable {
        if let Ok(exe) = env::current_exe() {
            let _ = key.set_value("ValorantStreamerRadar", &format!("\"{}\"", exe.display()));
        }
    } else {
        let _ = key.delete_value("ValorantStreamerRadar");
    }
}

#[cfg(not(windows))]
fn set_autostart(_enable: bool) {}

fn show_window(ctx: &egui::Context) {
    WINDOW_VISIBLE.store(true, Ordering::Relaxed);
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

fn quit_app(ctx: &egui::Context) {
    QUIT.store(true, Ordering::Relaxed);
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    ctx.request_repaint();
    // Dropping out of run_native removes the tray icon cleanly; this is just a safety net.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        std::process::exit(0);
    });
}

struct StreamerApp {
    state: Arc<Mutex<AppState>>,
    manual_scan_tx: mpsc::UnboundedSender<ScanTrigger>,
    search_query: String,
    show_add_modal: bool,
    input_riot_id: String,
    input_platform: String,
    input_channel_name: String,
    input_channel_url: String,
    modal_error: Option<String>,
}

impl StreamerApp {
    fn new(cc: &eframe::CreationContext<'_>, state: Arc<Mutex<AppState>>, tx: mpsc::UnboundedSender<ScanTrigger>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG_DARK;
        visuals.window_fill = CARD_BG;
        visuals.widgets.noninteractive.bg_fill = CARD_BG;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 44, 58);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 60, 78);
        visuals.selection.bg_fill = ACCENT_RED;
        cc.egui_ctx.set_visuals(visuals);

        Self {
            state,
            manual_scan_tx: tx,
            search_query: String::new(),
            show_add_modal: false,
            input_riot_id: String::new(),
            input_platform: "YouTube".to_string(),
            input_channel_name: String::new(),
            input_channel_url: String::new(),
            modal_error: None,
        }
    }

    fn submit_modal(&mut self) {
        let rid = self.input_riot_id.trim().to_string();
        let cname = self.input_channel_name.trim().to_string();
        let curl = self.input_channel_url.trim().to_string();
        let platform = self.input_platform.clone();

        if let Err(msg) = validate_submission(&rid, &platform, &cname, &curl) {
            self.modal_error = Some(msg);
            return;
        }

        let new_entry = DirectoryStreamer { riot_id: rid.clone(), platform: platform.clone(), channel_name: cname, channel_url: curl };
        {
            let mut s = lock(&self.state);
            // Replace only the same Riot ID + platform (a player may stream on several platforms).
            s.local_directory
                .retain(|x| !(x.riot_id.eq_ignore_ascii_case(&rid) && x.platform == platform));
            s.local_directory.push(new_entry.clone());
            save_json("custom_streamers.json", &s.local_directory);
        }
        set_toast(&self.state, format!("Saved {rid} locally & opened GitHub!"));

        let issue_title = format!("Add Streamer: {rid}");
        let json_payload = serde_json::to_string_pretty(&new_entry).unwrap_or_default();
        let issue_body = format!("### New Streamer Submission\n\n```json\n{json_payload}\n```");
        let gh_issue_url = format!(
            "https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}/issues/new?title={}&body={}",
            urlencode(&issue_title),
            urlencode(&issue_body)
        );
        let _ = open::that(gh_issue_url);

        self.input_riot_id.clear();
        self.input_channel_name.clear();
        self.input_channel_url.clear();
        self.modal_error = None;
        self.show_add_modal = false;
    }
}

impl eframe::App for StreamerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if QUIT.load(Ordering::Relaxed) {
                return; // let the real quit through
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            WINDOW_VISIBLE.store(false, Ordering::Relaxed);
        }

        // Hidden in the tray: do nothing and schedule nothing → true 0% idle CPU.
        // Tray events wake us through their own handlers (see main).
        if !WINDOW_VISIBLE.load(Ordering::Relaxed) {
            return;
        }

        let (status, is_scanning, valorant_running, history, toast) = {
            let mut s = lock(&self.state);
            if matches!(&s.toast, Some((_, at)) if at.elapsed() > TOAST_TTL) {
                s.toast = None;
            }
            (
                s.status.clone(),
                s.is_scanning,
                s.valorant_running,
                s.history.clone(),
                s.toast.as_ref().map(|(m, _)| m.clone()),
            )
        };
        if toast.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500)); // so the toast can expire
        }

        // Header
        egui::TopBottomPanel::top("header")
            .frame(Frame::none().fill(HEADER_BG).inner_margin(Margin::symmetric(22.0, 16.0)).stroke(Stroke::new(1.0, CARD_BORDER)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("VALORANT").font(FontId::proportional(20.0)).strong().color(ACCENT_RED));
                            ui.label(RichText::new("STREAMER RADAR").font(FontId::proportional(20.0)).strong().color(TEXT_PRIMARY));
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let (dot, text) = if valorant_running {
                                (LIVE_GREEN, "Valorant Active (Mumbai / AP)")
                            } else {
                                (TEXT_MUTED, "Waiting for Valorant...")
                            };
                            ui.label(RichText::new("●").color(dot).size(11.0));
                            ui.label(RichText::new(text).color(TEXT_MUTED).size(12.5));
                            ui.label(RichText::new("  |  ").color(CARD_BORDER));
                            ui.label(RichText::new(&status).color(TEXT_MUTED).size(12.5));
                        });
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if is_scanning { "Scanning..." } else { "Check Last Match" };
                        let scan_btn = egui::Button::new(RichText::new(label).color(Color32::WHITE).strong().size(13.0))
                            .fill(if is_scanning { CARD_BORDER } else { ACCENT_RED })
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(138.0, 36.0));
                        if ui.add_enabled(!is_scanning, scan_btn).clicked() {
                            let _ = self.manual_scan_tx.send(ScanTrigger::Manual);
                        }

                        ui.add_space(8.0);

                        let add_btn = egui::Button::new(RichText::new("+ Add Streamer").color(TEXT_PRIMARY).strong().size(13.0))
                            .fill(BTN_SECONDARY)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(125.0, 36.0));
                        if ui.add(add_btn).clicked() {
                            self.show_add_modal = true;
                            self.modal_error = None;
                        }
                    });
                });
            });

        // Add-streamer modal
        if self.show_add_modal {
            let (mut submit, mut cancel) = (false, false);
            egui::Window::new("Register Streamer to GitHub Directory")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([430.0, 310.0])
                .frame(
                    Frame::none()
                        .fill(CARD_BG)
                        .stroke(Stroke::new(1.5, ACCENT_RED))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(Margin::same(20.0)),
                )
                .show(ctx, |ui| {
                    ui.label(RichText::new("Link a Valorant Riot ID to a Stream Channel").color(TEXT_MUTED).size(12.5));
                    ui.add_space(12.0);

                    ui.label(RichText::new("Riot ID (IGN#TAG)").strong().color(TEXT_PRIMARY));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.input_riot_id)
                            .hint_text("e.g. RohanYT#IND")
                            .desired_width(f32::INFINITY)
                            .margin(Margin::symmetric(8.0, 6.0)),
                    );
                    ui.add_space(8.0);

                    ui.label(RichText::new("Platform").strong().color(TEXT_PRIMARY));
                    ui.horizontal(|ui| {
                        for plat in ["YouTube", "Twitch", "Kick"] {
                            ui.selectable_value(&mut self.input_platform, plat.to_string(), plat);
                        }
                    });
                    ui.add_space(8.0);

                    ui.label(RichText::new("Channel Display Name").strong().color(TEXT_PRIMARY));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.input_channel_name)
                            .hint_text("e.g. Rohan Gaming")
                            .desired_width(f32::INFINITY)
                            .margin(Margin::symmetric(8.0, 6.0)),
                    );
                    ui.add_space(8.0);

                    ui.label(RichText::new("Channel URL").strong().color(TEXT_PRIMARY));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.input_channel_url)
                            .hint_text("e.g. https://www.youtube.com/@RohanGaming")
                            .desired_width(f32::INFINITY)
                            .margin(Margin::symmetric(8.0, 6.0)),
                    );

                    if let Some(err) = &self.modal_error {
                        ui.add_space(6.0);
                        ui.label(RichText::new(err).color(ACCENT_RED).size(12.0));
                    }

                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        let submit_btn = egui::Button::new(RichText::new("Save & Submit to GitHub").color(Color32::WHITE).strong())
                            .fill(ACCENT_RED)
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(180.0, 34.0));
                        submit = ui.add(submit_btn).clicked();
                        ui.add_space(8.0);
                        cancel = ui
                            .add(egui::Button::new("Cancel").min_size(Vec2::new(80.0, 34.0)).rounding(Rounding::same(6.0)))
                            .clicked();
                    });
                });
            if submit {
                self.submit_modal();
            } else if cancel {
                self.show_add_modal = false;
            }
        }

        // History panel
        egui::CentralPanel::default()
            .frame(Frame::none().fill(BG_DARK).inner_margin(Margin::symmetric(22.0, 16.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("Search Riot ID, platform, channel, or stream title...")
                            .desired_width(350.0)
                            .margin(Margin::symmetric(10.0, 7.0)),
                    );
                    ui.label(RichText::new(format!("{} detected", history.len())).color(TEXT_MUTED).size(12.5));
                    if let Some(msg) = &toast {
                        ui.label(RichText::new(msg).color(LIVE_GREEN).size(12.5));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let clear = egui::Button::new(RichText::new("Clear History").color(TEXT_MUTED).size(12.0))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .rounding(Rounding::same(5.0));
                        if !history.is_empty() && ui.add(clear).clicked() {
                            let mut s = lock(&self.state);
                            s.history.clear();
                            save_json("streamer_history.json", &s.history);
                        }
                    });
                });

                ui.add_space(14.0);

                let q = self.search_query.to_lowercase();
                let filtered: Vec<&StreamerEntry> = history
                    .iter()
                    .filter(|e| {
                        q.is_empty()
                            || e.riot_id.to_lowercase().contains(&q)
                            || e.platform.to_lowercase().contains(&q)
                            || e.channel.to_lowercase().contains(&q)
                            || e.title.to_lowercase().contains(&q)
                    })
                    .collect();

                if filtered.is_empty() {
                    ui.add_space(80.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No Streamers Recorded Yet").font(FontId::proportional(18.0)).strong().color(TEXT_PRIMARY));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Play a match on Mumbai server or click '+ Add Streamer' to register your Riot ID & channel.")
                                .color(TEXT_MUTED)
                                .size(13.0),
                        );
                    });
                    return;
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in filtered {
                        let badge = match entry.platform.as_str() {
                            "Twitch" => TWITCH_PURPLE,
                            "Kick" => KICK_GREEN,
                            _ => ACCENT_RED,
                        };

                        Frame::none()
                            .fill(CARD_BG)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(Margin::symmetric(16.0, 14.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_max_width((ui.available_width() - 220.0).max(160.0));
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&entry.riot_id).font(FontId::proportional(16.0)).strong().color(Color32::WHITE));
                                            Frame::none()
                                                .fill(badge.linear_multiply(0.2))
                                                .stroke(Stroke::new(1.0, badge))
                                                .rounding(Rounding::same(4.0))
                                                .inner_margin(Margin::symmetric(6.0, 2.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new(&entry.platform).size(11.0).strong().color(badge));
                                                });
                                            ui.label(RichText::new(&entry.channel).font(FontId::proportional(13.5)).strong().color(badge));
                                        });
                                        ui.add_space(4.0);
                                        ui.label(RichText::new(&entry.title).color(TEXT_PRIMARY).size(13.0));
                                        ui.add_space(6.0);
                                        ui.label(RichText::new(&entry.timestamp).color(TEXT_MUTED).size(11.5));
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let watch = egui::Button::new(RichText::new("Watch Stream").color(Color32::WHITE).strong().size(12.5))
                                            .fill(ACCENT_RED)
                                            .rounding(Rounding::same(6.0))
                                            .min_size(Vec2::new(112.0, 32.0));
                                        if ui.add(watch).on_hover_text(&entry.url).clicked() {
                                            let _ = open::that(&entry.url);
                                        }

                                        let copy = egui::Button::new(RichText::new("Copy Link").color(TEXT_PRIMARY).size(12.5))
                                            .fill(BTN_SECONDARY)
                                            .stroke(Stroke::new(1.0, CARD_BORDER))
                                            .rounding(Rounding::same(6.0))
                                            .min_size(Vec2::new(84.0, 32.0));
                                        if ui.add(copy).clicked() {
                                            if let Ok(mut cb) = Clipboard::new() {
                                                let _ = cb.set_text(entry.url.clone());
                                            }
                                            set_toast(&self.state, "Copied link to clipboard!");
                                        }
                                    });
                                });
                            });
                        ui.add_space(10.0);
                    }
                });
            });
    }
}

// ============================================================================
// ENTRY POINT
// ============================================================================

fn main() -> Result<()> {
    std::panic::set_hook(Box::new(|info| log(format!("PANIC: {info}"))));

    // Single instance: a second launch (e.g. autostart + manual) exits quietly.
    let _instance_guard = match TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(l) => l,
        Err(_) => {
            log("Another instance is already running; exiting");
            return Ok(());
        }
    };

    let config = {
        let path = data_dir().join("config.json");
        if !path.exists() && !exe_dir().join("config.json").exists() {
            save_json("config.json", &AppConfig::default());
        }
        load_json::<AppConfig>("config.json")
    };
    set_autostart(config.start_with_windows);

    let state = Arc::new(Mutex::new(AppState {
        status: "Idle • Monitoring ShooterGame.log".to_string(),
        is_scanning: false,
        valorant_running: false,
        history: load_json("streamer_history.json"),
        local_directory: load_json("custom_streamers.json"),
        toast: None,
        last_scanned_match: None,
    }));

    let tray_menu = Menu::new();
    let open_item = MenuItem::new("Open Streamer History", true, None);
    let scan_item = MenuItem::new("Check Last Match Now", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let _ = tray_menu.append_items(&[&open_item, &scan_item, &quit_item]);
    let (open_id, scan_id, quit_id) = (open_item.id().clone(), scan_item.id().clone(), quit_item.id().clone());

    // Keep alive for the whole run; dropped (icon removed) when run_native returns.
    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Valorant Streamer Radar")
        .with_icon(create_tray_icon())
        .build()?;

    let (scan_tx, mut scan_rx) = mpsc::unbounded_channel::<ScanTrigger>();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Valorant Streamer Radar")
            .with_inner_size([820.0, 560.0])
            .with_min_inner_size([660.0, 440.0])
            .with_visible(false),
        ..Default::default()
    };

    let state_for_app = state.clone();
    let tx_for_app = scan_tx.clone();

    eframe::run_native(
        "Valorant Streamer Radar",
        native_options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();

            // Event-driven tray handling: works while the window is hidden and costs no polling.
            let ctx_tray = ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                    show_window(&ctx_tray);
                }
            }));
            let ctx_menu = ctx.clone();
            let tx_menu = scan_tx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id == open_id {
                    show_window(&ctx_menu);
                } else if event.id == scan_id {
                    let _ = tx_menu.send(ScanTrigger::Manual);
                } else if event.id == quit_id {
                    quit_app(&ctx_menu);
                }
            }));

            let state_bg = state.clone();
            let ctx_bg = ctx.clone();
            let cfg_bg = config.clone();
            let tx_bg = scan_tx.clone();

            let spawned = std::thread::Builder::new().name("radar-bg".into()).spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                    Ok(rt) => rt,
                    Err(e) => {
                        log(format!("Failed to start async runtime: {e}"));
                        return;
                    }
                };
                rt.block_on(async move {
                    let local_client = Client::builder()
                        .danger_accept_invalid_certs(true) // Riot's local API uses a self-signed cert
                        .no_proxy()
                        .timeout(Duration::from_secs(10))
                        .build()
                        .expect("local HTTP client");
                    let public_client = Client::builder()
                        .connect_timeout(Duration::from_secs(5))
                        .timeout(Duration::from_secs(10))
                        .build()
                        .expect("public HTTP client");

                    // Single worker serializes all scans (manual + post-match) and survives panics.
                    let (st, cx, cf) = (state_bg.clone(), ctx_bg.clone(), cfg_bg.clone());
                    tokio::spawn(async move {
                        while let Some(first) = scan_rx.recv().await {
                            let mut trigger = first;
                            while let Ok(t) = scan_rx.try_recv() {
                                if t == ScanTrigger::PostMatch {
                                    trigger = t;
                                }
                            }
                            lock(&st).is_scanning = true;
                            let handle = tokio::spawn(scan_last_match(
                                local_client.clone(),
                                public_client.clone(),
                                cf.clone(),
                                st.clone(),
                                cx.clone(),
                                trigger,
                            ));
                            if let Err(e) = handle.await {
                                log(format!("Scan task crashed: {e}"));
                                lock(&st).status = "Scan failed unexpectedly (see radar.log)".into();
                            }
                            lock(&st).is_scanning = false;
                            repaint(&cx);
                        }
                    });

                    let log_path = PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default())
                        .join("VALORANT")
                        .join("Saved")
                        .join("Logs")
                        .join("ShooterGame.log");
                    watch_log(log_path, state_bg, ctx_bg, tx_bg).await;
                });
            });
            if let Err(e) = spawned {
                log(format!("Failed to spawn background thread: {e}"));
            }

            Ok(Box::new(StreamerApp::new(cc, state_for_app, tx_for_app)))
        }),
    )
    .map_err(|e| anyhow!("GUI error: {e}"))
}
