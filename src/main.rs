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
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
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

const DEFAULT_GITHUB_OWNER: &str = "your-github-username";
const DEFAULT_GITHUB_REPO: &str = "valorant-streamers-db";
const DEFAULT_SHARD_OVERRIDE: &str = "auto";
const CUSTOM_TOAST_APP_ID: &str = "ValorantStreamerRadar";

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
const ALLY_TEAL: Color32 = Color32::from_rgb(45, 212, 191);
const ENEMY_AMBER: Color32 = Color32::from_rgb(251, 146, 60);

const RIOT_CLIENT_PLATFORM: &str = "ew0KCSJwbGF0Zm9ybVR5cGUiOiAiUEMiLA0KCSJwbGF0Zm9ybU9TIjogIldpbmRvd3MiLA0KCSJwbGF0Zm9ybU9TVmVyc2lvbiI6ICIxMC4wLjE5MDQyLjEuMjU2LjY0Yml0IiwNCgkicGxhdGZvcm1DaGlwc2V0IjogIlVua25vd24iDQp9";

const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Window visibility is shared with tray handlers (which run off the UI thread).
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(true);
static QUIT: AtomicBool = AtomicBool::new(false);
static CUSTOM_APP_ID_READY: AtomicBool = AtomicBool::new(false);

// ============================================================================
// SMALL UTILITIES (locking, logging, atomic file IO, procedural icon)
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
/// Never overwrites a corrupt file on disk; backs it up to `.bak` and returns defaults in memory.
fn load_json<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    let data_path = data_dir().join(name);
    let legacy_path = exe_dir().join(name);
    let (content, source_path) = match fs::read_to_string(&data_path) {
        Ok(c) => (Ok(c), data_path),
        Err(_) => (fs::read_to_string(&legacy_path), legacy_path),
    };
    match content {
        Ok(c) => serde_json::from_str(&c).unwrap_or_else(|e| {
            log(format!("{name} is corrupt ({e}); backing up to .bak and using defaults"));
            let _ = fs::copy(&source_path, data_dir().join(format!("{name}.bak")));
            T::default()
        }),
        Err(_) => T::default(),
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "..."
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

/// Generates the 32x32 RGBA pixel buffer (Obsidian #0F1923 fill, 2px Crimson #FF4655 border & "V").
fn radar_icon_rgba() -> Vec<u8> {
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
    rgba
}

/// Writes a valid 32x32 32-bit BGRA `.ico` file for Windows Toast branding with zero image crate deps.
#[cfg(windows)]
fn ensure_toast_icon_file() -> Option<PathBuf> {
    let path = data_dir().join("icon.ico");
    let rgba = radar_icon_rgba();
    let size: u32 = 32;
    let xor_len: u32 = size * size * 4;
    let and_len: u32 = size * 4; // 32-bit aligned 1bpp mask
    let dib_size: u32 = 40 + xor_len + and_len;

    let mut ico = Vec::with_capacity((22 + dib_size) as usize);
    // ICONDIR (6 bytes)
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    // ICONDIRENTRY (16 bytes)
    ico.push(size as u8);
    ico.push(size as u8);
    ico.push(0); // color count
    ico.push(0); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // bit count
    ico.extend_from_slice(&dib_size.to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes()); // offset
    // BITMAPINFOHEADER (40 bytes)
    ico.extend_from_slice(&40u32.to_le_bytes());
    ico.extend_from_slice(&(size as i32).to_le_bytes());
    ico.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // height * 2 (XOR + AND)
    ico.extend_from_slice(&1u16.to_le_bytes()); // planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // bpp
    ico.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    ico.extend_from_slice(&(xor_len + and_len).to_le_bytes());
    ico.extend_from_slice(&[0u8; 16]); // ppm + colors
    // Bottom-up BGRA pixels
    for y in (0..size).rev() {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            ico.push(rgba[idx + 2]); // B
            ico.push(rgba[idx + 1]); // G
            ico.push(rgba[idx]); // R
            ico.push(rgba[idx + 3]); // A
        }
    }
    // AND mask (all 0 = opaque, alpha channel controls visibility)
    ico.resize(ico.len() + and_len as usize, 0u8);

    fs::write(&path, &ico).ok()?;
    Some(path)
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
    #[serde(default = "default_shard_override")]
    shard_override: String,
    #[serde(default = "default_github_owner")]
    github_owner: String,
    #[serde(default = "default_github_repo")]
    github_repo: String,
    #[serde(default = "default_true")]
    use_custom_toast_app_id: bool,
}

fn default_true() -> bool {
    true
}

fn default_shard_override() -> String {
    DEFAULT_SHARD_OVERRIDE.to_string()
}

fn default_github_owner() -> String {
    DEFAULT_GITHUB_OWNER.to_string()
}

fn default_github_repo() -> String {
    DEFAULT_GITHUB_REPO.to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            youtube_api_key: String::new(),
            twitch_client_id: String::new(),
            twitch_client_secret: String::new(),
            start_with_windows: true,
            shard_override: default_shard_override(),
            github_owner: default_github_owner(),
            github_repo: default_github_repo(),
            use_custom_toast_app_id: true,
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
    #[serde(default)]
    team_relation: String,
    #[serde(default)]
    map_name: String,
    #[serde(default)]
    agent_name: String,
    #[serde(default)]
    queue_name: String,
}

fn default_platform() -> String {
    "YouTube".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RiotStatus {
    Offline,
    RiotClientOnly,
    ValorantActive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CheckState {
    Idle,
    Checking,
    Ok(String),
    Warn { msg: String, suggested_name: Option<String> },
    Err(String),
}

enum BgTask {
    TestYouTubeKey(String),
    TestTwitchKeys { client_id: String, client_secret: String },
    VerifyStreamer { riot_id: String, platform: String, channel_name: String, channel_url: String },
}

struct AppState {
    status: String,
    is_scanning: bool,
    is_syncing_directory: bool,
    riot_status: RiotStatus,
    detected_shard: String,
    config: AppConfig,
    history: Vec<StreamerEntry>,
    local_directory: Vec<DirectoryStreamer>,
    remote_directory: Vec<DirectoryStreamer>,
    toast: Option<(String, Instant)>,
    last_scanned_match: Option<String>,
    yt_key_check: CheckState,
    twitch_key_check: CheckState,
    streamer_verify: CheckState,
}

fn set_toast(state: &Arc<Mutex<AppState>>, msg: impl Into<String>) {
    lock(state).toast = Some((msg.into(), Instant::now()));
}

fn set_status(state: &Arc<Mutex<AppState>>, ctx: &egui::Context, msg: impl Into<String>) {
    lock(state).status = msg.into();
    repaint(ctx);
}

fn canonical_shard(s: &str) -> Option<&'static str> {
    match s.trim().to_ascii_lowercase().as_str() {
        "ap" => Some("ap"),
        "na" | "latam" | "br" => Some("na"),
        "eu" => Some("eu"),
        "kr" => Some("kr"),
        _ => None,
    }
}

fn shard_label(shard: &str) -> &'static str {
    match shard {
        "ap" => "AP / Mumbai",
        "na" => "NA / Americas",
        "eu" => "EU / Europe",
        "kr" => "KR / Korea",
        _ => "AP / Mumbai",
    }
}

fn effective_shard(config: &AppConfig, detected: &str) -> &'static str {
    canonical_shard(&config.shard_override)
        .or_else(|| canonical_shard(detected))
        .unwrap_or("ap")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanTrigger {
    Manual,
    PostMatch,
    RefreshDirectory,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntitlementsResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    subject: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RiotNameRecord {
    #[serde(default)]
    subject: String,
    #[serde(default)]
    game_name: String,
    #[serde(default)]
    tag_line: String,
}

#[derive(Debug, Clone)]
struct MatchContext {
    match_id: String,
    map_name: String,
    queue_name: String,
    players: Vec<MatchPlayerInfo>,
}

#[derive(Debug, Clone)]
struct MatchPlayerInfo {
    record: RiotNameRecord,
    team_relation: String,
    agent_name: String,
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

fn valid_github_segment(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty()
        && t.len() <= 100
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
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
// RIOT LOCAL CLIENT & MULTI-SHARD MATCH ENDPOINTS
// ============================================================================

fn read_riot_lockfile() -> Result<(String, String, String)> {
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
    let protocol = if parts[4].trim().is_empty() { "https" } else { parts[4].trim() };
    Ok((parts[2].trim().to_string(), parts[3].trim().to_string(), protocol.to_string()))
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

fn map_name_from_id(map_id: &str) -> String {
    let lower = map_id.to_ascii_lowercase();
    let known = match lower.as_str() {
        s if s.contains("ascent") => Some("Ascent"),
        s if s.contains("duality") => Some("Bind"),
        s if s.contains("triad") => Some("Haven"),
        s if s.contains("bonsai") => Some("Split"),
        s if s.contains("port") => Some("Icebox"),
        s if s.contains("foxtrot") => Some("Breeze"),
        s if s.contains("canyon") => Some("Fracture"),
        s if s.contains("pitt") => Some("Pearl"),
        s if s.contains("jam") => Some("Lotus"),
        s if s.contains("juliett") => Some("Sunset"),
        s if s.contains("infinity") => Some("Abyss"),
        s if s.contains("rook") => Some("Corrode"),
        s if s.contains("hurm_alley") => Some("District"),
        s if s.contains("hurm_bowl") => Some("Kasbah"),
        s if s.contains("hurm_yard") => Some("Piazza"),
        s if s.contains("hurm_helix") => Some("Drift"),
        s if s.contains("hurm_high") => Some("Glitch"),
        _ => None,
    };
    if let Some(name) = known {
        return name.to_string();
    }
    map_id.rsplit('/').next().unwrap_or("").to_string()
}

fn queue_name_from_id(queue_id: &str) -> String {
    match queue_id.trim().to_ascii_lowercase().as_str() {
        "competitive" => "Competitive".to_string(),
        "unrated" => "Unrated".to_string(),
        "swiftplay" => "Swiftplay".to_string(),
        "spikerush" => "Spike Rush".to_string(),
        "deathmatch" => "Deathmatch".to_string(),
        "hurm" => "Team Deathmatch".to_string(),
        "premier" => "Premier".to_string(),
        "ggteam" => "Escalation".to_string(),
        "" => "Custom".to_string(),
        other => other.to_string(),
    }
}

fn agent_name_from_id(char_id: &str) -> String {
    match char_id.trim().to_ascii_lowercase().as_str() {
        "add6443a-41bd-e414-f6ad-e58d267f4e95" => "Jett",
        "a3bfb853-43b2-7238-a4f1-ad90e9e46bcc" => "Reyna",
        "f94c3b30-42be-e959-889c-5aa313dba261" => "Raze",
        "569fdd95-4d10-43ab-ca70-79becc718b46" => "Sage",
        "320b2a48-4d9b-a075-30f1-1f93a9b638fa" => "Sova",
        "8e253930-4c05-31dd-1b6c-968525494517" => "Omen",
        "9f0d8ba9-4140-b941-57d3-a7ad57c6b417" => "Brimstone",
        "eb93336a-449b-9c1b-0a54-a891f7921d69" => "Phoenix",
        "707eab51-4836-f488-046a-cda6bf494859" => "Viper",
        "117ed9e3-49f3-6512-3ccf-0cada7e3823b" => "Cypher",
        "5f8d3a7f-467b-97f3-062c-13acf203c006" => "Breach",
        "1e58de9c-4950-5125-93e9-a0aee9f98746" => "Killjoy",
        "6f2a04ca-43e0-be17-7f36-b3908627744d" => "Skye",
        "7f94d92c-4234-0a36-9646-3a87eb8b5c89" => "Yoru",
        "41fb69c1-4189-7b37-f117-bcaf1e96f1bf" => "Astra",
        "601dbbe7-43ce-be57-2a40-4abd24953621" => "KAY/O",
        "22697a3d-45bf-8dd7-4fec-84a9e28c69d7" => "Chamber",
        "bb2a4828-46eb-8cd1-e765-15848195d751" => "Neon",
        "dade69b4-4f5a-8528-247b-219e5a1facd6" => "Fade",
        "95b78ed7-4637-86d9-7e41-71ba8c293152" => "Harbor",
        "e370fa57-4757-3604-3648-499e1f642d3f" => "Gekko",
        "cc8b64c8-4b25-4ff9-6e7f-37b4da43d235" => "Deadlock",
        "0e38b510-41a8-5780-5e8f-568b2a4f2d6c" => "Iso",
        "1dbf2edd-4729-0984-3115-daa5eed44993" => "Clove",
        "efba5359-4016-a1e5-7626-b1ae76895940" => "Vyse",
        "b444168c-4e35-8076-db47-ef9bf368f384" => "Tejo",
        "df1cb487-4902-002e-5c17-d28e83e78588" => "Waylay",
        _ => "",
    }
    .to_string()
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

async fn fetch_local_entitlements(local: &Client) -> Result<EntitlementsResponse> {
    let (port, password, protocol) = read_riot_lockfile()?;
    let auth_header = format!("Basic {}", STANDARD.encode(format!("riot:{password}")));
    let url = format!("{protocol}://127.0.0.1:{port}/entitlements/v1/token");

    let resp = local
        .get(&url)
        .header("Authorization", auth_header)
        .header("Accept", "application/json")
        .header("User-Agent", BROWSER_UA)
        .send()
        .await
        .context("Riot Client local API unreachable (is Valorant running?)")?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let riot_msg = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v["message"].as_str().map(String::from))
            .unwrap_or_default();
        log(format!("Entitlements HTTP {} from {}: {}", status.as_u16(), url, short(&body, 240)));

        return match status.as_u16() {
            400 | 404 => Err(anyhow!(
                "Open & sign into Valorant first (Riot Client HTTP {}{})",
                status.as_u16(),
                if riot_msg.is_empty() { String::new() } else { format!(": {}", short(&riot_msg, 40)) }
            )),
            401 | 403 => Err(anyhow!(
                "Riot lockfile rejected (HTTP {}) - restart Riot Client & Valorant",
                status.as_u16()
            )),
            _ => Err(anyhow!("Riot entitlements returned HTTP {}", status.as_u16())),
        };
    }

    let ent: EntitlementsResponse =
        serde_json::from_str(&body).context("Unexpected entitlements JSON from Riot Client")?;
    if ent.access_token.is_empty() || ent.token.is_empty() || ent.subject.is_empty() {
        return Err(anyhow!("Valorant session not ready yet - sign in & enter lobby"));
    }
    Ok(ent)
}

async fn probe_riot_session(local: &Client) -> RiotStatus {
    if read_riot_lockfile().is_err() {
        return RiotStatus::Offline;
    }
    match fetch_local_entitlements(local).await {
        Ok(_) => RiotStatus::ValorantActive,
        Err(_) => RiotStatus::RiotClientOnly,
    }
}

async fn riot_authenticate(local: &Client, public: &Client) -> Result<RiotAuth> {
    // Re-read lockfile across brief retries to survive Riot Client startup / port rotation races.
    let ent = retry(2, Duration::from_millis(1200), || fetch_local_entitlements(local)).await?;
    let version = client_version(public).await?;
    Ok(RiotAuth { access_token: ent.access_token, token: ent.token, subject: ent.subject, version })
}

async fn latest_match_id(public: &Client, auth: &RiotAuth, shard: &str) -> Result<String> {
    let url = format!(
        "https://pd.{shard}.a.pvp.net/match-history/v1/history/{}?startIndex=0&endIndex=1",
        auth.subject
    );
    let v = riot_json(riot_headers(public.get(url), auth)).await?;
    v["History"][0]["MatchID"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("No recent match found on {shard} shard"))
}

async fn fetch_match_context(
    public: &Client,
    auth: &RiotAuth,
    shard: &str,
    match_id: &str,
) -> Result<MatchContext> {
    let url = format!("https://pd.{shard}.a.pvp.net/match-details/v1/matches/{match_id}");
    let details = riot_json(riot_headers(public.get(url), auth)).await?;

    let map_name = map_name_from_id(details["matchInfo"]["mapId"].as_str().unwrap_or(""));
    let queue_name = queue_name_from_id(details["matchInfo"]["queueID"].as_str().unwrap_or(""));

    let mut player_meta: HashMap<String, (String, String)> = HashMap::new();
    let mut puuids: Vec<String> = Vec::new();
    if let Some(arr) = details["players"].as_array() {
        for p in arr {
            if let Some(subj) = p["subject"].as_str() {
                let team = p["teamId"].as_str().unwrap_or("").to_string();
                let agent = agent_name_from_id(p["characterId"].as_str().unwrap_or(""));
                player_meta.insert(subj.to_string(), (team, agent));
                puuids.push(subj.to_string());
            }
        }
    }
    if puuids.is_empty() {
        return Err(anyhow!("Match details contained no players yet"));
    }

    let my_team = player_meta.get(&auth.subject).map(|(t, _)| t.clone()).unwrap_or_default();

    let names_val = riot_json(riot_headers(
        public.put(format!("https://pd.{shard}.a.pvp.net/name-service/v2/players")).json(&puuids),
        auth,
    ))
    .await?;
    let records: Vec<RiotNameRecord> =
        serde_json::from_value(names_val).context("Unexpected name-service response")?;

    let players = records
        .into_iter()
        .map(|rec| {
            let (team, agent) = player_meta.get(&rec.subject).cloned().unwrap_or_default();
            let team_relation = if !my_team.is_empty() && !team.is_empty() && team != "Neutral" {
                if team == my_team { "Ally".to_string() } else { "Enemy".to_string() }
            } else {
                String::new()
            };
            MatchPlayerInfo { record: rec, team_relation, agent_name: agent }
        })
        .collect();

    Ok(MatchContext {
        match_id: match_id.to_string(),
        map_name,
        queue_name,
        players,
    })
}

/// For post-match scans the latest MatchID may still be the *previous* match while history
/// propagates, so poll until it changes instead of trusting a fixed sleep.
async fn fetch_match_and_names(
    local: &Client,
    public: &Client,
    shard: &str,
    previous: Option<&str>,
    wait_for_new: bool,
) -> Result<(String, MatchContext)> {
    let auth = riot_authenticate(local, public).await?;
    let attempts = if wait_for_new { 6 } else { 2 };
    let match_id = retry(attempts, Duration::from_secs(3), || async {
        let id = latest_match_id(public, &auth, shard).await?;
        if wait_for_new && Some(id.as_str()) == previous {
            return Err(anyhow!("Match history has not updated yet"));
        }
        Ok(id)
    })
    .await?;
    let ctx = retry(3, Duration::from_secs(3), || {
        fetch_match_context(public, &auth, shard, &match_id)
    })
    .await?;
    Ok((auth.subject, ctx))
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
static OG_TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<meta\s+property=["']og:title["']\s+content=["']([^"']+)["']"#).unwrap());
static OG_TITLE_REV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<meta\s+content=["']([^"']+)["']\s+property=["']og:title["']"#).unwrap());
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

async fn http_get_text_status(client: &Client, url: &str) -> Result<(reqwest::StatusCode, String)> {
    let resp = client
        .get(url)
        .header("User-Agent", BROWSER_UA)
        .header("Accept-Language", "en-IN,en;q=0.9")
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    Ok((status, body))
}

async fn http_get_text(client: &Client, url: &str) -> Option<String> {
    let (status, body) = http_get_text_status(client, url).await.ok()?;
    if !status.is_success() {
        return None;
    }
    Some(body)
}

async fn test_youtube_api_key(client: &Client, api_key: &str) -> CheckState {
    let key = api_key.trim();
    if key.is_empty() {
        return CheckState::Err("Paste a YouTube Data API v3 key first (or leave blank to run keyless).".into());
    }
    match client
        .get("https://www.googleapis.com/youtube/v3/search")
        .query(&[
            ("part", "snippet"),
            ("q", "Valorant"),
            ("type", "video"),
            ("maxResults", "1"),
            ("key", key),
        ])
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                CheckState::Ok("Valid! YouTube Data API v3 key is active.".into())
            } else {
                let reason = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(String::from))
                    .unwrap_or_else(|| "Check that YouTube Data API v3 is enabled in Google Cloud".into());
                CheckState::Err(format!("Rejected (HTTP {}): {}", status.as_u16(), short(&reason, 85)))
            }
        }
        Err(e) => CheckState::Err(format!("Network error: {}", short(&e.to_string(), 80))),
    }
}

async fn test_twitch_api_keys(client: &Client, client_id: &str, client_secret: &str) -> CheckState {
    let cid = client_id.trim();
    let sec = client_secret.trim();
    if cid.is_empty() || sec.is_empty() {
        return CheckState::Err("Enter both Twitch Client ID and Client Secret first.".into());
    }
    match client
        .post("https://id.twitch.tv/oauth2/token")
        .form(&[("client_id", cid), ("client_secret", sec), ("grant_type", "client_credentials")])
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(tok) = v["access_token"].as_str() {
                        let ttl = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(300);
                        *lock(&TWITCH_TOKEN) = Some((tok.to_string(), Instant::now() + Duration::from_secs(ttl)));
                    }
                }
                CheckState::Ok("Valid! Twitch Helix OAuth credentials verified.".into())
            } else {
                let reason = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["message"].as_str().map(String::from))
                    .unwrap_or_else(|| "Invalid Client ID or Client Secret".into());
                CheckState::Err(format!("Rejected (HTTP {}): {}", status.as_u16(), short(&reason, 80)))
            }
        }
        Err(e) => CheckState::Err(format!("Network error: {}", short(&e.to_string(), 80))),
    }
}

fn names_roughly_match(entered: &str, detected: &str) -> bool {
    let a = normalize(entered);
    let b = normalize(detected);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

async fn verify_streamer_entry(
    client: &Client,
    riot_id: &str,
    platform: &str,
    channel_name: &str,
    channel_url: &str,
) -> CheckState {
    let rid = riot_id.trim();
    let cname = channel_name.trim();
    let curl = channel_url.trim();

    // 1. Validate Riot ID format
    let Some((name, tag)) = rid.split_once('#') else {
        return CheckState::Err("Riot ID must include '#' (example: RohanYT#IND).".into());
    };
    let name_len = name.trim().chars().count();
    if !(3..=16).contains(&name_len) || !(3..=5).contains(&tag.len()) || !tag.chars().all(|c| c.is_alphanumeric()) {
        return CheckState::Err("Riot ID looks invalid (name 3-16 chars, tag 3-5 letters/digits).".into());
    }

    // 2. Validate URL host & detect wrong platform selection
    let Some(host) = host_of(curl) else {
        return CheckState::Err("Channel URL must start with https:// and be a valid link.".into());
    };
    if !host_allowed(platform, &host) {
        for other in ["YouTube", "Twitch", "Kick"] {
            if other != platform && host_allowed(other, &host) {
                return CheckState::Err(format!(
                    "URL belongs to {other}, not {platform}. Switch Platform to {other}."
                ));
            }
        }
        return CheckState::Err(format!("URL host '{host}' is not a valid {platform} domain."));
    }

    // 3. Live reachability + real channel name lookup
    match platform {
        "YouTube" => match http_get_text_status(client, curl).await {
            Ok((status, html)) => {
                if status == reqwest::StatusCode::NOT_FOUND {
                    return CheckState::Err("YouTube channel not found (HTTP 404). Check the handle/URL.".into());
                }
                if !status.is_success() {
                    return CheckState::Err(format!("YouTube returned HTTP {}. Check the channel link.", status.as_u16()));
                }
                let detected = OG_TITLE_RE
                    .captures(&html)
                    .or_else(|| OG_TITLE_REV_RE.captures(&html))
                    .or_else(|| TITLE_RE.captures(&html))
                    .map(|c| decode_entities(c[1].trim()))
                    .unwrap_or_default();

                if detected.is_empty() || detected.eq_ignore_ascii_case("YouTube") {
                    return CheckState::Warn {
                        msg: "Page loaded, but could not confirm channel name. Click 'Open Link' to verify.".into(),
                        suggested_name: None,
                    };
                }
                let live_note = if html.contains(r#""isLive":true"#) { " (LIVE now!)" } else { "" };
                if cname.is_empty() {
                    CheckState::Warn {
                        msg: format!("Found YouTube channel '{detected}'{live_note}. Click 'Use detected name' to fill it."),
                        suggested_name: Some(short(&detected, 64)),
                    }
                } else if names_roughly_match(cname, &detected) {
                    CheckState::Ok(format!("Verified! Riot ID valid & YouTube channel '{detected}' matched{live_note}."))
                } else {
                    CheckState::Warn {
                        msg: format!("Link works, but channel is named '{detected}'{live_note} (you entered '{cname}')."),
                        suggested_name: Some(short(&detected, 64)),
                    }
                }
            }
            Err(e) => CheckState::Err(format!("Could not reach YouTube: {}", short(&e.to_string(), 75))),
        },
        "Twitch" => {
            let login = last_path_segment(curl).to_lowercase();
            if login.len() < 3 || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return CheckState::Err("Twitch URL must end with a channel name (e.g. https://www.twitch.tv/tenz).".into());
            }
            let body = serde_json::json!({
                "query": "query($login: String!) { user(login: $login) { login displayName stream { title } } }",
                "variables": { "login": login }
            });
            match client
                .post("https://gql.twitch.tv/gql")
                .header("Client-Id", "kimne78kx3ncx6brgo4mv6wki5h1ko")
                .json(&body)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let v: Value = resp.json().await.unwrap_or(Value::Null);
                    let user = &v["data"]["user"];
                    if !user.is_object() {
                        return CheckState::Err(format!("Twitch user '{login}' does not exist. Check spelling."));
                    }
                    let display = user["displayName"].as_str().unwrap_or(&login).to_string();
                    let live_note = if user["stream"].is_object() { " (LIVE now!)" } else { "" };
                    if cname.is_empty() {
                        CheckState::Warn {
                            msg: format!("Found Twitch channel '{display}'{live_note}. Click 'Use detected name' to fill it."),
                            suggested_name: Some(short(&display, 64)),
                        }
                    } else if names_roughly_match(cname, &display) {
                        CheckState::Ok(format!("Verified! Riot ID valid & Twitch channel '{display}' matched{live_note}."))
                    } else {
                        CheckState::Warn {
                            msg: format!("Link works, but Twitch channel is '{display}'{live_note} (you entered '{cname}')."),
                            suggested_name: Some(short(&display, 64)),
                        }
                    }
                }
                Ok(resp) => CheckState::Warn {
                    msg: format!("URL format valid, but Twitch check returned HTTP {}. Click 'Open Link' to verify.", resp.status().as_u16()),
                    suggested_name: None,
                },
                Err(e) => CheckState::Err(format!("Could not reach Twitch: {}", short(&e.to_string(), 75))),
            }
        }
        "Kick" => {
            let slug = last_path_segment(curl).to_lowercase();
            if slug.len() < 3 || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                return CheckState::Err("Kick URL must end with a channel slug (e.g. https://kick.com/kairoclutch).".into());
            }
            match client
                .get(format!("https://kick.com/api/v2/channels/{slug}"))
                .header("User-Agent", BROWSER_UA)
                .header("Accept", "application/json")
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let v: Value = resp.json().await.unwrap_or(Value::Null);
                    let display = v["user"]["username"].as_str().unwrap_or(&slug).to_string();
                    let live_note = if v["livestream"].is_object() { " (LIVE now!)" } else { "" };
                    if cname.is_empty() {
                        CheckState::Warn {
                            msg: format!("Found Kick channel '{display}'{live_note}. Click 'Use detected name' to fill it."),
                            suggested_name: Some(short(&display, 64)),
                        }
                    } else if names_roughly_match(cname, &display) {
                        CheckState::Ok(format!("Verified! Riot ID valid & Kick channel '{display}' matched{live_note}."))
                    } else {
                        CheckState::Warn {
                            msg: format!("Link works, but Kick channel is '{display}'{live_note} (you entered '{cname}')."),
                            suggested_name: Some(short(&display, 64)),
                        }
                    }
                }
                Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
                    CheckState::Err(format!("Kick channel '{slug}' not found (HTTP 404)."))
                }
                Ok(resp) if resp.status() == reqwest::StatusCode::FORBIDDEN => CheckState::Warn {
                    msg: "Format valid, but Kick Cloudflare blocked auto-check. Click 'Open Link' to confirm.".into(),
                    suggested_name: None,
                },
                Ok(resp) => CheckState::Warn {
                    msg: format!("URL format valid, Kick returned HTTP {}. Click 'Open Link' to confirm.", resp.status().as_u16()),
                    suggested_name: None,
                },
                Err(e) => CheckState::Err(format!("Could not reach Kick: {}", short(&e.to_string(), 75))),
            }
        }
        _ => CheckState::Err("Unsupported platform selected.".into()),
    }
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

async fn fetch_remote_directory(public: &Client, owner: &str, repo: &str) -> (Vec<DirectoryStreamer>, bool) {
    if !valid_github_segment(owner) || !valid_github_segment(repo) {
        return (load_json::<Vec<DirectoryStreamer>>("directory_cache.json"), false);
    }
    let gh_url = format!(
        "https://raw.githubusercontent.com/{}/{}/main/streamers.json",
        owner.trim(),
        repo.trim()
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

    match remote {
        Some(list) => {
            let clean: Vec<DirectoryStreamer> = list.iter().filter_map(sanitize_directory_entry).collect();
            save_json("directory_cache.json", &clean);
            (clean, true)
        }
        None => {
            log("GitHub directory unavailable; using cached copy");
            let cached: Vec<DirectoryStreamer> = load_json::<Vec<DirectoryStreamer>>("directory_cache.json")
                .iter()
                .filter_map(sanitize_directory_entry)
                .collect();
            (cached, false)
        }
    }
}

fn merge_directories(local: &[DirectoryStreamer], remote: &[DirectoryStreamer]) -> Vec<DirectoryStreamer> {
    let mut seen = HashSet::new();
    local
        .iter()
        .chain(remote.iter())
        .filter_map(sanitize_directory_entry)
        .filter(|d| seen.insert((d.riot_id.to_lowercase(), d.platform.clone(), d.channel_url.to_lowercase())))
        .collect()
}

#[cfg(windows)]
fn register_toast_app_id(enable: bool) {
    if !enable {
        CUSTOM_APP_ID_READY.store(false, Ordering::Relaxed);
        return;
    }
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let subkey = format!(r"Software\Classes\AppUserModelId\{CUSTOM_TOAST_APP_ID}");
    match hkcu.create_subkey(&subkey) {
        Ok((key, _)) => {
            let _ = key.set_value("DisplayName", &"Valorant Streamer Radar");
            let _ = key.set_value("IconBackgroundColor", &"FF0F1923");
            if let Some(icon_path) = ensure_toast_icon_file() {
                let _ = key.set_value("IconUri", &icon_path.to_string_lossy().to_string());
            }
            CUSTOM_APP_ID_READY.store(true, Ordering::Relaxed);
        }
        Err(e) => {
            log(format!("AppUserModelID registration failed (falling back to PowerShell ID): {e}"));
            CUSTOM_APP_ID_READY.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(not(windows))]
fn register_toast_app_id(_enable: bool) {}

#[cfg(windows)]
fn show_toast(entry: &StreamerEntry, use_custom_app_id: bool) {
    use tauri_winrt_notification::{Duration as ToastDuration, Toast};
    let url = entry.url.clone();
    let relation = if entry.team_relation.is_empty() {
        String::new()
    } else {
        format!(" ({})", entry.team_relation)
    };
    let title = format!("[{}] LIVE: {}{}", entry.platform, entry.riot_id, relation);
    let line1 = format!("{} ({})", short(&entry.title, 90), entry.channel);
    let app_id = if use_custom_app_id && CUSTOM_APP_ID_READY.load(Ordering::Relaxed) {
        CUSTOM_TOAST_APP_ID
    } else {
        Toast::POWERSHELL_APP_ID
    };
    let result = Toast::new(app_id)
        .title(&title)
        .text1(&line1)
        .text2("Click to open stream | link copied to clipboard")
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
fn show_toast(entry: &StreamerEntry, _use_custom_app_id: bool) {
    log(format!("LIVE: {} -> {}", entry.riot_id, entry.url));
}

/// Records new detections (deduplicated by match_id + riot_id + platform), copies first link, and fires toast.
fn record_and_notify(
    state: &Arc<Mutex<AppState>>,
    ctx: &egui::Context,
    entry: StreamerEntry,
    copy: bool,
    use_custom_app_id: bool,
) {
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
        show_toast(&entry, use_custom_app_id);
    }
}

async fn scan_player(
    client: Client,
    config: AppConfig,
    directory: Arc<Vec<DirectoryStreamer>>,
    twitch_token: Option<String>,
    match_id: String,
    map_name: String,
    queue_name: String,
    player_info: MatchPlayerInfo,
) -> Vec<StreamerEntry> {
    let player = player_info.record;
    let riot_id = format!("{}#{}", player.game_name, player.tag_line);
    let timestamp = Local::now().format("%d %b %Y | %I:%M %p").to_string();
    let team_relation = player_info.team_relation;
    let agent_name = player_info.agent_name;

    let make = |platform: &str, channel: String, title: String, url: String| StreamerEntry {
        match_id: match_id.clone(),
        riot_id: riot_id.clone(),
        platform: platform.to_string(),
        channel,
        title,
        url,
        timestamp: timestamp.clone(),
        team_relation: team_relation.clone(),
        map_name: map_name.clone(),
        agent_name: agent_name.clone(),
        queue_name: queue_name.clone(),
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

async fn sync_directory_only(public_client: Client, state: Arc<Mutex<AppState>>, ctx: egui::Context) {
    let (owner, repo) = {
        let mut s = lock(&state);
        s.is_syncing_directory = true;
        (s.config.github_owner.clone(), s.config.github_repo.clone())
    };
    repaint(&ctx);

    let (remote, ok) = fetch_remote_directory(&public_client, &owner, &repo).await;
    {
        let mut s = lock(&state);
        s.remote_directory = remote;
        s.is_syncing_directory = false;
    }
    if ok {
        set_toast(&state, format!("Synced directory from {owner}/{repo}"));
    } else {
        set_toast(&state, "GitHub unreachable - using cached directory");
    }
    repaint(&ctx);
}

async fn scan_last_match(
    local_client: Client,
    public_client: Client,
    state: Arc<Mutex<AppState>>,
    ctx: egui::Context,
    trigger: ScanTrigger,
) {
    let (config, detected_shard, previous) = {
        let s = lock(&state);
        (s.config.clone(), s.detected_shard.clone(), s.last_scanned_match.clone())
    };
    let shard = effective_shard(&config, &detected_shard);
    set_status(
        &state,
        &ctx,
        format!("Syncing directory & scanning last match ({})...", shard_label(shard)),
    );

    let fetched = fetch_match_and_names(
        &local_client,
        &public_client,
        shard,
        previous.as_deref(),
        trigger == ScanTrigger::PostMatch,
    )
    .await;

    let (my_puuid, match_ctx) = match fetched {
        Ok(v) => v,
        Err(e) => {
            log(format!("Match fetch failed on {shard}: {e:#}"));
            set_status(&state, &ctx, format!("Could not fetch match: {}", short(&e.to_string(), 110)));
            return;
        }
    };
    let MatchContext { match_id, map_name, queue_name, players } = match_ctx;
    lock(&state).last_scanned_match = Some(match_id.clone());

    let (remote, _) = fetch_remote_directory(&public_client, &config.github_owner, &config.github_repo).await;
    let local_dir = {
        let mut s = lock(&state);
        s.remote_directory = remote.clone();
        s.local_directory.clone()
    };
    let directory = Arc::new(merge_directories(&local_dir, &remote));
    let twitch_token =
        get_twitch_app_token(&public_client, &config.twitch_client_id, &config.twitch_client_secret).await;

    let sem = Arc::new(Semaphore::new(4));
    let mut set = JoinSet::new();
    let mut seen = HashSet::new();
    for player_info in players {
        if player_info.record.subject == my_puuid
            || player_info.record.game_name.trim().is_empty()
            || !seen.insert(player_info.record.subject.clone())
        {
            continue;
        }
        let (client, cfg, dir, tok, mid, map_n, queue_n, sem) = (
            public_client.clone(),
            config.clone(),
            directory.clone(),
            twitch_token.clone(),
            match_id.clone(),
            map_name.clone(),
            queue_name.clone(),
            sem.clone(),
        );
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            scan_player(client, cfg, dir, tok, mid, map_n, queue_n, player_info).await
        });
    }

    let mut found = 0usize;
    while let Some(res) = set.join_next().await {
        match res {
            Ok(entries) => {
                for entry in entries {
                    record_and_notify(&state, &ctx, entry, found == 0, config.use_custom_toast_app_id);
                    found += 1;
                }
            }
            Err(e) => log(format!("Player scan task failed: {e}")),
        }
    }

    let status = if found > 0 {
        format!("Found {found} live stream(s) in last match!")
    } else {
        format!("Last checked at {} | No live streamers found", Local::now().format("%I:%M %p"))
    };
    set_status(&state, &ctx, status);
}

// ============================================================================
// LOG WATCHER
// ============================================================================

static SHARD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https://pd\.([a-z0-9]+)\.a\.pvp\.net").unwrap());

/// Reads only *complete* lines appended after `start`. A half-written trailing line is left for
/// the next poll. Also extracts any Riot PD shard endpoint seen in the log.
fn read_new_log_events(path: &Path, start: u64) -> std::io::Result<(Vec<String>, Option<String>, u64)> {
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
    let text = String::from_utf8_lossy(&buf[..end]);
    let mut reconcile_lines = Vec::new();
    let mut detected_shard = None;

    for line in text.lines() {
        if line.contains("Reconcile") {
            reconcile_lines.push(line.to_string());
        }
        if line.contains(".a.pvp.net") {
            if let Some(caps) = SHARD_RE.captures(line) {
                if let Some(s) = canonical_shard(&caps[1]) {
                    detected_shard = Some(s.to_string());
                }
            }
        }
    }
    Ok((reconcile_lines, detected_shard, start + end as u64))
}

/// Scans the head of ShooterGame.log once at startup so we know the shard even before a new line lands.
fn detect_initial_shard(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(256 * 1024).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    for caps in SHARD_RE.captures_iter(&text) {
        if let Some(s) = canonical_shard(&caps[1]) {
            return Some(s.to_string());
        }
    }
    None
}

async fn watch_log(
    log_path: PathBuf,
    local_client: Client,
    state: Arc<Mutex<AppState>>,
    ctx: egui::Context,
    tx: mpsc::UnboundedSender<ScanTrigger>,
) {
    let mut in_match = false;
    if let Some(shard) = detect_initial_shard(&log_path) {
        lock(&state).detected_shard = shard;
    }

    let initial = fs::metadata(&log_path).ok();
    let mut last_pos = initial.as_ref().map(|m| m.len()).unwrap_or(0);
    let mut last_created = initial.and_then(|m| m.created().ok());
    let mut last_riot_status: Option<RiotStatus> = None;
    let mut last_probe: Option<Instant> = None;

    loop {
        let has_lockfile = read_riot_lockfile().is_ok();
        let should_probe = match (has_lockfile, last_riot_status, last_probe) {
            (false, _, _) => false,
            (true, None, _) | (true, Some(RiotStatus::Offline), _) => true,
            (true, _, Some(at)) => at.elapsed() >= Duration::from_secs(5),
            (true, _, None) => true,
        };
        let next_status = if !has_lockfile {
            RiotStatus::Offline
        } else if should_probe {
            last_probe = Some(Instant::now());
            probe_riot_session(&local_client).await
        } else {
            last_riot_status.unwrap_or(RiotStatus::RiotClientOnly)
        };
        if last_riot_status != Some(next_status) {
            last_riot_status = Some(next_status);
            lock(&state).riot_status = next_status;
            repaint(&ctx);
        }

        let Ok(meta) = fs::metadata(&log_path) else {
            last_pos = 0;
            last_created = None;
            in_match = false;
            sleep(Duration::from_secs(5)).await;
            continue;
        };

        let created = meta.created().ok();
        let len = meta.len();
        if len < last_pos || (created.is_some() && created != last_created) {
            last_pos = 0;
            in_match = false;
        }
        last_created = created;

        if len > last_pos {
            match read_new_log_events(&log_path, last_pos) {
                Ok((lines, shard_opt, new_pos)) => {
                    last_pos = new_pos;
                    if let Some(shard) = shard_opt {
                        let mut s = lock(&state);
                        if s.detected_shard != shard {
                            s.detected_shard = shard;
                            drop(s);
                            repaint(&ctx);
                        }
                    }
                    for line in lines {
                        if line.contains("InGame") {
                            if !in_match {
                                in_match = true;
                                set_status(&state, &ctx, "In Match | Waiting for match end...");
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
    Icon::from_rgba(radar_icon_rgba(), 32, 32).expect("Failed to create tray icon")
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
            // Launch silently in tray at Windows logon; manual double-clicks open the UI directly.
            let _ = key.set_value("ValorantStreamerRadar", &format!("\"{}\" --background", exe.display()));
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
    // Follow-up repaint ensures NVIDIA G-SYNC / OpenGL redraws once DWM maps the unhidden window.
    ctx.request_repaint_after(Duration::from_millis(80));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    History,
    Directory,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeamFilter {
    All,
    Ally,
    Enemy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectorySourceTab {
    Custom,
    Community,
}

/// Crisp vector-painted icons (avoids missing Unicode glyphs in egui's bundled font).
fn paint_status_dot(ui: &mut egui::Ui, color: Color32, glowing: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
    let c = rect.center();
    if glowing {
        ui.painter().circle_filled(c, 5.5, color.linear_multiply(0.28));
    }
    ui.painter().circle_filled(c, 3.6, color);
}

fn paint_check_icon(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
    let p = ui.painter();
    let stroke = Stroke::new(1.8, color);
    let a = egui::pos2(rect.left() + 2.5, rect.center().y + 0.5);
    let b = egui::pos2(rect.left() + 5.5, rect.bottom() - 3.0);
    let c = egui::pos2(rect.right() - 2.0, rect.top() + 3.0);
    p.line_segment([a, b], stroke);
    p.line_segment([b, c], stroke);
}

fn paint_cross_icon(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
    let p = ui.painter();
    let stroke = Stroke::new(1.8, color);
    let r = rect.shrink(3.0);
    p.line_segment([r.left_top(), r.right_bottom()], stroke);
    p.line_segment([r.right_top(), r.left_bottom()], stroke);
}

fn paint_warn_icon(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
    let p = ui.painter();
    let stroke = Stroke::new(1.5, color);
    let top = egui::pos2(rect.center().x, rect.top() + 1.5);
    let bl = egui::pos2(rect.left() + 1.5, rect.bottom() - 2.0);
    let br = egui::pos2(rect.right() - 1.5, rect.bottom() - 2.0);
    p.line_segment([top, br], stroke);
    p.line_segment([br, bl], stroke);
    p.line_segment([bl, top], stroke);
    p.circle_filled(egui::pos2(rect.center().x, rect.bottom() - 4.5), 1.0, color);
}

fn render_check_banner(ui: &mut egui::Ui, state: &CheckState) -> Option<String> {
    let mut apply_name: Option<String> = None;
    match state {
        CheckState::Idle => {}
        CheckState::Checking => {
            ui.horizontal(|ui| {
                paint_status_dot(ui, ENEMY_AMBER, true);
                ui.label(RichText::new("Checking live credentials...").color(TEXT_MUTED).size(12.0));
            });
        }
        CheckState::Ok(msg) => {
            Frame::none()
                .fill(LIVE_GREEN.linear_multiply(0.12))
                .stroke(Stroke::new(1.0, LIVE_GREEN.linear_multiply(0.55)))
                .rounding(Rounding::same(5.0))
                .inner_margin(Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        paint_check_icon(ui, LIVE_GREEN);
                        ui.label(RichText::new(msg).color(LIVE_GREEN).size(12.0));
                    });
                });
        }
        CheckState::Warn { msg, suggested_name } => {
            Frame::none()
                .fill(ENEMY_AMBER.linear_multiply(0.12))
                .stroke(Stroke::new(1.0, ENEMY_AMBER.linear_multiply(0.55)))
                .rounding(Rounding::same(5.0))
                .inner_margin(Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        paint_warn_icon(ui, ENEMY_AMBER);
                        ui.label(RichText::new(msg).color(ENEMY_AMBER).size(12.0));
                    });
                    if let Some(name) = suggested_name {
                        ui.add_space(5.0);
                        let use_btn = egui::Button::new(
                            RichText::new(format!("Use detected name: {name}")).color(Color32::WHITE).strong().size(11.5),
                        )
                        .fill(BTN_SECONDARY)
                        .stroke(Stroke::new(1.0, ENEMY_AMBER))
                        .rounding(Rounding::same(4.0));
                        if ui.add(use_btn).clicked() {
                            apply_name = Some(name.clone());
                        }
                    }
                });
        }
        CheckState::Err(msg) => {
            Frame::none()
                .fill(ACCENT_RED.linear_multiply(0.12))
                .stroke(Stroke::new(1.0, ACCENT_RED.linear_multiply(0.6)))
                .rounding(Rounding::same(5.0))
                .inner_margin(Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        paint_cross_icon(ui, ACCENT_RED);
                        ui.label(RichText::new(msg).color(ACCENT_RED).size(12.0));
                    });
                });
        }
    }
    apply_name
}

struct StreamerApp {
    state: Arc<Mutex<AppState>>,
    manual_scan_tx: mpsc::UnboundedSender<ScanTrigger>,
    bg_tx: mpsc::UnboundedSender<BgTask>,
    pending_initial_hide: bool,
    active_tab: ActiveTab,
    // History filters
    search_query: String,
    team_filter: TeamFilter,
    // Directory manager state
    dir_source_tab: DirectorySourceTab,
    dir_search: String,
    // Add / Edit modal state
    show_add_modal: bool,
    editing_original: Option<(String, String)>,
    input_riot_id: String,
    input_platform: String,
    input_channel_name: String,
    input_channel_url: String,
    modal_error: Option<String>,
    // Settings draft state
    cfg_draft: AppConfig,
    show_api_secrets: bool,
    settings_error: Option<String>,
}

impl StreamerApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        state: Arc<Mutex<AppState>>,
        tx: mpsc::UnboundedSender<ScanTrigger>,
        bg_tx: mpsc::UnboundedSender<BgTask>,
        start_hidden: bool,
    ) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG_DARK;
        visuals.window_fill = CARD_BG;
        visuals.widgets.noninteractive.bg_fill = CARD_BG;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 44, 58);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 60, 78);
        visuals.selection.bg_fill = ACCENT_RED;
        cc.egui_ctx.set_visuals(visuals);

        let initial_cfg = lock(&state).config.clone();

        Self {
            state,
            manual_scan_tx: tx,
            bg_tx,
            pending_initial_hide: start_hidden,
            active_tab: ActiveTab::History,
            search_query: String::new(),
            team_filter: TeamFilter::All,
            dir_source_tab: DirectorySourceTab::Custom,
            dir_search: String::new(),
            show_add_modal: false,
            editing_original: None,
            input_riot_id: String::new(),
            input_platform: "YouTube".to_string(),
            input_channel_name: String::new(),
            input_channel_url: String::new(),
            modal_error: None,
            cfg_draft: initial_cfg,
            show_api_secrets: false,
            settings_error: None,
        }
    }

    fn open_add_modal(&mut self, edit: Option<&DirectoryStreamer>) {
        if let Some(d) = edit {
            self.editing_original = Some((d.riot_id.clone(), d.platform.clone()));
            self.input_riot_id = d.riot_id.clone();
            self.input_platform = d.platform.clone();
            self.input_channel_name = d.channel_name.clone();
            self.input_channel_url = d.channel_url.clone();
        } else {
            self.editing_original = None;
            self.input_riot_id.clear();
            self.input_platform = "YouTube".to_string();
            self.input_channel_name.clear();
            self.input_channel_url.clear();
        }
        self.modal_error = None;
        lock(&self.state).streamer_verify = CheckState::Idle;
        self.show_add_modal = true;
    }

    fn submit_modal(&mut self, open_github: bool) {
        let rid = self.input_riot_id.trim().to_string();
        let cname = self.input_channel_name.trim().to_string();
        let curl = self.input_channel_url.trim().to_string();
        let platform = self.input_platform.clone();

        if let Err(msg) = validate_submission(&rid, &platform, &cname, &curl) {
            self.modal_error = Some(msg);
            return;
        }

        let new_entry = DirectoryStreamer {
            riot_id: rid.clone(),
            platform: platform.clone(),
            channel_name: cname,
            channel_url: curl,
        };
        let (gh_owner, gh_repo) = {
            let mut s = lock(&self.state);
            if let Some((orig_rid, orig_plat)) = self.editing_original.take() {
                s.local_directory
                    .retain(|x| !(x.riot_id.eq_ignore_ascii_case(&orig_rid) && x.platform == orig_plat));
            }
            // Replace only the same Riot ID + platform (a player may stream on several platforms).
            s.local_directory
                .retain(|x| !(x.riot_id.eq_ignore_ascii_case(&rid) && x.platform == platform));
            s.local_directory.push(new_entry.clone());
            save_json("custom_streamers.json", &s.local_directory);
            (s.config.github_owner.clone(), s.config.github_repo.clone())
        };

        if open_github && valid_github_segment(&gh_owner) && valid_github_segment(&gh_repo) {
            set_toast(&self.state, format!("Saved {rid} locally & opened GitHub!"));
            let issue_title = format!("Add Streamer: {rid}");
            let json_payload = serde_json::to_string_pretty(&new_entry).unwrap_or_default();
            let issue_body = format!("### New Streamer Submission\n\n```json\n{json_payload}\n```");
            let gh_issue_url = format!(
                "https://github.com/{}/{}/issues/new?title={}&body={}",
                gh_owner.trim(),
                gh_repo.trim(),
                urlencode(&issue_title),
                urlencode(&issue_body)
            );
            let _ = open::that(gh_issue_url);
        } else {
            set_toast(&self.state, format!("Saved {rid} to custom_streamers.json"));
        }

        self.input_riot_id.clear();
        self.input_channel_name.clear();
        self.input_channel_url.clear();
        self.modal_error = None;
        self.show_add_modal = false;
    }

    fn save_settings(&mut self) {
        let owner = self.cfg_draft.github_owner.trim().to_string();
        let repo = self.cfg_draft.github_repo.trim().to_string();
        if !valid_github_segment(&owner) || !valid_github_segment(&repo) {
            self.settings_error = Some("Enter a valid GitHub owner and repository name.".into());
            return;
        }
        self.cfg_draft.github_owner = owner;
        self.cfg_draft.github_repo = repo;
        self.cfg_draft.youtube_api_key = self.cfg_draft.youtube_api_key.trim().to_string();
        self.cfg_draft.twitch_client_id = self.cfg_draft.twitch_client_id.trim().to_string();
        self.cfg_draft.twitch_client_secret = self.cfg_draft.twitch_client_secret.trim().to_string();

        save_json("config.json", &self.cfg_draft);
        set_autostart(self.cfg_draft.start_with_windows);
        register_toast_app_id(self.cfg_draft.use_custom_toast_app_id);
        *lock(&TWITCH_TOKEN) = None; // reset cached token if Twitch keys changed
        {
            let mut s = lock(&self.state);
            s.config = self.cfg_draft.clone();
        }
        self.settings_error = None;
        set_toast(&self.state, "Settings saved to config.json!");
    }
}

impl eframe::App for StreamerApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [15.0 / 255.0, 25.0 / 255.0, 35.0 / 255.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if QUIT.load(Ordering::Relaxed) {
                return; // let the real quit through
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            WINDOW_VISIBLE.store(false, Ordering::Relaxed);
            return;
        }

        // eframe 0.29 may unhide the window after the first frame even with `.with_visible(false)`;
        // enforce hiding explicitly only when launched with `--background`.
        if self.pending_initial_hide {
            self.pending_initial_hide = false;
            WINDOW_VISIBLE.store(false, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }

        // Never render a blank black screen if the OS window is actually focused/visible.
        if !WINDOW_VISIBLE.load(Ordering::Relaxed) {
            if ctx.input(|i| i.viewport().focused == Some(true)) {
                WINDOW_VISIBLE.store(true, Ordering::Relaxed);
            } else {
                return;
            }
        }

        let (
            status,
            is_scanning,
            is_syncing_dir,
            riot_status,
            active_shard,
            history,
            local_dir,
            remote_dir,
            toast,
            streamer_verify,
            yt_key_check,
            twitch_key_check,
        ) = {
            let mut s = lock(&self.state);
            if matches!(&s.toast, Some((_, at)) if at.elapsed() > TOAST_TTL) {
                s.toast = None;
            }
            let shard = effective_shard(&s.config, &s.detected_shard);
            (
                s.status.clone(),
                s.is_scanning,
                s.is_syncing_directory,
                s.riot_status,
                shard,
                s.history.clone(),
                s.local_directory.clone(),
                s.remote_directory.clone(),
                s.toast.as_ref().map(|(m, _)| m.clone()),
                s.streamer_verify.clone(),
                s.yt_key_check.clone(),
                s.twitch_key_check.clone(),
            )
        };
        if toast.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        // Header
        egui::TopBottomPanel::top("header")
            .frame(
                Frame::none()
                    .fill(HEADER_BG)
                    .inner_margin(Margin::symmetric(22.0, 14.0))
                    .stroke(Stroke::new(1.0, CARD_BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("VALORANT").font(FontId::proportional(20.0)).strong().color(ACCENT_RED));
                            ui.label(RichText::new("STREAMER RADAR").font(FontId::proportional(20.0)).strong().color(TEXT_PRIMARY));
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let (dot, glowing, text) = match riot_status {
                                RiotStatus::ValorantActive => (
                                    LIVE_GREEN,
                                    true,
                                    format!("Valorant Active ({})", shard_label(active_shard)),
                                ),
                                RiotStatus::RiotClientOnly => (
                                    ENEMY_AMBER,
                                    true,
                                    format!("Riot Client Open - Launch Valorant ({})", shard_label(active_shard)),
                                ),
                                RiotStatus::Offline => (
                                    TEXT_MUTED,
                                    false,
                                    format!("Waiting for Valorant ({})", shard_label(active_shard)),
                                ),
                            };
                            paint_status_dot(ui, dot, glowing);
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
                            self.open_add_modal(None);
                        }
                    });
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    for (tab, title) in [
                        (ActiveTab::History, format!("History ({})", history.len())),
                        (ActiveTab::Directory, format!("Directory ({})", local_dir.len() + remote_dir.len())),
                        (ActiveTab::Settings, "Settings".to_string()),
                    ] {
                        let selected = self.active_tab == tab;
                        let btn = egui::Button::new(
                            RichText::new(title)
                                .size(12.5)
                                .strong()
                                .color(if selected { Color32::WHITE } else { TEXT_MUTED }),
                        )
                        .fill(if selected { BTN_SECONDARY } else { Color32::TRANSPARENT })
                        .stroke(Stroke::new(1.0, if selected { ACCENT_RED } else { Color32::TRANSPARENT }))
                        .rounding(Rounding::same(5.0))
                        .min_size(Vec2::new(96.0, 28.0));
                        if ui.add(btn).clicked() {
                            if tab == ActiveTab::Settings {
                                self.cfg_draft = lock(&self.state).config.clone();
                                self.settings_error = None;
                            }
                            self.active_tab = tab;
                        }
                        ui.add_space(4.0);
                    }

                    if let Some(msg) = &toast {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(msg).color(LIVE_GREEN).size(12.5));
                        });
                    }
                });
            });

        // Add / Edit streamer modal
        if self.show_add_modal {
            let is_edit = self.editing_original.is_some();
            let modal_title = if is_edit {
                "Edit Custom Streamer Entry"
            } else {
                "Register Streamer to Directory"
            };
            let (mut submit_gh, mut submit_local, mut cancel, mut trigger_verify) = (false, false, false, false);
            let mut apply_suggested_name: Option<String> = None;
            let mut changed_input = false;
            let is_verifying = matches!(streamer_verify, CheckState::Checking);

            egui::Window::new(modal_title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(460.0)
                .min_width(460.0)
                .max_width(460.0)
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
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.input_riot_id)
                                .hint_text("e.g. RohanYT#IND")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(8.0, 6.0)),
                        )
                        .changed()
                    {
                        changed_input = true;
                    }
                    ui.add_space(8.0);

                    ui.label(RichText::new("Platform").strong().color(TEXT_PRIMARY));
                    ui.horizontal(|ui| {
                        for plat in ["YouTube", "Twitch", "Kick"] {
                            if ui.selectable_value(&mut self.input_platform, plat.to_string(), plat).changed() {
                                changed_input = true;
                            }
                        }
                    });
                    ui.add_space(8.0);

                    ui.label(RichText::new("Channel Display Name (or leave blank & click Verify to auto-fill)").strong().color(TEXT_PRIMARY));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.input_channel_name)
                                .hint_text("e.g. Rohan Gaming")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(8.0, 6.0)),
                        )
                        .changed()
                    {
                        changed_input = true;
                    }
                    ui.add_space(8.0);

                    ui.label(RichText::new("Channel URL").strong().color(TEXT_PRIMARY));
                    let url_hint = match self.input_platform.as_str() {
                        "Twitch" => "e.g. https://www.twitch.tv/tenz",
                        "Kick" => "e.g. https://kick.com/kairoclutch",
                        _ => "e.g. https://www.youtube.com/@RohanGaming",
                    };
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.input_channel_url)
                                .hint_text(url_hint)
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(8.0, 6.0)),
                        )
                        .changed()
                    {
                        changed_input = true;
                    }

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let verify_label = if is_verifying { "Verifying..." } else { "Verify Link & Name" };
                        let verify_btn = egui::Button::new(RichText::new(verify_label).color(Color32::WHITE).strong().size(12.0))
                            .fill(BTN_SECONDARY)
                            .stroke(Stroke::new(1.0, ALLY_TEAL))
                            .rounding(Rounding::same(5.0))
                            .min_size(Vec2::new(140.0, 28.0));
                        if ui.add_enabled(!is_verifying, verify_btn).clicked() {
                            trigger_verify = true;
                        }

                        let can_open = host_of(self.input_channel_url.trim())
                            .map(|h| host_allowed(&self.input_platform, &h))
                            .unwrap_or(false);
                        let open_btn = egui::Button::new(RichText::new("Open Link").color(TEXT_PRIMARY).size(12.0))
                            .fill(BTN_SECONDARY)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .rounding(Rounding::same(5.0))
                            .min_size(Vec2::new(82.0, 28.0));
                        if ui.add_enabled(can_open, open_btn).clicked() {
                            let _ = open::that(self.input_channel_url.trim());
                        }
                    });

                    if !matches!(streamer_verify, CheckState::Idle) {
                        ui.add_space(8.0);
                        apply_suggested_name = render_check_banner(ui, &streamer_verify);
                    }

                    if let Some(err) = &self.modal_error {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            paint_cross_icon(ui, ACCENT_RED);
                            ui.label(RichText::new(err).color(ACCENT_RED).size(12.0));
                        });
                    }

                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let submit_btn = egui::Button::new(
                            RichText::new("Save & Submit to GitHub").color(Color32::WHITE).strong(),
                        )
                        .fill(ACCENT_RED)
                        .rounding(Rounding::same(6.0))
                        .min_size(Vec2::new(172.0, 34.0));
                        submit_gh = ui.add(submit_btn).clicked();

                        ui.add_space(6.0);
                        let local_btn = egui::Button::new(RichText::new("Save Local").color(TEXT_PRIMARY))
                            .fill(BTN_SECONDARY)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(92.0, 34.0));
                        submit_local = ui.add(local_btn).clicked();

                        ui.add_space(6.0);
                        cancel = ui
                            .add(egui::Button::new("Cancel").min_size(Vec2::new(72.0, 34.0)).rounding(Rounding::same(6.0)))
                            .clicked();
                    });
                });

            if changed_input {
                self.modal_error = None;
                lock(&self.state).streamer_verify = CheckState::Idle;
            }
            if let Some(name) = apply_suggested_name {
                self.input_channel_name = name.clone();
                self.modal_error = None;
                lock(&self.state).streamer_verify = CheckState::Ok(format!("Channel name updated to '{name}' and verified."));
            }
            if trigger_verify {
                self.modal_error = None;
                lock(&self.state).streamer_verify = CheckState::Checking;
                let _ = self.bg_tx.send(BgTask::VerifyStreamer {
                    riot_id: self.input_riot_id.clone(),
                    platform: self.input_platform.clone(),
                    channel_name: self.input_channel_name.clone(),
                    channel_url: self.input_channel_url.clone(),
                });
            }
            if submit_gh {
                self.submit_modal(true);
            } else if submit_local {
                self.submit_modal(false);
            } else if cancel {
                self.show_add_modal = false;
                self.editing_original = None;
                lock(&self.state).streamer_verify = CheckState::Idle;
            }
        }

        // Main panel
        egui::CentralPanel::default()
            .frame(Frame::none().fill(BG_DARK).inner_margin(Margin::symmetric(22.0, 16.0)))
            .show(ctx, |ui| match self.active_tab {
                ActiveTab::History => self.render_history_tab(ui, &history),
                ActiveTab::Directory => self.render_directory_tab(ui, &local_dir, &remote_dir, is_syncing_dir),
                ActiveTab::Settings => self.render_settings_tab(ui, &yt_key_check, &twitch_key_check),
            });
    }
}

impl StreamerApp {
    fn render_history_tab(&mut self, ui: &mut egui::Ui, history: &[StreamerEntry]) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Search Riot ID, platform, channel, map, or stream title...")
                    .desired_width(320.0)
                    .margin(Margin::symmetric(10.0, 7.0)),
            );
            ui.add_space(6.0);
            for (f, label) in [
                (TeamFilter::All, "All"),
                (TeamFilter::Ally, "Ally"),
                (TeamFilter::Enemy, "Enemy"),
            ] {
                ui.selectable_value(&mut self.team_filter, f, label);
            }
            ui.add_space(6.0);
            ui.label(RichText::new(format!("{} detected", history.len())).color(TEXT_MUTED).size(12.5));

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
                let team_ok = match self.team_filter {
                    TeamFilter::All => true,
                    TeamFilter::Ally => e.team_relation.eq_ignore_ascii_case("Ally"),
                    TeamFilter::Enemy => e.team_relation.eq_ignore_ascii_case("Enemy"),
                };
                team_ok
                    && (q.is_empty()
                        || e.riot_id.to_lowercase().contains(&q)
                        || e.platform.to_lowercase().contains(&q)
                        || e.channel.to_lowercase().contains(&q)
                        || e.title.to_lowercase().contains(&q)
                        || e.map_name.to_lowercase().contains(&q)
                        || e.agent_name.to_lowercase().contains(&q))
            })
            .collect();

        if filtered.is_empty() {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No Streamers Recorded Yet").font(FontId::proportional(18.0)).strong().color(TEXT_PRIMARY));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Play a match or click '+ Add Streamer' to register your Riot ID & channel.")
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
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(&entry.riot_id).font(FontId::proportional(16.0)).strong().color(Color32::WHITE));
                                    Frame::none()
                                        .fill(badge.linear_multiply(0.2))
                                        .stroke(Stroke::new(1.0, badge))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(Margin::symmetric(6.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(&entry.platform).size(11.0).strong().color(badge));
                                        });
                                    if !entry.team_relation.is_empty() {
                                        let tc = if entry.team_relation == "Ally" { ALLY_TEAL } else { ENEMY_AMBER };
                                        Frame::none()
                                            .fill(tc.linear_multiply(0.18))
                                            .stroke(Stroke::new(1.0, tc))
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(Margin::symmetric(6.0, 2.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(&entry.team_relation).size(11.0).strong().color(tc));
                                            });
                                    }
                                    ui.label(RichText::new(&entry.channel).font(FontId::proportional(13.5)).strong().color(badge));
                                });
                                ui.add_space(4.0);
                                ui.label(RichText::new(&entry.title).color(TEXT_PRIMARY).size(13.0));
                                ui.add_space(6.0);

                                let mut meta_parts: Vec<&str> = Vec::new();
                                if !entry.map_name.is_empty() {
                                    meta_parts.push(&entry.map_name);
                                }
                                if !entry.agent_name.is_empty() {
                                    meta_parts.push(&entry.agent_name);
                                }
                                if !entry.queue_name.is_empty() {
                                    meta_parts.push(&entry.queue_name);
                                }
                                meta_parts.push(&entry.timestamp);
                                ui.label(RichText::new(meta_parts.join("  |  ")).color(TEXT_MUTED).size(11.5));
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
    }

    fn render_directory_tab(
        &mut self,
        ui: &mut egui::Ui,
        local_dir: &[DirectoryStreamer],
        remote_dir: &[DirectoryStreamer],
        is_syncing: bool,
    ) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.dir_source_tab,
                DirectorySourceTab::Custom,
                format!("My Custom Streamers ({})", local_dir.len()),
            );
            ui.selectable_value(
                &mut self.dir_source_tab,
                DirectorySourceTab::Community,
                format!("Community Cache ({})", remote_dir.len()),
            );
            ui.add_space(10.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.dir_search)
                    .hint_text("Filter Riot ID or channel...")
                    .desired_width(220.0)
                    .margin(Margin::symmetric(8.0, 6.0)),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let sync_label = if is_syncing { "Syncing..." } else { "Sync GitHub" };
                let sync_btn = egui::Button::new(RichText::new(sync_label).color(TEXT_PRIMARY).size(12.0))
                    .fill(BTN_SECONDARY)
                    .stroke(Stroke::new(1.0, CARD_BORDER))
                    .rounding(Rounding::same(5.0))
                    .min_size(Vec2::new(98.0, 28.0));
                if ui.add_enabled(!is_syncing, sync_btn).clicked() {
                    let _ = self.manual_scan_tx.send(ScanTrigger::RefreshDirectory);
                }
            });
        });

        ui.add_space(12.0);
        let q = self.dir_search.to_lowercase();
        let list = match self.dir_source_tab {
            DirectorySourceTab::Custom => local_dir,
            DirectorySourceTab::Community => remote_dir,
        };
        let filtered: Vec<&DirectoryStreamer> = list
            .iter()
            .filter(|d| {
                q.is_empty()
                    || d.riot_id.to_lowercase().contains(&q)
                    || d.channel_name.to_lowercase().contains(&q)
                    || d.platform.to_lowercase().contains(&q)
            })
            .collect();

        if filtered.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                let msg = match self.dir_source_tab {
                    DirectorySourceTab::Custom => "No custom streamers saved yet. Click '+ Add Streamer' to add one.",
                    DirectorySourceTab::Community => "Community directory cache is empty. Click 'Sync GitHub' or check your repo in Settings.",
                };
                ui.label(RichText::new(msg).color(TEXT_MUTED).size(13.0));
            });
            return;
        }

        let mut to_edit: Option<DirectoryStreamer> = None;
        let mut to_delete: Option<(String, String)> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for d in filtered {
                let badge = match d.platform.as_str() {
                    "Twitch" => TWITCH_PURPLE,
                    "Kick" => KICK_GREEN,
                    _ => ACCENT_RED,
                };
                Frame::none()
                    .fill(CARD_BG)
                    .stroke(Stroke::new(1.0, CARD_BORDER))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&d.riot_id).strong().color(Color32::WHITE).size(14.5));
                                    ui.label(RichText::new(&d.platform).strong().color(badge).size(12.0));
                                    ui.label(RichText::new(&d.channel_name).color(TEXT_PRIMARY).size(13.0));
                                });
                                ui.label(RichText::new(&d.channel_url).color(TEXT_MUTED).size(11.5));
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if self.dir_source_tab == DirectorySourceTab::Custom {
                                    let del = egui::Button::new(RichText::new("Delete").color(ACCENT_RED).size(12.0))
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::new(1.0, CARD_BORDER))
                                        .rounding(Rounding::same(5.0));
                                    if ui.add(del).clicked() {
                                        to_delete = Some((d.riot_id.clone(), d.platform.clone()));
                                    }
                                    let edit = egui::Button::new(RichText::new("Edit").color(TEXT_PRIMARY).size(12.0))
                                        .fill(BTN_SECONDARY)
                                        .stroke(Stroke::new(1.0, CARD_BORDER))
                                        .rounding(Rounding::same(5.0));
                                    if ui.add(edit).clicked() {
                                        to_edit = Some(d.clone());
                                    }
                                }
                                let open_btn = egui::Button::new(RichText::new("Open").color(TEXT_PRIMARY).size(12.0))
                                    .fill(BTN_SECONDARY)
                                    .stroke(Stroke::new(1.0, CARD_BORDER))
                                    .rounding(Rounding::same(5.0));
                                if ui.add(open_btn).clicked() {
                                    let _ = open::that(&d.channel_url);
                                }
                            });
                        });
                    });
                ui.add_space(8.0);
            }
        });

        if let Some(item) = to_edit {
            self.open_add_modal(Some(&item));
        }
        if let Some((rid, plat)) = to_delete {
            let mut s = lock(&self.state);
            s.local_directory
                .retain(|x| !(x.riot_id.eq_ignore_ascii_case(&rid) && x.platform == plat));
            save_json("custom_streamers.json", &s.local_directory);
            drop(s);
            set_toast(&self.state, format!("Removed {rid} ({plat})"));
        }
    }

    fn render_settings_tab(&mut self, ui: &mut egui::Ui, yt_check: &CheckState, twitch_check: &CheckState) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            Frame::none()
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::same(18.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("Riot Shard & Community Directory").font(FontId::proportional(15.0)).strong().color(Color32::WHITE));
                    ui.add_space(10.0);

                    ui.label(RichText::new("Riot Shard").strong().color(TEXT_PRIMARY).size(12.5));
                    ui.horizontal_wrapped(|ui| {
                        for (code, label) in [
                            ("auto", "Auto-detect (ShooterGame.log)"),
                            ("ap", "AP (Mumbai)"),
                            ("na", "NA / LATAM / BR"),
                            ("eu", "EU (Europe)"),
                            ("kr", "KR (Korea)"),
                        ] {
                            ui.selectable_value(&mut self.cfg_draft.shard_override, code.to_string(), label);
                        }
                    });
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("GitHub Directory Owner").strong().color(TEXT_PRIMARY).size(12.5));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cfg_draft.github_owner)
                                    .desired_width(240.0)
                                    .margin(Margin::symmetric(8.0, 6.0)),
                            );
                        });
                        ui.add_space(14.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("GitHub Directory Repo").strong().color(TEXT_PRIMARY).size(12.5));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cfg_draft.github_repo)
                                    .desired_width(260.0)
                                    .margin(Margin::symmetric(8.0, 6.0)),
                            );
                        });
                    });
                });

            ui.add_space(12.0);

            Frame::none()
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::same(18.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Optional Platform API Keys").font(FontId::proportional(15.0)).strong().color(Color32::WHITE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.checkbox(&mut self.show_api_secrets, "Show keys");
                        });
                    });
                    ui.label(RichText::new("Leave blank to run 100% keyless on YouTube, Twitch, and Kick, or add free keys for faster official API checks.").color(TEXT_MUTED).size(12.0));
                    ui.add_space(10.0);

                    // YouTube Data API v3 Key row
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("YouTube Data API v3 Key").strong().color(TEXT_PRIMARY).size(12.5));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let yt_busy = matches!(yt_check, CheckState::Checking);
                            let test_yt = egui::Button::new(RichText::new(if yt_busy { "Testing..." } else { "Test Key" }).color(TEXT_PRIMARY).size(11.5))
                                .fill(BTN_SECONDARY)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .rounding(Rounding::same(4.0));
                            if ui.add_enabled(!yt_busy, test_yt).clicked() {
                                lock(&self.state).yt_key_check = CheckState::Checking;
                                let _ = self.bg_tx.send(BgTask::TestYouTubeKey(self.cfg_draft.youtube_api_key.clone()));
                            }
                            let get_yt = egui::Button::new(RichText::new("Get YouTube Key").color(Color32::WHITE).strong().size(11.5))
                                .fill(ACCENT_RED)
                                .rounding(Rounding::same(4.0));
                            if ui.add(get_yt).on_hover_text("Opens Google Cloud Console -> Credentials").clicked() {
                                let _ = open::that("https://console.cloud.google.com/apis/credentials");
                            }
                        });
                    });
                    ui.add_space(4.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cfg_draft.youtube_api_key)
                                .password(!self.show_api_secrets)
                                .hint_text("AIza... (optional)")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(8.0, 6.0)),
                        )
                        .changed()
                    {
                        lock(&self.state).yt_key_check = CheckState::Idle;
                    }
                    if !matches!(yt_check, CheckState::Idle) {
                        ui.add_space(5.0);
                        render_check_banner(ui, yt_check);
                    }
                    ui.add_space(10.0);

                    // Twitch Helix Client ID + Secret row
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Twitch Helix API (Client ID & Secret)").strong().color(TEXT_PRIMARY).size(12.5));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let tw_busy = matches!(twitch_check, CheckState::Checking);
                            let test_tw = egui::Button::new(RichText::new(if tw_busy { "Testing..." } else { "Test Twitch" }).color(TEXT_PRIMARY).size(11.5))
                                .fill(BTN_SECONDARY)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .rounding(Rounding::same(4.0));
                            if ui.add_enabled(!tw_busy, test_tw).clicked() {
                                lock(&self.state).twitch_key_check = CheckState::Checking;
                                let _ = self.bg_tx.send(BgTask::TestTwitchKeys {
                                    client_id: self.cfg_draft.twitch_client_id.clone(),
                                    client_secret: self.cfg_draft.twitch_client_secret.clone(),
                                });
                            }
                            let get_tw = egui::Button::new(RichText::new("Get Twitch Keys").color(Color32::WHITE).strong().size(11.5))
                                .fill(TWITCH_PURPLE)
                                .rounding(Rounding::same(4.0));
                            if ui.add(get_tw).on_hover_text("Opens Twitch Developer Console -> Applications").clicked() {
                                let _ = open::that("https://dev.twitch.tv/console/apps");
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new("Twitch Client ID").color(TEXT_MUTED).size(11.5));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cfg_draft.twitch_client_id)
                                .password(!self.show_api_secrets)
                                .hint_text("Optional Helix Client ID")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(8.0, 6.0)),
                        )
                        .changed()
                    {
                        lock(&self.state).twitch_key_check = CheckState::Idle;
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Twitch Client Secret").color(TEXT_MUTED).size(11.5));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cfg_draft.twitch_client_secret)
                                .password(!self.show_api_secrets)
                                .hint_text("Optional Helix Client Secret")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(8.0, 6.0)),
                        )
                        .changed()
                    {
                        lock(&self.state).twitch_key_check = CheckState::Idle;
                    }
                    if !matches!(twitch_check, CheckState::Idle) {
                        ui.add_space(5.0);
                        render_check_banner(ui, twitch_check);
                    }

                    ui.add_space(12.0);
                    egui::CollapsingHeader::new(
                        RichText::new("How to get free API keys (Step-by-Step Guide)")
                            .color(ALLY_TEAL)
                            .strong()
                            .size(12.5),
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        Frame::none()
                            .fill(BG_DARK)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(Margin::same(12.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new("YouTube Data API v3 (Free - 10,000 units/day, no card needed)").strong().color(ACCENT_RED).size(12.0));
                                ui.label(RichText::new("1. Click 'Get YouTube Key' above to open Google Cloud Console.").color(TEXT_PRIMARY).size(11.5));
                                ui.label(RichText::new("2. Create a new project (e.g. Valorant-Radar) from the top-left selector.").color(TEXT_PRIMARY).size(11.5));
                                ui.label(RichText::new("3. Open APIs & Services -> Library, search 'YouTube Data API v3', click Enable.").color(TEXT_PRIMARY).size(11.5));
                                ui.label(RichText::new("4. Open APIs & Services -> Credentials -> + CREATE CREDENTIALS -> API key.").color(TEXT_PRIMARY).size(11.5));
                                ui.label(RichText::new("5. Paste the AIza... key above, click 'Test Key', then 'Save Settings'.").color(TEXT_PRIMARY).size(11.5));

                                ui.add_space(8.0);
                                ui.label(RichText::new("Twitch Helix API (Free - requires 2FA on your Twitch account)").strong().color(TWITCH_PURPLE).size(12.0));
                                ui.label(RichText::new("1. Click 'Get Twitch Keys' above to open Twitch Developer Console.").color(TEXT_PRIMARY).size(11.5));
                                ui.label(RichText::new("2. Click 'Register Your Application', name it 'Valorant Radar'.").color(TEXT_PRIMARY).size(11.5));
                                ui.label(RichText::new("3. Set OAuth Redirect URL to http://localhost and Category to Application Integration.").color(TEXT_PRIMARY).size(11.5));
                                ui.label(RichText::new("4. Open Manage -> copy Client ID, click 'New Secret' -> copy Client Secret.").color(TEXT_PRIMARY).size(11.5));
                                ui.label(RichText::new("5. Paste both above, click 'Test Twitch', then 'Save Settings'.").color(TEXT_PRIMARY).size(11.5));

                                ui.add_space(6.0);
                                ui.label(RichText::new("Kick does not need an API key. Leaving keys empty still works via keyless fallbacks.").color(TEXT_MUTED).size(11.0));
                            });
                    });
                });

            ui.add_space(12.0);

            Frame::none()
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::same(18.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("Windows Integration").font(FontId::proportional(15.0)).strong().color(Color32::WHITE));
                    ui.add_space(8.0);
                    ui.checkbox(&mut self.cfg_draft.start_with_windows, "Start silently with Windows (HKCU Run)");
                    ui.checkbox(
                        &mut self.cfg_draft.use_custom_toast_app_id,
                        "Register custom Windows Toast AppUserModelID (shows 'Valorant Streamer Radar' instead of PowerShell)",
                    );
                });

            if let Some(err) = &self.settings_error {
                ui.add_space(8.0);
                ui.label(RichText::new(err).color(ACCENT_RED).size(12.5));
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let save = egui::Button::new(RichText::new("Save Settings").color(Color32::WHITE).strong().size(13.0))
                    .fill(ACCENT_RED)
                    .rounding(Rounding::same(6.0))
                    .min_size(Vec2::new(136.0, 36.0));
                if ui.add(save).clicked() {
                    self.save_settings();
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("Stored atomically in {}", data_dir().join("config.json").display()))
                        .color(TEXT_MUTED)
                        .size(11.5),
                );
            });
        });
    }
}

// ============================================================================
// ENTRY POINT
// ============================================================================

fn main() -> Result<()> {
    std::panic::set_hook(Box::new(|info| log(format!("PANIC: {info}"))));

    let start_hidden = env::args().any(|a| a == "--background");
    WINDOW_VISIBLE.store(!start_hidden, Ordering::Relaxed);

    // Single instance: if already running, wake the existing window (unless --background) and exit.
    let instance_listener = match TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(l) => l,
        Err(_) => {
            if !start_hidden {
                let addr = SocketAddr::from(([127, 0, 0, 1], SINGLE_INSTANCE_PORT));
                if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(250)) {
                    let _ = stream.write_all(b"SHOW");
                }
            }
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
    register_toast_app_id(config.use_custom_toast_app_id);

    let state = Arc::new(Mutex::new(AppState {
        status: "Idle | Monitoring ShooterGame.log".to_string(),
        is_scanning: false,
        is_syncing_directory: false,
        riot_status: RiotStatus::Offline,
        detected_shard: "ap".to_string(),
        config: config.clone(),
        history: load_json("streamer_history.json"),
        local_directory: load_json("custom_streamers.json"),
        remote_directory: load_json("directory_cache.json"),
        toast: None,
        last_scanned_match: None,
        yt_key_check: CheckState::Idle,
        twitch_key_check: CheckState::Idle,
        streamer_verify: CheckState::Idle,
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
    let (bg_tx, mut bg_rx) = mpsc::unbounded_channel::<BgTask>();

    let window_icon = Arc::new(egui::IconData {
        rgba: radar_icon_rgba(),
        width: 32,
        height: 32,
    });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Valorant Streamer Radar")
            .with_icon(window_icon)
            .with_inner_size([840.0, 580.0])
            .with_min_inner_size([660.0, 440.0])
            .with_visible(!start_hidden),
        ..Default::default()
    };

    let state_for_app = state.clone();
    let tx_for_app = scan_tx.clone();
    let bg_tx_for_app = bg_tx.clone();

    eframe::run_native(
        "Valorant Streamer Radar",
        native_options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();

            // Keep single-instance port bound and wake this window if the user launches the exe again.
            let ctx_wake = ctx.clone();
            let _ = std::thread::Builder::new().name("radar-ipc".into()).spawn(move || {
                for stream in instance_listener.incoming().flatten() {
                    drop(stream);
                    show_window(&ctx_wake);
                }
            });

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
                        .http1_only()
                        .no_proxy()
                        .timeout(Duration::from_secs(10))
                        .build()
                        .expect("local HTTP client");
                    let public_client = Client::builder()
                        .connect_timeout(Duration::from_secs(5))
                        .timeout(Duration::from_secs(10))
                        .build()
                        .expect("public HTTP client");

                    // Non-blocking worker for API key tests & live streamer credential verification.
                    let (st_check, cx_check, pub_check) = (state_bg.clone(), ctx_bg.clone(), public_client.clone());
                    tokio::spawn(async move {
                        while let Some(task) = bg_rx.recv().await {
                            let (st, cx, client) = (st_check.clone(), cx_check.clone(), pub_check.clone());
                            tokio::spawn(async move {
                                match task {
                                    BgTask::TestYouTubeKey(key) => {
                                        let res = test_youtube_api_key(&client, &key).await;
                                        lock(&st).yt_key_check = res;
                                        repaint(&cx);
                                    }
                                    BgTask::TestTwitchKeys { client_id, client_secret } => {
                                        let res = test_twitch_api_keys(&client, &client_id, &client_secret).await;
                                        lock(&st).twitch_key_check = res;
                                        repaint(&cx);
                                    }
                                    BgTask::VerifyStreamer { riot_id, platform, channel_name, channel_url } => {
                                        let res = verify_streamer_entry(&client, &riot_id, &platform, &channel_name, &channel_url).await;
                                        lock(&st).streamer_verify = res;
                                        repaint(&cx);
                                    }
                                }
                            });
                        }
                    });

                    // Single worker serializes all scans (manual + post-match) and survives panics.
                    let (st, cx, local_scan) = (state_bg.clone(), ctx_bg.clone(), local_client.clone());
                    tokio::spawn(async move {
                        while let Some(first) = scan_rx.recv().await {
                            let mut trigger = first;
                            while let Ok(t) = scan_rx.try_recv() {
                                if t == ScanTrigger::PostMatch {
                                    trigger = t;
                                } else if trigger == ScanTrigger::RefreshDirectory && t == ScanTrigger::Manual {
                                    trigger = t;
                                }
                            }
                            if trigger == ScanTrigger::RefreshDirectory {
                                let handle = tokio::spawn(sync_directory_only(
                                    public_client.clone(),
                                    st.clone(),
                                    cx.clone(),
                                ));
                                if let Err(e) = handle.await {
                                    log(format!("Directory sync crashed: {e}"));
                                    lock(&st).is_syncing_directory = false;
                                }
                                repaint(&cx);
                                continue;
                            }

                            lock(&st).is_scanning = true;
                            let handle = tokio::spawn(scan_last_match(
                                local_scan.clone(),
                                public_client.clone(),
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
                    watch_log(log_path, local_client, state_bg, ctx_bg, tx_bg).await;
                });
            });
            if let Err(e) = spawned {
                log(format!("Failed to spawn background thread: {e}"));
            }

            Ok(Box::new(StreamerApp::new(cc, state_for_app, tx_for_app, bg_tx_for_app, start_hidden)))
        }),
    )
    .map_err(|e| anyhow!("GUI error: {e}"))
}
