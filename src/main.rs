#![windows_subsystem = "windows"]

use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Local;
use egui::{Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, StrokeKind, Vec2};
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

// ============================================================================
// CONFIGURATION & THEME CONSTANTS
// ============================================================================

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_BUILD_SHA: &str = env!("APP_BUILD_SHA");
const UPDATE_REPO_OWNER: &str = "svg2709";
const UPDATE_REPO_NAME: &str = "Valorant-Streamer-Radar";

const COMMUNITY_REPO_OWNER: &str = "svg2709";
const COMMUNITY_REPO_NAME: &str = "Valorant-Streamer-Radar";
const DEFAULT_SERVER_OVERRIDE: &str = "auto";
const CUSTOM_TOAST_APP_ID: &str = "ValorantStreamerRadar";

const SINGLE_INSTANCE_PORT: u16 = 47831;
const MAX_HISTORY: usize = 300;
const TOAST_TTL: Duration = Duration::from_secs(4);
const RECOMMENDATIONS_CACHE_TTL: Duration = Duration::from_secs(300);

const BG_DARK: Color32 = Color32::from_rgb(0, 0, 0);
const HEADER_BG: Color32 = Color32::from_rgb(0, 0, 0);
const CARD_BG: Color32 = Color32::from_rgb(11, 11, 14);
const CARD_BORDER: Color32 = Color32::from_rgb(31, 31, 38);
const BTN_SECONDARY: Color32 = Color32::from_rgb(18, 18, 22);
const SURFACE_HOVER: Color32 = Color32::from_rgb(26, 26, 32);
const ACCENT_PINK: Color32 = Color32::from_rgb(255, 92, 168);
const ACCENT_BLUSH: Color32 = Color32::from_rgb(255, 166, 210);
const ACCENT_PEARL: Color32 = Color32::from_rgb(255, 245, 250);
const ERR_ROSE: Color32 = Color32::from_rgb(255, 102, 138);
const TWITCH_PURPLE: Color32 = Color32::from_rgb(167, 112, 255);
const KICK_GREEN: Color32 = Color32::from_rgb(83, 252, 24);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(244, 244, 246);
const TEXT_MUTED: Color32 = Color32::from_rgb(142, 149, 165);
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

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)).round() as u8
}

fn pink_white_rgba(t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    if t < 0.55 {
        let k = t / 0.55;
        [255, lerp_u8(77, 166, k), lerp_u8(148, 210, k), 255]
    } else {
        let k = (t - 0.55) / 0.45;
        [255, lerp_u8(166, 245, k), lerp_u8(210, 250, k), 255]
    }
}

fn pink_white_color(t: f32) -> Color32 {
    let px = pink_white_rgba(t);
    Color32::from_rgb(px[0], px[1], px[2])
}

fn icon_dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    if len_sq <= 1e-8 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * abx + (py - ay) * aby) / len_sq).clamp(0.0, 1.0);
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn icon_rounded_box_sdf(u: f32, v: f32, half: f32, radius: f32) -> f32 {
    let qx = (u - 0.5).abs() - (half - radius);
    let qy = (v - 0.5).abs() - (half - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - radius
}

/// Samples a single normalized (u, v) point of the unified AMOLED rounded-square brand emblem
/// (matches build.rs .exe icon, paint_brand_mark, and RadarIcon.tsx).
fn sample_radar_icon_rgba(u: f32, v: f32) -> [u8; 4] {
    let outer_half = 0.47;
    let radius = 0.16;
    let border_w = 0.068;
    let sdf = icon_rounded_box_sdf(u, v, outer_half, radius);

    if sdf > 0.0 {
        return [0, 0, 0, 0];
    }

    let in_border = sdf >= -border_w;
    let d_left = icon_dist_to_segment(u, v, 0.27, 0.27, 0.50, 0.74);
    let d_right = icon_dist_to_segment(u, v, 0.50, 0.74, 0.73, 0.27);
    let in_v = d_left.min(d_right) <= 0.058;

    let dot_dist = ((u - 0.50).powi(2) + (v - 0.30).powi(2)).sqrt();
    let in_dot = dot_dist <= 0.072;

    if in_border {
        let grad_t = u * 0.55 + (1.0 - v) * 0.45;
        pink_white_rgba(grad_t)
    } else if in_dot {
        [255, 92, 168, 255]
    } else if in_v {
        let grad_t = 0.78 + (1.0 - v) * 0.22;
        pink_white_rgba(grad_t)
    } else {
        [lerp_u8(0, 10, v), lerp_u8(0, 8, v), lerp_u8(0, 14, v), 255]
    }
}

/// Generates the 32x32 RGBA pixel buffer with 2x2 anti-aliasing matching the .exe and in-app brand emblem.
fn radar_icon_rgba() -> Vec<u8> {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let offsets = [0.25f32, 0.75f32];
    for y in 0..size {
        for x in 0..size {
            let mut r_acc = 0.0f32;
            let mut g_acc = 0.0f32;
            let mut b_acc = 0.0f32;
            let mut a_acc = 0.0f32;
            for &oy in &offsets {
                for &ox in &offsets {
                    let u = (x as f32 + ox) / size as f32;
                    let v = (y as f32 + oy) / size as f32;
                    let s = sample_radar_icon_rgba(u, v);
                    let a = s[3] as f32 / 255.0;
                    r_acc += s[0] as f32 * a;
                    g_acc += s[1] as f32 * a;
                    b_acc += s[2] as f32 * a;
                    a_acc += a;
                }
            }
            let idx = ((y * size + x) * 4) as usize;
            let avg_a = a_acc / 4.0;
            if avg_a <= 0.001 {
                rgba[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
            } else {
                rgba[idx] = (r_acc / a_acc).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = (g_acc / a_acc).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = (b_acc / a_acc).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = (avg_a * 255.0).round().clamp(0.0, 255.0) as u8;
            }
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
    let and_row_bytes: u32 = ((size + 31) / 32) * 4;
    let and_len: u32 = and_row_bytes * size;
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
    // 1bpp AND mask matching transparent rounded corners
    for y in (0..size).rev() {
        let mut row = vec![0u8; and_row_bytes as usize];
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            if rgba[idx + 3] == 0 {
                row[(x / 8) as usize] |= 1u8 << (7 - (x % 8));
            }
        }
        ico.extend_from_slice(&row);
    }

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
    #[serde(default = "default_server_override", alias = "shard_override")]
    server_override: String,
    #[allow(dead_code)]
    #[serde(default = "default_true")]
    use_custom_toast_app_id: bool,
    #[serde(default = "default_true")]
    capture_in_game_chat: bool,
    #[serde(default = "default_true")]
    skip_party_members: bool,
    #[serde(default)]
    enable_recommendations: bool,
    #[serde(default = "default_true")]
    check_updates_on_boot: bool,
}

fn default_true() -> bool {
    true
}

fn default_server_override() -> String {
    DEFAULT_SERVER_OVERRIDE.to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            youtube_api_key: String::new(),
            twitch_client_id: String::new(),
            twitch_client_secret: String::new(),
            start_with_windows: true,
            server_override: default_server_override(),
            use_custom_toast_app_id: true,
            capture_in_game_chat: true,
            skip_party_members: true,
            enable_recommendations: false,
            check_updates_on_boot: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DirectoryStreamer {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    display_name: String,
    riot_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
    platform: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    channel_id: String,
    channel_name: String,
    channel_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    server_region: String,
    #[serde(default)]
    verified: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    submitted_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfidenceTier {
    Confirmed,
    Probable,
    Unverified,
}

impl ConfidenceTier {
    fn as_str(self) -> &'static str {
        match self {
            ConfidenceTier::Confirmed => "Confirmed",
            ConfidenceTier::Probable => "Probable",
            ConfidenceTier::Unverified => "Unverified",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LikelyStreamerCandidate {
    platform: String,
    channel: String,
    title: String,
    url: String,
    #[serde(default)]
    confidence: u8,
    #[serde(default)]
    confidence_tier: String,
    #[serde(default)]
    region_hint: String,
    #[serde(default)]
    server_match_status: String,
    #[serde(default)]
    match_reason: String,
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
    detected_at_iso: String,
    #[serde(default)]
    team_relation: String,
    #[serde(default)]
    map_name: String,
    #[serde(default)]
    agent_name: String,
    #[serde(default)]
    queue_name: String,
    #[serde(default)]
    chat_message: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    confidence: u8,
    #[serde(default)]
    confidence_tier: String,
    #[serde(default)]
    server_match_status: String,
    #[serde(default)]
    match_reason: String,
    #[serde(default)]
    server_region: String,
    #[serde(default)]
    candidates: Vec<LikelyStreamerCandidate>,
}

#[derive(Debug, Clone)]
struct RecommendedStreamer {
    platform: String,
    channel: String,
    title: String,
    url: String,
    viewers: Option<u64>,
    region_label: String,
}

#[derive(Debug, Clone)]
enum RecommendationsState {
    Idle,
    Loading,
    Loaded(Vec<RecommendedStreamer>, String),
    Error(String),
}

#[derive(Debug, Clone, Deserialize)]
struct ChatMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    mid: String,
    #[serde(default)]
    cid: String,
    #[serde(default)]
    puuid: String,
    #[serde(default)]
    game_name: String,
    #[serde(default)]
    game_tag: String,
    #[serde(default)]
    body: String,
    #[serde(default, rename = "type")]
    msg_type: String,
}

#[derive(Debug, Clone)]
struct ChatStreamPlug {
    platform: String,
    url: Option<String>,
    handle: Option<String>,
    raw_message: String,
    priority: u8,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateReleaseInfo {
    tag_name: String,
    release_name: String,
    changelog: String,
    download_url: String,
    html_url: String,
    expected_sha256: Option<String>,
    published_at: String,
    size_bytes: u64,
    remote_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateState {
    Idle,
    Checking,
    UpToDate(String),
    Available(UpdateReleaseInfo),
    Downloading(String),
    Installing(String),
    Error(String),
}

enum BgTask {
    TestYouTubeKey(String),
    TestTwitchKeys { client_id: String, client_secret: String },
    VerifyStreamer { riot_id: String, platform: String, channel_name: String, channel_url: String },
    CheckForUpdates { silent: bool },
    InstallUpdate(UpdateReleaseInfo),
    FetchRecommendations {
        server_region: String,
        youtube_api_key: String,
        twitch_client_id: String,
        twitch_client_secret: String,
        force: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LastMatchSummary {
    match_id: String,
    queue_name: String,
    map_name: String,
    #[serde(default)]
    my_agent: String,
    players_checked: usize,
    #[serde(default)]
    party_skipped: usize,
    #[serde(default)]
    hidden_skipped: usize,
    streamers_found: usize,
    scanned_at: String,
    #[serde(default, alias = "shard")]
    server_name: String,
}

struct AppState {
    status: String,
    is_scanning: bool,
    is_syncing_directory: bool,
    riot_status: RiotStatus,
    detected_api_region: String,
    detected_server: String,
    config: AppConfig,
    history: Vec<StreamerEntry>,
    local_directory: Vec<DirectoryStreamer>,
    remote_directory: Vec<DirectoryStreamer>,
    toast: Option<(String, Instant)>,
    last_scanned_match: Option<String>,
    last_match_summary: Option<LastMatchSummary>,
    chat_buffer: Vec<ChatMessage>,
    yt_key_check: CheckState,
    twitch_key_check: CheckState,
    streamer_verify: CheckState,
    update_state: UpdateState,
    notified_update_key: Option<String>,
    recommendations: RecommendationsState,
    recommendations_cache: Option<(String, Instant, Vec<RecommendedStreamer>, String)>,
}

fn set_toast(state: &Arc<Mutex<AppState>>, msg: impl Into<String>) {
    lock(state).toast = Some((msg.into(), Instant::now()));
}

fn set_status(state: &Arc<Mutex<AppState>>, ctx: &egui::Context, msg: impl Into<String>) {
    lock(state).status = msg.into();
    repaint(ctx);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeoZone {
    India,
    Sea,
    Japan,
    Korea,
    Oce,
    Na,
    Latam,
    Brazil,
    Eu,
    MiddleEast,
    SouthAfrica,
}

#[derive(Debug, Clone, Copy)]
struct ServerRegionInfo {
    code: &'static str,
    api_region: &'static str,
    label: &'static str,
    short_label: &'static str,
    yt_region_code: &'static str,
    accept_lang: &'static str,
    search_region_term: &'static str,
    twitch_lang: &'static str,
    zone: GeoZone,
}

const SERVER_REGIONS: &[ServerRegionInfo] = &[
    ServerRegionInfo {
        code: "ap-mumbai",
        api_region: "ap",
        label: "AP / Mumbai (India)",
        short_label: "Mumbai",
        yt_region_code: "IN",
        accept_lang: "en-IN,hi;q=0.9,en;q=0.8",
        search_region_term: "India Mumbai",
        twitch_lang: "hi",
        zone: GeoZone::India,
    },
    ServerRegionInfo {
        code: "ap-singapore",
        api_region: "ap",
        label: "AP / Singapore (SEA)",
        short_label: "Singapore",
        yt_region_code: "SG",
        accept_lang: "en-SG,en;q=0.9",
        search_region_term: "Singapore SEA",
        twitch_lang: "en",
        zone: GeoZone::Sea,
    },
    ServerRegionInfo {
        code: "ap-tokyo",
        api_region: "ap",
        label: "AP / Tokyo (Japan)",
        short_label: "Japan",
        yt_region_code: "JP",
        accept_lang: "ja-JP,ja;q=0.9,en;q=0.7",
        search_region_term: "Japan Tokyo",
        twitch_lang: "ja",
        zone: GeoZone::Japan,
    },
    ServerRegionInfo {
        code: "ap-manila",
        api_region: "ap",
        label: "AP / Manila (Philippines)",
        short_label: "Manila",
        yt_region_code: "PH",
        accept_lang: "en-PH,tl;q=0.9,en;q=0.8",
        search_region_term: "Philippines Manila",
        twitch_lang: "tl",
        zone: GeoZone::Sea,
    },
    ServerRegionInfo {
        code: "ap-hongkong",
        api_region: "ap",
        label: "AP / Hong Kong",
        short_label: "Hong Kong",
        yt_region_code: "HK",
        accept_lang: "zh-HK,en-HK;q=0.9,en;q=0.8",
        search_region_term: "Hong Kong",
        twitch_lang: "zh",
        zone: GeoZone::Sea,
    },
    ServerRegionInfo {
        code: "ap-sydney",
        api_region: "ap",
        label: "AP / Sydney (OCE)",
        short_label: "Sydney",
        yt_region_code: "AU",
        accept_lang: "en-AU,en;q=0.9",
        search_region_term: "OCE Australia",
        twitch_lang: "en",
        zone: GeoZone::Oce,
    },
    ServerRegionInfo {
        code: "kr-seoul",
        api_region: "kr",
        label: "KR / Seoul (Korea)",
        short_label: "Seoul",
        yt_region_code: "KR",
        accept_lang: "ko-KR,ko;q=0.9,en;q=0.7",
        search_region_term: "Korea KR",
        twitch_lang: "ko",
        zone: GeoZone::Korea,
    },
    ServerRegionInfo {
        code: "na-austin",
        api_region: "na",
        label: "NA / Austin & Dallas (Texas)",
        short_label: "Austin / TX",
        yt_region_code: "US",
        accept_lang: "en-US,en;q=0.9",
        search_region_term: "NA Texas",
        twitch_lang: "en",
        zone: GeoZone::Na,
    },
    ServerRegionInfo {
        code: "na-us-east",
        api_region: "na",
        label: "NA / US East (Virginia / Georgia / Illinois)",
        short_label: "US East",
        yt_region_code: "US",
        accept_lang: "en-US,en;q=0.9",
        search_region_term: "NA East",
        twitch_lang: "en",
        zone: GeoZone::Na,
    },
    ServerRegionInfo {
        code: "na-us-west",
        api_region: "na",
        label: "NA / US West (California / Oregon)",
        short_label: "US West",
        yt_region_code: "US",
        accept_lang: "en-US,en;q=0.9",
        search_region_term: "NA West",
        twitch_lang: "en",
        zone: GeoZone::Na,
    },
    ServerRegionInfo {
        code: "latam-mexico",
        api_region: "na",
        label: "LATAM / Mexico, Bogota & Santiago",
        short_label: "LATAM",
        yt_region_code: "MX",
        accept_lang: "es-MX,es;q=0.9,en;q=0.7",
        search_region_term: "LATAM",
        twitch_lang: "es",
        zone: GeoZone::Latam,
    },
    ServerRegionInfo {
        code: "br-saopaulo",
        api_region: "na",
        label: "BR / Sao Paulo (Brazil)",
        short_label: "Brazil",
        yt_region_code: "BR",
        accept_lang: "pt-BR,pt;q=0.9,en;q=0.7",
        search_region_term: "Brasil",
        twitch_lang: "pt",
        zone: GeoZone::Brazil,
    },
    ServerRegionInfo {
        code: "eu-london",
        api_region: "eu",
        label: "EU / London (UK)",
        short_label: "London",
        yt_region_code: "GB",
        accept_lang: "en-GB,en;q=0.9",
        search_region_term: "EU UK",
        twitch_lang: "en",
        zone: GeoZone::Eu,
    },
    ServerRegionInfo {
        code: "eu-frankfurt",
        api_region: "eu",
        label: "EU / Frankfurt (Germany / Central)",
        short_label: "Frankfurt",
        yt_region_code: "DE",
        accept_lang: "de-DE,en;q=0.8",
        search_region_term: "EU Frankfurt",
        twitch_lang: "de",
        zone: GeoZone::Eu,
    },
    ServerRegionInfo {
        code: "eu-paris",
        api_region: "eu",
        label: "EU / Paris (France)",
        short_label: "Paris",
        yt_region_code: "FR",
        accept_lang: "fr-FR,en;q=0.8",
        search_region_term: "France EU",
        twitch_lang: "fr",
        zone: GeoZone::Eu,
    },
    ServerRegionInfo {
        code: "eu-stockholm",
        api_region: "eu",
        label: "EU / Stockholm (Nordics)",
        short_label: "Stockholm",
        yt_region_code: "SE",
        accept_lang: "sv-SE,en;q=0.9",
        search_region_term: "EU Nordics",
        twitch_lang: "en",
        zone: GeoZone::Eu,
    },
    ServerRegionInfo {
        code: "eu-warsaw",
        api_region: "eu",
        label: "EU / Warsaw (Poland)",
        short_label: "Warsaw",
        yt_region_code: "PL",
        accept_lang: "pl-PL,en;q=0.8",
        search_region_term: "Poland EU",
        twitch_lang: "pl",
        zone: GeoZone::Eu,
    },
    ServerRegionInfo {
        code: "eu-madrid",
        api_region: "eu",
        label: "EU / Madrid (Spain)",
        short_label: "Madrid",
        yt_region_code: "ES",
        accept_lang: "es-ES,en;q=0.8",
        search_region_term: "Spain EU",
        twitch_lang: "es",
        zone: GeoZone::Eu,
    },
    ServerRegionInfo {
        code: "eu-istanbul",
        api_region: "eu",
        label: "EU / Istanbul (Turkey)",
        short_label: "Istanbul",
        yt_region_code: "TR",
        accept_lang: "tr-TR,en;q=0.8",
        search_region_term: "Turkey TR",
        twitch_lang: "tr",
        zone: GeoZone::Eu,
    },
    ServerRegionInfo {
        code: "eu-bahrain",
        api_region: "eu",
        label: "EU / Bahrain (Middle East)",
        short_label: "Bahrain",
        yt_region_code: "AE",
        accept_lang: "ar-AE,en;q=0.9",
        search_region_term: "Middle East Bahrain",
        twitch_lang: "ar",
        zone: GeoZone::MiddleEast,
    },
    ServerRegionInfo {
        code: "eu-capetown",
        api_region: "eu",
        label: "EU / Cape Town (South Africa)",
        short_label: "Cape Town",
        yt_region_code: "ZA",
        accept_lang: "en-ZA,en;q=0.9",
        search_region_term: "South Africa",
        twitch_lang: "en",
        zone: GeoZone::SouthAfrica,
    },
];

fn server_region_by_code(code: &str) -> Option<&'static ServerRegionInfo> {
    let norm = code.trim().to_ascii_lowercase();
    if let Some(info) = SERVER_REGIONS.iter().find(|r| r.code == norm) {
        return Some(info);
    }
    match norm.as_str() {
        "ap" | "mumbai" | "india" => SERVER_REGIONS.iter().find(|r| r.code == "ap-mumbai"),
        "singapore" | "sg" => SERVER_REGIONS.iter().find(|r| r.code == "ap-singapore"),
        "tokyo" | "japan" | "jp" => SERVER_REGIONS.iter().find(|r| r.code == "ap-tokyo"),
        "manila" | "philippines" | "ph" => SERVER_REGIONS.iter().find(|r| r.code == "ap-manila"),
        "hongkong" | "hong kong" | "hk" => SERVER_REGIONS.iter().find(|r| r.code == "ap-hongkong"),
        "sydney" | "oce" => SERVER_REGIONS.iter().find(|r| r.code == "ap-sydney"),
        "kr" | "seoul" | "korea" => SERVER_REGIONS.iter().find(|r| r.code == "kr-seoul"),
        "na" | "austin" | "texas" | "dallas" => SERVER_REGIONS.iter().find(|r| r.code == "na-austin"),
        "us-east" | "virginia" => SERVER_REGIONS.iter().find(|r| r.code == "na-us-east"),
        "us-west" | "california" | "oregon" => SERVER_REGIONS.iter().find(|r| r.code == "na-us-west"),
        "latam" | "mexico" => SERVER_REGIONS.iter().find(|r| r.code == "latam-mexico"),
        "br" | "brazil" | "saopaulo" => SERVER_REGIONS.iter().find(|r| r.code == "br-saopaulo"),
        "eu" | "frankfurt" => SERVER_REGIONS.iter().find(|r| r.code == "eu-frankfurt"),
        "london" | "uk" => SERVER_REGIONS.iter().find(|r| r.code == "eu-london"),
        "paris" => SERVER_REGIONS.iter().find(|r| r.code == "eu-paris"),
        "stockholm" => SERVER_REGIONS.iter().find(|r| r.code == "eu-stockholm"),
        "warsaw" => SERVER_REGIONS.iter().find(|r| r.code == "eu-warsaw"),
        "madrid" => SERVER_REGIONS.iter().find(|r| r.code == "eu-madrid"),
        "istanbul" => SERVER_REGIONS.iter().find(|r| r.code == "eu-istanbul"),
        "bahrain" => SERVER_REGIONS.iter().find(|r| r.code == "eu-bahrain"),
        "capetown" | "cape town" => SERVER_REGIONS.iter().find(|r| r.code == "eu-capetown"),
        _ => None,
    }
}

fn parse_game_pod_to_server(pod: &str) -> Option<&'static str> {
    let p = pod.to_ascii_lowercase();
    if p.contains("mumbai") || p.contains("ap-south-1") || p.contains("bom") {
        Some("ap-mumbai")
    } else if p.contains("manila") || p.contains("mnl") {
        Some("ap-manila")
    } else if p.contains("tokyo") || p.contains("ap-northeast-1") || p.contains("tyo") || p.contains("japan") {
        Some("ap-tokyo")
    } else if p.contains("singapore") || p.contains("ap-southeast-1") || p.contains("sgp") {
        Some("ap-singapore")
    } else if p.contains("hongkong") || p.contains("hong-kong") || p.contains("ap-east-1") || p.contains("hkg") {
        Some("ap-hongkong")
    } else if p.contains("sydney") || p.contains("ap-southeast-2") || p.contains("syd") {
        Some("ap-sydney")
    } else if p.contains("seoul") || p.contains("ap-northeast-2") || p.contains("icn") {
        Some("kr-seoul")
    } else if p.contains("austin") || p.contains("texas") || p.contains("dallas") || p.contains("dfw") {
        Some("na-austin")
    } else if p.contains("ashburn")
        || p.contains("virginia")
        || p.contains("atlanta")
        || p.contains("georgia")
        || p.contains("chicago")
        || p.contains("illinois")
        || p.contains("miami")
        || p.contains("us-east")
    {
        Some("na-us-east")
    } else if p.contains("california") || p.contains("sanjose") || p.contains("oregon") || p.contains("us-west") {
        Some("na-us-west")
    } else if p.contains("saopaulo") || p.contains("sa-east-1") || p.contains("gru") {
        Some("br-saopaulo")
    } else if p.contains("mexico") || p.contains("bogota") || p.contains("santiago") {
        Some("latam-mexico")
    } else if p.contains("london") || p.contains("eu-west-2") {
        Some("eu-london")
    } else if p.contains("frankfurt") || p.contains("eu-central-1") {
        Some("eu-frankfurt")
    } else if p.contains("paris") || p.contains("eu-west-3") {
        Some("eu-paris")
    } else if p.contains("stockholm") || p.contains("eu-north-1") {
        Some("eu-stockholm")
    } else if p.contains("warsaw") {
        Some("eu-warsaw")
    } else if p.contains("madrid") || p.contains("eu-south-2") {
        Some("eu-madrid")
    } else if p.contains("istanbul") {
        Some("eu-istanbul")
    } else if p.contains("bahrain") || p.contains("me-south-1") || p.contains("dubai") {
        Some("eu-bahrain")
    } else if p.contains("capetown") || p.contains("af-south-1") {
        Some("eu-capetown")
    } else {
        None
    }
}

fn canonical_api_region(s: &str) -> Option<&'static str> {
    if let Some(reg) = server_region_by_code(s) {
        return Some(reg.api_region);
    }
    match s.trim().to_ascii_lowercase().as_str() {
        "ap" => Some("ap"),
        "na" | "latam" | "br" => Some("na"),
        "eu" => Some("eu"),
        "kr" => Some("kr"),
        _ => None,
    }
}

fn effective_server_region(
    config: &AppConfig,
    detected_server: &str,
    detected_api_region: &str,
) -> &'static ServerRegionInfo {
    if !config.server_override.eq_ignore_ascii_case("auto") {
        if let Some(info) = server_region_by_code(&config.server_override) {
            return info;
        }
    }
    server_region_by_code(detected_server)
        .or_else(|| server_region_by_code(detected_api_region))
        .unwrap_or(&SERVER_REGIONS[0])
}

fn geo_zone_label(zone: GeoZone) -> &'static str {
    match zone {
        GeoZone::India => "India / Mumbai",
        GeoZone::Sea => "SEA / Manila / Singapore",
        GeoZone::Japan => "Japan / Tokyo",
        GeoZone::Korea => "Korea / Seoul",
        GeoZone::Oce => "OCE / Sydney",
        GeoZone::Na => "North America",
        GeoZone::Latam => "LATAM",
        GeoZone::Brazil => "Brazil",
        GeoZone::Eu => "Europe",
        GeoZone::MiddleEast => "Middle East",
        GeoZone::SouthAfrica => "South Africa",
    }
}

fn zones_compatible(expected: GeoZone, detected: GeoZone) -> bool {
    if expected == detected {
        return true;
    }
    matches!(
        (expected, detected),
        (GeoZone::India, GeoZone::Sea)
            | (GeoZone::Sea, GeoZone::India)
            | (GeoZone::Sea, GeoZone::Japan)
            | (GeoZone::Japan, GeoZone::Sea)
            | (GeoZone::Japan, GeoZone::Korea)
            | (GeoZone::Korea, GeoZone::Japan)
            | (GeoZone::Sea, GeoZone::Oce)
            | (GeoZone::Oce, GeoZone::Sea)
            | (GeoZone::Na, GeoZone::Latam)
            | (GeoZone::Latam, GeoZone::Na)
            | (GeoZone::Latam, GeoZone::Brazil)
            | (GeoZone::Brazil, GeoZone::Latam)
            | (GeoZone::Eu, GeoZone::MiddleEast)
            | (GeoZone::MiddleEast, GeoZone::Eu)
    )
}

fn has_ascii_word(haystack_lower: &str, needle: &str) -> bool {
    let bytes = haystack_lower.as_bytes();
    let nlen = needle.len();
    if nlen == 0 || bytes.len() < nlen {
        return false;
    }
    let mut idx = 0usize;
    while let Some(pos) = haystack_lower[idx..].find(needle) {
        let start = idx + pos;
        let end = start + nlen;
        let left_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let right_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if left_ok && right_ok {
            return true;
        }
        idx = start + 1;
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionEvidence {
    /// Explicit Valorant in-game server reference (highest authority).
    InGameServer(GeoZone),
    /// Explicit streaming region/country tag (secondary authority).
    StreamingRegion(GeoZone),
    /// Unambiguous non-English broadcast language cue (lowest authority; never assumed to equal server alone).
    LanguageHint(GeoZone),
    /// No verifiable region signal found.
    Unknown,
}

impl RegionEvidence {
    fn zone(self) -> Option<GeoZone> {
        match self {
            RegionEvidence::InGameServer(z)
            | RegionEvidence::StreamingRegion(z)
            | RegionEvidence::LanguageHint(z) => Some(z),
            RegionEvidence::Unknown => None,
        }
    }
}

/// Distinguishes actual in-game server cues from streaming region tags and language hints.
fn classify_region_evidence(text: &str, lang_code: Option<&str>) -> RegionEvidence {
    let lower = text.to_ascii_lowercase();

    // 1. Highest priority: explicit in-game Valorant server cues
    const IN_GAME_SERVER_MAP: &[(&[&str], GeoZone)] = &[
        (&["mumbai server", "mumbai lobby", "ind server", "india server"], GeoZone::India),
        (&["tokyo server", "tokyo lobby", "jp server", "japan server"], GeoZone::Japan),
        (&["manila server", "singapore server", "sg server", "hk server", "hong kong server", "sea server"], GeoZone::Sea),
        (&["seoul server", "kr server", "korea server"], GeoZone::Korea),
        (&["sydney server", "oce server", "oce ranked"], GeoZone::Oce),
        (
            &[
                "austin server", "texas server", "dallas server", "na east", "na west", "na central",
                "na server", "na ranked", "na radiant", "na immortal", "na ascendant", "us east", "us west",
                "illinois server", "virginia server", "georgia server", "oregon server", "california server",
            ],
            GeoZone::Na,
        ),
        (&["br server", "sao paulo server"], GeoZone::Brazil),
        (&["latam server", "mexico server", "bogota server", "santiago server"], GeoZone::Latam),
        (&["bahrain server", "dubai server", "mena server"], GeoZone::MiddleEast),
        (&["cape town server"], GeoZone::SouthAfrica),
        (
            &[
                "frankfurt server", "london server", "paris server", "stockholm server",
                "warsaw server", "madrid server", "istanbul server", "eu server", "eu ranked", "emea ranked",
            ],
            GeoZone::Eu,
        ),
    ];
    for (keys, zone) in IN_GAME_SERVER_MAP {
        if keys.iter().any(|k| has_ascii_word(&lower, k)) {
            return RegionEvidence::InGameServer(*zone);
        }
    }

    // 2. Secondary priority: explicit regional / broadcast location cues
    const INDIA_KEYS: &[&str] = &["mumbai", "india", "indian", "hindi", "hinglish", "ist", "delhi", "hyderabad"];
    if INDIA_KEYS.iter().any(|k| has_ascii_word(&lower, k)) || lower.contains("[in]") || lower.contains("(in)") {
        return RegionEvidence::StreamingRegion(GeoZone::India);
    }
    const JAPAN_KEYS: &[&str] = &["tokyo", "japan", "japanese", "jpn", "jst", "nihon"];
    if JAPAN_KEYS.iter().any(|k| has_ascii_word(&lower, k)) || lower.contains("[jp]") || lower.contains("(jp)") {
        return RegionEvidence::StreamingRegion(GeoZone::Japan);
    }
    const SEA_KEYS: &[&str] = &[
        "manila", "philippines", "pinoy", "tagalog", "filipino", "singapore", "hong kong",
        "indonesia", "indo", "malaysia", "vietnam", "thailand", "pht",
    ];
    if SEA_KEYS.iter().any(|k| has_ascii_word(&lower, k))
        || lower.contains("[ph]")
        || lower.contains("[sg]")
        || lower.contains("[sea]")
    {
        return RegionEvidence::StreamingRegion(GeoZone::Sea);
    }
    const KOREA_KEYS: &[&str] = &["seoul", "korea", "korean", "kst"];
    if KOREA_KEYS.iter().any(|k| has_ascii_word(&lower, k)) || lower.contains("[kr]") || lower.contains("(kr)") {
        return RegionEvidence::StreamingRegion(GeoZone::Korea);
    }
    const OCE_KEYS: &[&str] = &["sydney", "australia", "aussie", "new zealand"];
    if OCE_KEYS.iter().any(|k| has_ascii_word(&lower, k)) || lower.contains("[oce]") {
        return RegionEvidence::StreamingRegion(GeoZone::Oce);
    }
    const NA_KEYS: &[&str] = &[
        "usa", "america", "american", "texas", "austin", "dallas", "chicago", "illinois",
        "virginia", "georgia", "california", "oregon", "new york", "canada", "toronto",
        "est", "pst", "cst", "mst", "vct americas",
    ];
    if NA_KEYS.iter().any(|k| has_ascii_word(&lower, k))
        || lower.contains("[na]")
        || lower.contains("(na)")
        || lower.contains("[us]")
    {
        return RegionEvidence::StreamingRegion(GeoZone::Na);
    }
    const BR_KEYS: &[&str] = &["brasil", "brazil", "sao paulo", "portugues", "brt"];
    if BR_KEYS.iter().any(|k| has_ascii_word(&lower, k)) || lower.contains("[br]") {
        return RegionEvidence::StreamingRegion(GeoZone::Brazil);
    }
    const LATAM_KEYS: &[&str] = &["latam", "mexico", "bogota", "santiago", "argentina", "chile", "colombia", "peru"];
    if LATAM_KEYS.iter().any(|k| has_ascii_word(&lower, k)) {
        return RegionEvidence::StreamingRegion(GeoZone::Latam);
    }
    const ME_KEYS: &[&str] = &["bahrain", "dubai", "mena", "saudi", "uae", "middle east"];
    if ME_KEYS.iter().any(|k| has_ascii_word(&lower, k)) {
        return RegionEvidence::StreamingRegion(GeoZone::MiddleEast);
    }
    const ZA_KEYS: &[&str] = &["cape town", "south africa"];
    if ZA_KEYS.iter().any(|k| has_ascii_word(&lower, k)) {
        return RegionEvidence::StreamingRegion(GeoZone::SouthAfrica);
    }
    const EU_KEYS: &[&str] = &[
        "eu", "euw", "emea", "europe", "frankfurt", "london", "paris", "stockholm", "warsaw",
        "madrid", "istanbul", "turkey", "turkish", "germany", "german", "france", "french",
        "poland", "polish", "spain", "uk", "british", "cet", "cest", "bst",
    ];
    if EU_KEYS.iter().any(|k| has_ascii_word(&lower, k)) || lower.contains("[eu]") || lower.contains("(eu)") {
        return RegionEvidence::StreamingRegion(GeoZone::Eu);
    }

    // 3. Lowest priority: unambiguous non-Latin script or broadcast language code (English is never assumed)
    for ch in text.chars() {
        match ch {
            '\u{0900}'..='\u{097F}'
            | '\u{0980}'..='\u{09FF}'
            | '\u{0B80}'..='\u{0BFF}'
            | '\u{0C00}'..='\u{0C7F}'
            | '\u{0D00}'..='\u{0D7F}' => return RegionEvidence::LanguageHint(GeoZone::India),
            '\u{3040}'..='\u{30FF}' => return RegionEvidence::LanguageHint(GeoZone::Japan),
            '\u{AC00}'..='\u{D7AF}' => return RegionEvidence::LanguageHint(GeoZone::Korea),
            '\u{0E00}'..='\u{0E7F}' => return RegionEvidence::LanguageHint(GeoZone::Sea),
            '\u{0600}'..='\u{06FF}' => return RegionEvidence::LanguageHint(GeoZone::MiddleEast),
            '\u{0400}'..='\u{04FF}' => return RegionEvidence::LanguageHint(GeoZone::Eu),
            _ => {}
        }
    }
    if let Some(lang) = lang_code {
        match lang.trim().to_ascii_lowercase().as_str() {
            "hi" | "ta" | "te" | "ml" | "bn" | "mr" | "pa" => return RegionEvidence::LanguageHint(GeoZone::India),
            "ja" => return RegionEvidence::LanguageHint(GeoZone::Japan),
            "ko" => return RegionEvidence::LanguageHint(GeoZone::Korea),
            "tl" | "fil" | "th" | "vi" | "id" | "ms" => return RegionEvidence::LanguageHint(GeoZone::Sea),
            "pt" => return RegionEvidence::LanguageHint(GeoZone::Brazil),
            "ar" => return RegionEvidence::LanguageHint(GeoZone::MiddleEast),
            "de" | "fr" | "pl" | "ru" | "tr" | "it" | "sv" | "nl" | "uk" | "cs" | "ro" | "hu" => {
                return RegionEvidence::LanguageHint(GeoZone::Eu)
            }
            _ => {}
        }
    }

    RegionEvidence::Unknown
}

fn detect_text_geo_zone(text: &str, lang_code: Option<&str>) -> Option<GeoZone> {
    classify_region_evidence(text, lang_code).zone()
}

fn is_generic_ign(norm: &str) -> bool {
    const GENERIC: &[&str] = &[
        "valorant", "player", "gamer", "gaming", "stream", "streamer", "live", "official", "real",
        "jett", "reyna", "omen", "sova", "sage", "clove", "iso", "raze", "viper", "cypher", "chamber",
        "phoenix", "neon", "yoru", "skye", "fade", "gekko", "deadlock", "breach", "harbor", "astra",
        "killjoy", "brimstone", "kayo", "vyse", "tejo", "waylay", "shadow", "ghost", "king", "queen",
        "alpha", "omega", "dark", "angel", "demon", "ninja", "sniper", "clutch", "radiant", "immortal",
        "diamond", "gold", "silver", "iron", "bronze", "vandal", "phantom", "operator", "sheriff",
        "ace", "smurf", "carry", "toxic", "chill", "vibe", "zero", "hero", "nova", "apex", "echo",
    ];
    GENERIC.contains(&norm)
}

fn looks_like_valorant_stream(title: &str, channel: &str) -> bool {
    let combined = format!("{title} {channel}").to_ascii_lowercase();
    const VAL_HINTS: &[&str] = &[
        "valorant", "valo", "vct", "radiant", "immortal", "ascendant", "premier", "ranked", "comp",
        "ascent", "bind", "haven", "split", "icebox", "breeze", "fracture", "pearl", "lotus", "sunset",
        "abyss", "corrode", "summit", "jett", "reyna", "clove", "omen", "sova", "cypher", "chamber", "raze",
    ];
    VAL_HINTS.iter().any(|k| has_ascii_word(&combined, k))
}

/// Multi-signal heuristic scorer. Never promotes heuristic matches to `Confirmed` (which requires
/// verified database match + compatible server, or direct match-chat link). Rejects incompatible
/// regions (e.g., NA/US streamer while user is playing on Mumbai server) and marks unknown regions
/// strictly as `Unverified`.
fn score_heuristic_candidate(
    platform: &str,
    cleaned_ign: &str,
    channel: &str,
    title: &str,
    url: &str,
    lang_code: Option<&str>,
    game_confirmed: bool,
    region: &ServerRegionInfo,
) -> Option<LikelyStreamerCandidate> {
    let ign_norm = normalize(cleaned_ign);
    let chan_norm = normalize(channel);
    if ign_norm.chars().count() < 4 || chan_norm.is_empty() || is_generic_ign(&ign_norm) {
        return None;
    }

    if !game_confirmed && !looks_like_valorant_stream(title, channel) {
        return None;
    }

    // 1. Username-to-channel signal (similar usernames alone cannot confirm a match)
    let is_exact_name = chan_norm == ign_norm;
    let (mut score, name_reason): (i32, &str) = if is_exact_name {
        (46, "Exact channel handle")
    } else if chan_norm.starts_with(&ign_norm) || chan_norm.ends_with(&ign_norm) {
        let ratio = (ign_norm.len() * 100) / chan_norm.len().max(1);
        if ratio < 55 {
            return None;
        }
        (36, "Close handle match")
    } else if chan_norm.contains(&ign_norm) {
        let ratio = (ign_norm.len() * 100) / chan_norm.len().max(1);
        if ign_norm.len() < 5 || ratio < 55 {
            return None;
        }
        (28, "Channel contains IGN")
    } else if ign_norm.contains(&chan_norm) && chan_norm.len() >= 5 {
        let ratio = (chan_norm.len() * 100) / ign_norm.len().max(1);
        if ratio < 60 {
            return None;
        }
        (28, "IGN matches channel")
    } else {
        return None;
    };

    // 2. Game category signal
    if game_confirmed {
        score += 14;
    } else if has_ascii_word(&title.to_ascii_lowercase(), "valorant") {
        score += 8;
    }

    // 3. Server-priority region compatibility (InGameServer > StreamingRegion > LanguageHint > Unknown)
    let combined_text = format!("{title} {channel}");
    let evidence = classify_region_evidence(&combined_text, lang_code);
    let (tier, region_hint, server_status, region_note) = match evidence {
        RegionEvidence::InGameServer(dz) if dz == region.zone => {
            score += 28;
            (
                if score >= 74 { ConfidenceTier::Probable } else { ConfidenceTier::Unverified },
                geo_zone_label(dz).to_string(),
                "In-Game Server Match".to_string(),
                format!("Explicit {} server cue", region.short_label),
            )
        }
        RegionEvidence::InGameServer(dz) if zones_compatible(region.zone, dz) => {
            score += 18;
            (
                if score >= 74 { ConfidenceTier::Probable } else { ConfidenceTier::Unverified },
                geo_zone_label(dz).to_string(),
                "Compatible Nearby Server".to_string(),
                format!("Nearby server ({})", geo_zone_label(dz)),
            )
        }
        RegionEvidence::StreamingRegion(dz) if dz == region.zone => {
            score += 20;
            (
                if score >= 72 && is_exact_name { ConfidenceTier::Probable } else { ConfidenceTier::Unverified },
                geo_zone_label(dz).to_string(),
                "Streaming Region Match".to_string(),
                format!("Streaming region matches {}", region.short_label),
            )
        }
        RegionEvidence::StreamingRegion(dz) if zones_compatible(region.zone, dz) => {
            score += 12;
            (
                if score >= 72 && is_exact_name { ConfidenceTier::Probable } else { ConfidenceTier::Unverified },
                geo_zone_label(dz).to_string(),
                "Compatible Streaming Region".to_string(),
                format!("Compatible region ({})", geo_zone_label(dz)),
            )
        }
        RegionEvidence::LanguageHint(dz) if dz == region.zone || zones_compatible(region.zone, dz) => {
            // Language alone is never treated as proof of in-game server; cap below Probable unless exact + confirmed game
            score += 8;
            (
                ConfidenceTier::Unverified,
                format!("{} (Language hint only)", geo_zone_label(dz)),
                "Server Unverified (Language Hint)".to_string(),
                "Broadcast language matches region (server unverified)".to_string(),
            )
        }
        RegionEvidence::InGameServer(_conflict)
        | RegionEvidence::StreamingRegion(_conflict)
        | RegionEvidence::LanguageHint(_conflict) => {
            // Incompatible continent/server (e.g., NA/US streamer while playing on Mumbai) -> reject
            return None;
        }
        RegionEvidence::Unknown => {
            // Unknown server/region: NEVER confirm; surface as Unverified only for exact handle +
            // confirmed VALORANT category or explicit "valorant"/"valo" in the live stream title.
            let title_low = title.to_ascii_lowercase();
            let title_mentions_val =
                has_ascii_word(&title_low, "valorant") || has_ascii_word(&title_low, "valo");
            if !is_exact_name || (!game_confirmed && !title_mentions_val) {
                return None;
            }
            score = score.min(58);
            (
                ConfidenceTier::Unverified,
                "Server Unknown".to_string(),
                "Server Unverified".to_string(),
                "Streamer server could not be verified".to_string(),
            )
        }
    };

    let final_conf = match tier {
        ConfidenceTier::Probable => score.clamp(68, 88) as u8,
        ConfidenceTier::Unverified => score.clamp(42, 62) as u8,
        ConfidenceTier::Confirmed => score.clamp(90, 100) as u8,
    };
    if final_conf < 48 {
        return None;
    }

    let match_reason = if game_confirmed {
        format!("{name_reason} + VALORANT live + {region_note}")
    } else {
        format!("{name_reason} + {region_note}")
    };

    Some(LikelyStreamerCandidate {
        platform: platform.to_string(),
        channel: channel.to_string(),
        title: title.to_string(),
        url: url.to_string(),
        confidence: final_conf,
        confidence_tier: tier.as_str().to_string(),
        region_hint,
        server_match_status: server_status,
        match_reason,
    })
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
    my_agent: String,
    server_region: Option<String>,
    players: Vec<MatchPlayerInfo>,
}

#[derive(Debug, Clone)]
struct MatchPlayerInfo {
    record: RiotNameRecord,
    team_relation: String,
    agent_name: String,
    party_id: String,
    is_party_member: bool,
}

/// Returns true only for real Riot party UUIDs (rejects blank strings and nil all-zero UUIDs
/// such as `00000000-0000-0000-0000-000000000000` that solo/FFA players can share).
fn is_valid_party_id(party_id: &str) -> bool {
    let trimmed = party_id.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.chars().any(|c| c != '0' && c != '-')
}

/// Returns true if a Riot GameName contains at least one visible non-whitespace character
/// (filters out empty strings and invisible filler names such as Hangul Filler U+3164).
fn is_visible_game_name(name: &str) -> bool {
    name.chars().any(|c| {
        !c.is_whitespace()
            && !c.is_control()
            && !matches!(
                c,
                '\u{115F}'
                    | '\u{1160}'
                    | '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{206F}'
                    | '\u{2800}'
                    | '\u{3164}'
                    | '\u{FEFF}'
                    | '\u{FFA0}'
            )
    })
}

/// Null-tolerant parser for Riot's `name-service/v2/players` response so a `null` field on
/// one player never breaks deserialization for the rest of the lobby, while supporting
/// PascalCase, camelCase, and `DisplayName` fallbacks.
fn parse_name_service_records(val: &Value) -> Vec<RiotNameRecord> {
    let Some(arr) = val.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let subject = item["Subject"]
            .as_str()
            .or_else(|| item["subject"].as_str())
            .or_else(|| item["puuid"].as_str())
            .or_else(|| item["Puuid"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if subject.is_empty() {
            continue;
        }
        let mut game_name = item["GameName"]
            .as_str()
            .or_else(|| item["gameName"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let mut tag_line = item["TagLine"]
            .as_str()
            .or_else(|| item["tagLine"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if !is_visible_game_name(&game_name) {
            if let Some(disp) = item["DisplayName"]
                .as_str()
                .or_else(|| item["displayName"].as_str())
                .map(str::trim)
                .filter(|s| is_visible_game_name(s))
            {
                if let Some((gn, tl)) = disp.split_once('#') {
                    if is_visible_game_name(gn.trim()) {
                        game_name = gn.trim().to_string();
                        if tag_line.is_empty() {
                            tag_line = tl.trim().to_string();
                        }
                    }
                } else {
                    game_name = disp.to_string();
                }
            }
        }
        out.push(RiotNameRecord {
            subject,
            game_name,
            tag_line,
        });
    }
    out
}

/// Builds the complete match roster from `match-details["players"]` first, enriching names from
/// `name-service/v2/players` as best-effort so partial or failed name-service responses NEVER drop
/// players, and enforces strict same-team + non-nil + <=5-member party verification.
fn resolve_match_players(
    details_players: &[Value],
    name_records: &[RiotNameRecord],
    my_subject: &str,
) -> (String, Vec<MatchPlayerInfo>) {
    let my_sub_low = my_subject.trim().to_ascii_lowercase();

    let mut name_by_sub: HashMap<String, &RiotNameRecord> = HashMap::new();
    for rec in name_records {
        let key = rec.subject.trim().to_ascii_lowercase();
        if !key.is_empty() {
            name_by_sub.insert(key, rec);
        }
    }

    struct RawPlayer {
        subject: String,
        sub_low: String,
        game_name: String,
        tag_line: String,
        team: String,
        agent: String,
        party_id: String,
    }

    let mut raw_players: Vec<RawPlayer> = Vec::with_capacity(details_players.len());
    for p in details_players {
        let Some(subj) = p["subject"].as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let sub_low = subj.to_ascii_lowercase();
        let details_name = p["gameName"]
            .as_str()
            .or_else(|| p["GameName"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let details_tag = p["tagLine"]
            .as_str()
            .or_else(|| p["TagLine"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let (game_name, tag_line) = match name_by_sub.get(&sub_low) {
            Some(rec) => {
                let gn = if is_visible_game_name(&rec.game_name) {
                    rec.game_name.trim().to_string()
                } else {
                    details_name
                };
                let tl = if !rec.tag_line.trim().is_empty() {
                    rec.tag_line.trim().to_string()
                } else {
                    details_tag
                };
                (gn, tl)
            }
            None => (details_name, details_tag),
        };
        let team = p["teamId"].as_str().unwrap_or("").trim().to_string();
        let agent = agent_name_from_id(p["characterId"].as_str().unwrap_or(""));
        let party_id = p["partyId"].as_str().unwrap_or("").trim().to_string();

        raw_players.push(RawPlayer {
            subject: subj.to_string(),
            sub_low,
            game_name,
            tag_line,
            team,
            agent,
            party_id,
        });
    }

    let (my_team, my_agent, my_party_id) = raw_players
        .iter()
        .find(|p| p.sub_low == my_sub_low)
        .map(|p| (p.team.clone(), p.agent.clone(), p.party_id.clone()))
        .unwrap_or_default();

    // Guard against dummy server-wide partyIds: a real Valorant party has at most 5 players.
    let my_party_valid = is_valid_party_id(&my_party_id);
    let same_party_count = if my_party_valid {
        raw_players
            .iter()
            .filter(|p| p.party_id.eq_ignore_ascii_case(&my_party_id))
            .count()
    } else {
        0
    };
    let party_check_active = my_party_valid
        && same_party_count <= 5
        && !my_team.is_empty()
        && !my_team.eq_ignore_ascii_case("Neutral");

    let players = raw_players
        .into_iter()
        .map(|p| {
            let same_team = !my_team.is_empty()
                && !p.team.is_empty()
                && !p.team.eq_ignore_ascii_case("Neutral")
                && p.team.eq_ignore_ascii_case(&my_team);
            let team_relation = if !my_team.is_empty()
                && !p.team.is_empty()
                && !p.team.eq_ignore_ascii_case("Neutral")
            {
                if same_team {
                    "Ally".to_string()
                } else {
                    "Enemy".to_string()
                }
            } else {
                String::new()
            };
            // Only mark as party member if partyId is valid non-nil, party size <= 5,
            // player is not self, and player is on the same non-Neutral team.
            let is_party_member = party_check_active
                && same_team
                && p.sub_low != my_sub_low
                && p.party_id.eq_ignore_ascii_case(&my_party_id);
            MatchPlayerInfo {
                record: RiotNameRecord {
                    subject: p.subject,
                    game_name: p.game_name,
                    tag_line: p.tag_line,
                },
                team_relation,
                agent_name: p.agent,
                party_id: p.party_id,
                is_party_member,
            }
        })
        .collect();

    (my_agent, players)
}

/// Excludes self, duplicates, (when enabled) party members, and hidden/blank Riot IDs
/// before external scans, returning `(scannable, party_skipped, hidden_skipped)`.
fn filter_scannable_players(
    players: Vec<MatchPlayerInfo>,
    my_puuid: &str,
    skip_party_members: bool,
) -> (Vec<MatchPlayerInfo>, usize, usize) {
    let mut seen = HashSet::new();
    let mut scannable = Vec::new();
    let mut party_skipped = 0usize;
    let mut hidden_skipped = 0usize;
    let my_low = my_puuid.trim().to_ascii_lowercase();
    for player_info in players {
        let sub_low = player_info.record.subject.trim().to_ascii_lowercase();
        if sub_low == my_low || !seen.insert(sub_low) {
            continue;
        }
        if skip_party_members && player_info.is_party_member {
            party_skipped += 1;
            continue;
        }
        if !is_visible_game_name(&player_info.record.game_name) {
            hidden_skipped += 1;
            continue;
        }
        scannable.push(player_info);
    }
    (scannable, party_skipped, hidden_skipped)
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

fn parse_aliases_csv(raw: &str, primary_riot_id: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    seen.insert(primary_riot_id.trim().to_ascii_lowercase());
    let mut out = Vec::new();
    for part in raw.split(',') {
        let a = part.trim();
        if a.is_empty() || a.chars().count() > 24 || !a.contains('#') {
            continue;
        }
        if seen.insert(a.to_ascii_lowercase()) {
            out.push(a.to_string());
        }
    }
    out
}

/// Checks exact Riot ID or verified alias in the community/local streamer database.
fn directory_entry_matches_player(entry: &DirectoryStreamer, riot_id: &str) -> Option<&'static str> {
    if entry.riot_id.eq_ignore_ascii_case(riot_id) {
        return Some("Exact Riot ID in streamer database");
    }
    if entry.aliases.iter().any(|a| a.eq_ignore_ascii_case(riot_id)) {
        return Some("Known streamer alias in database");
    }
    None
}

/// Returns a cleaned entry only if platform + URL host are consistent and safe.
fn sanitize_directory_entry(d: &DirectoryStreamer) -> Option<DirectoryStreamer> {
    let platform = canonical_platform(&d.platform)?;
    let url = d.channel_url.trim();
    if !host_allowed(platform, &host_of(url)?) || !d.riot_id.contains('#') {
        return None;
    }
    let riot_id = d.riot_id.trim().to_string();
    let cname = short(
        if !d.channel_name.trim().is_empty() {
            d.channel_name.trim()
        } else {
            d.display_name.trim()
        },
        64,
    );
    if cname.is_empty() {
        return None;
    }
    let display_name = if d.display_name.trim().is_empty() {
        cname.clone()
    } else {
        short(d.display_name.trim(), 64)
    };
    let mut seen_aliases = HashSet::new();
    seen_aliases.insert(riot_id.to_ascii_lowercase());
    let mut aliases = Vec::new();
    for a in &d.aliases {
        let trimmed = a.trim();
        if !trimmed.is_empty()
            && trimmed.contains('#')
            && trimmed.chars().count() <= 24
            && seen_aliases.insert(trimmed.to_ascii_lowercase())
        {
            aliases.push(trimmed.to_string());
        }
    }
    let channel_id = if d.channel_id.trim().is_empty() {
        last_path_segment(url)
    } else {
        d.channel_id.trim().to_string()
    };
    let id = if d.id.trim().is_empty() {
        format!("{}-{}", normalize(&cname), platform.to_ascii_lowercase())
    } else {
        d.id.trim().to_string()
    };
    Some(DirectoryStreamer {
        id,
        display_name,
        riot_id,
        aliases,
        platform: platform.to_string(),
        channel_id,
        channel_name: cname,
        channel_url: url.to_string(),
        server_region: d.server_region.trim().to_ascii_lowercase(),
        verified: d.verified,
        submitted_at: d.submitted_at.trim().to_string(),
        source: d.source.trim().to_string(),
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

fn valorant_log_path() -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default())
        .join("VALORANT")
        .join("Saved")
        .join("Logs")
        .join("ShooterGame.log")
}

/// Extracts Riot's client version string from `ShooterGame.log` (`CI server version: ...`)
/// as a zero-dependency local fallback if `valorant-api.com` is temporarily unreachable.
fn parse_client_version_from_log_text(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some((_, rest)) = line.split_once("CI server version:") {
            let raw = rest.trim().split_whitespace().next().unwrap_or("").trim();
            if raw.is_empty() {
                continue;
            }
            if raw.contains("-shipping-") {
                return Some(raw.to_string());
            }
            let parts: Vec<&str> = raw.split('-').collect();
            if parts.len() >= 4 && parts[0].eq_ignore_ascii_case("release") {
                return Some(format!("{}-{}-shipping-{}", parts[0], parts[1], parts[2..].join("-")));
            }
            return Some(raw.to_string());
        }
    }
    None
}

fn read_client_version_from_log() -> Option<String> {
    let mut f = File::open(valorant_log_path()).ok()?;
    let mut buf = Vec::new();
    f.take(128 * 1024).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    parse_client_version_from_log_text(&text)
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
        (Err(e), None) => {
            if let Some(local_ver) = read_client_version_from_log() {
                log(format!("Version API failed ({e}); using ShooterGame.log version {local_ver}"));
                *lock(&CLIENT_VERSION) = Some((local_ver.clone(), Instant::now()));
                return Ok(local_ver);
            }
            Err(e)
        }
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
        s if s.contains("plummet") => Some("Summit"),
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

fn queue_name_from_id(queue_id: &str, game_mode: &str) -> String {
    match queue_id.trim().to_ascii_lowercase().as_str() {
        "competitive" => return "Competitive".to_string(),
        "unrated" => return "Unrated".to_string(),
        "swiftplay" => return "Swiftplay".to_string(),
        "spikerush" => return "Spike Rush".to_string(),
        "deathmatch" => return "Deathmatch".to_string(),
        "hurm" => return "Team Deathmatch".to_string(),
        "premier" => return "Premier".to_string(),
        "ggteam" => return "Escalation".to_string(),
        "onefa" => return "Replication".to_string(),
        "snowball" => return "Snowball Fight".to_string(),
        "newmap" => return "New Map".to_string(),
        "" => {}
        other => return other.to_string(),
    }
    let gm = game_mode.to_ascii_lowercase();
    if gm.contains("deathmatch") {
        "Deathmatch".to_string()
    } else if gm.contains("hurm") {
        "Team Deathmatch".to_string()
    } else if gm.contains("quickbomb") {
        "Spike Rush".to_string()
    } else if gm.contains("swiftplay") {
        "Swiftplay".to_string()
    } else if gm.contains("gungame") {
        "Escalation".to_string()
    } else if gm.contains("oneforall") {
        "Replication".to_string()
    } else if gm.contains("snowball") {
        "Snowball Fight".to_string()
    } else {
        "Custom".to_string()
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

/// Null-tolerant parser for Riot's `/chat/v6/messages` response so `null` fields on
/// anonymous/system messages never drop valid chat records.
fn parse_chat_messages(val: &Value) -> Vec<ChatMessage> {
    val["messages"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| ChatMessage {
                    id: v["id"].as_str().unwrap_or("").to_string(),
                    mid: v["mid"].as_str().unwrap_or("").to_string(),
                    cid: v["cid"].as_str().unwrap_or("").to_string(),
                    puuid: v["puuid"].as_str().unwrap_or("").to_string(),
                    game_name: v["game_name"].as_str().unwrap_or("").to_string(),
                    game_tag: v["game_tag"].as_str().unwrap_or("").to_string(),
                    body: v["body"].as_str().unwrap_or("").to_string(),
                    msg_type: v["type"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_local_chat_messages(local: &Client) -> Result<Vec<ChatMessage>> {
    let (port, password, protocol) = read_riot_lockfile()?;
    let auth_header = format!("Basic {}", STANDARD.encode(format!("riot:{password}")));
    let url = format!("{protocol}://127.0.0.1:{port}/chat/v6/messages");

    let resp = local
        .get(&url)
        .header("Authorization", auth_header)
        .header("Accept", "application/json")
        .header("User-Agent", BROWSER_UA)
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow!("Local chat API returned HTTP {}", resp.status().as_u16()));
    }
    let val: Value = resp.json().await?;
    Ok(parse_chat_messages(&val))
}

fn is_match_chat_message(m: &ChatMessage) -> bool {
    if m.body.trim().is_empty() {
        return false;
    }
    let cid = m.cid.to_ascii_lowercase();
    let ty = m.msg_type.to_ascii_lowercase();
    if cid.contains("ares-parties") || cid.contains("ares-lobby") {
        return false;
    }
    cid.contains("ares-coregame") || cid.contains("ares-pregame") || ty == "groupchat"
}

fn chat_msg_key(m: &ChatMessage) -> String {
    if !m.id.is_empty() {
        m.id.clone()
    } else if !m.mid.is_empty() {
        m.mid.clone()
    } else {
        format!("{}:{}", m.puuid, m.body.trim())
    }
}

fn merge_chat_messages(target: &mut Vec<ChatMessage>, incoming: Vec<ChatMessage>) {
    let mut existing: HashSet<String> = target.iter().map(chat_msg_key).collect();
    for m in incoming {
        if !is_match_chat_message(&m) {
            continue;
        }
        let key = chat_msg_key(&m);
        if existing.insert(key) {
            target.push(m);
        }
    }
    if target.len() > 250 {
        let drop_n = target.len() - 250;
        target.drain(0..drop_n);
    }
}

static CHAT_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:https?://)?(?:www\.|m\.)?(?:youtube\.com|youtu\.be|twitch\.tv|kick\.com)/[^\s<>"'\)\]]+"#).unwrap()
});
static CHAT_YT_SHORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:yt|youtube)\s*(?:/|:|@|-)\s*@?([a-z0-9._-]{3,30})\b").unwrap()
});
static CHAT_TW_SHORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:ttv|twitch)\s*(?:/|:|@|-)\s*([a-z0-9_]{3,25})\b").unwrap()
});
static CHAT_KICK_SHORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bkick\s*(?:/|:|@|-)\s*([a-z0-9_-]{3,25})\b").unwrap()
});
static CHAT_AT_HANDLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|\s)@([a-z0-9._-]{3,30})\b").unwrap()
});
static CHAT_LIVE_INTENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:live|streaming|stream|watch|tune\s+in|sub|subscribe|follow|check\s+(?:out\s+)?my|going\s+live|im\s+live|i'm\s+live)\b").unwrap()
});
static CHAT_YT_WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:yt|youtube)\b").unwrap());
static CHAT_TW_WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:ttv|twitch)\b").unwrap());
static CHAT_KICK_WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bkick\b").unwrap());

fn is_chat_stop_word(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "live" | "stream" | "streaming" | "valorant" | "val" | "gaming" | "channel"
            | "link" | "here" | "now" | "guys" | "bro" | "pls" | "please" | "check"
            | "watch" | "sub" | "follow" | "search" | "find" | "look" | "youtube"
            | "twitch" | "kick" | "ttv" | "com" | "www" | "http" | "https" | "results"
            | "shorts" | "playlist" | "videos" | "featured" | "about" | "streams"
    )
}

fn handle_from_stream_url(platform: &str, url: &str) -> Option<String> {
    let clean = strip_query(url).trim_end_matches('/');
    let rest = clean.strip_prefix("https://").or_else(|| clean.strip_prefix("http://"))?;
    let mut parts = rest.split('/').skip(1);
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    match platform {
        "YouTube" => {
            if let Some(h) = first.strip_prefix('@') {
                if h.len() >= 2 {
                    return Some(h.to_string());
                }
            }
            if matches!(first.to_ascii_lowercase().as_str(), "c" | "user" | "channel") && !second.is_empty() {
                return Some(second.to_string());
            }
            None
        }
        "Twitch" | "Kick" => {
            let slug = first.trim_start_matches('@');
            if slug.len() >= 3 && !is_chat_stop_word(slug) {
                Some(slug.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn extract_chat_stream_plug(body: &str) -> Option<ChatStreamPlug> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Priority 3: Direct YouTube / Twitch / Kick URL (with or without https://)
    for m in CHAT_URL_RE.find_iter(trimmed) {
        let raw_link = m.as_str().trim_end_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']'));
        let full_url = if raw_link.to_ascii_lowercase().starts_with("http://")
            || raw_link.to_ascii_lowercase().starts_with("https://")
        {
            raw_link.to_string()
        } else {
            format!("https://{raw_link}")
        };
        let Some(host) = host_of(&full_url) else {
            continue;
        };
        let platform = if host_allowed("YouTube", &host) {
            "YouTube"
        } else if host_allowed("Twitch", &host) {
            "Twitch"
        } else if host_allowed("Kick", &host) {
            "Kick"
        } else {
            continue;
        };
        let handle = handle_from_stream_url(platform, &full_url);
        return Some(ChatStreamPlug {
            platform: platform.to_string(),
            url: Some(full_url),
            handle,
            raw_message: trimmed.to_string(),
            priority: 3,
        });
    }

    // Priority 2: Shorthand like "ttv/vexfps", "yt/@RohanGaming", "kick/kairo"
    for (re, platform) in [
        (&*CHAT_YT_SHORT_RE, "YouTube"),
        (&*CHAT_TW_SHORT_RE, "Twitch"),
        (&*CHAT_KICK_SHORT_RE, "Kick"),
    ] {
        if let Some(caps) = re.captures(trimmed) {
            let handle = caps[1].trim_matches(|c: char| c == '.' || c == '-');
            if handle.len() >= 3 && !is_chat_stop_word(handle) {
                let url = match platform {
                    "YouTube" => format!("https://www.youtube.com/@{handle}"),
                    "Twitch" => format!("https://www.twitch.tv/{handle}"),
                    _ => format!("https://kick.com/{handle}"),
                };
                return Some(ChatStreamPlug {
                    platform: platform.to_string(),
                    url: Some(url),
                    handle: Some(handle.to_string()),
                    raw_message: trimmed.to_string(),
                    priority: 2,
                });
            }
        }
    }

    // Priority 1/2: Live/stream self-promo intent ("im live on yt", "streaming on twitch @name")
    if CHAT_LIVE_INTENT_RE.is_match(trimmed) {
        let platform = if CHAT_YT_WORD_RE.is_match(trimmed) {
            Some("YouTube")
        } else if CHAT_TW_WORD_RE.is_match(trimmed) {
            Some("Twitch")
        } else if CHAT_KICK_WORD_RE.is_match(trimmed) {
            Some("Kick")
        } else {
            None
        };

        if let Some(plat) = platform {
            if let Some(caps) = CHAT_AT_HANDLE_RE.captures(trimmed) {
                let handle = caps[1].trim_matches(|c: char| c == '.' || c == '-');
                if handle.len() >= 3 && !is_chat_stop_word(handle) {
                    let url = match plat {
                        "YouTube" => format!("https://www.youtube.com/@{handle}"),
                        "Twitch" => format!("https://www.twitch.tv/{handle}"),
                        _ => format!("https://kick.com/{handle}"),
                    };
                    return Some(ChatStreamPlug {
                        platform: plat.to_string(),
                        url: Some(url),
                        handle: Some(handle.to_string()),
                        raw_message: trimmed.to_string(),
                        priority: 2,
                    });
                }
            }
            return Some(ChatStreamPlug {
                platform: plat.to_string(),
                url: None,
                handle: None,
                raw_message: trimmed.to_string(),
                priority: 1,
            });
        }
    }

    None
}

/// Pure classifier used to determine live Valorant vs Riot Client status without false positives.
/// Valorant is only reported Active when the actual game process (or active Valorant product session)
/// is running - never when Riot Client is simply open in the tray or running another Riot title.
fn classify_riot_status(valorant_running: bool, riot_client_running: bool, has_lockfile: bool) -> RiotStatus {
    if valorant_running {
        RiotStatus::ValorantActive
    } else if riot_client_running && has_lockfile {
        RiotStatus::RiotClientOnly
    } else {
        RiotStatus::Offline
    }
}

/// Checks Riot's local product-session API for an active `valorant` session (non-Windows fallback).
#[cfg(not(windows))]
async fn has_active_valorant_session(local: &Client) -> bool {
    let Ok((port, password, protocol)) = read_riot_lockfile() else {
        return false;
    };
    let auth = format!("Basic {}", STANDARD.encode(format!("riot:{password}")));
    let url = format!("{protocol}://127.0.0.1:{port}/product-session/v1/external-sessions");
    let Ok(resp) = local
        .get(url)
        .header("Authorization", auth)
        .timeout(Duration::from_secs(2))
        .send()
        .await
    else {
        return false;
    };
    if !resp.status().is_success() {
        return false;
    }
    let Ok(val) = resp.json::<Value>().await else {
        return false;
    };
    if let Some(map) = val.as_object() {
        for (_id, sess) in map {
            let prod = sess["productId"].as_str().unwrap_or("");
            let state = sess["launchConfiguration"]["arguments"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(true);
            if prod.eq_ignore_ascii_case("valorant") && state {
                return true;
            }
        }
    }
    false
}

async fn probe_riot_session(local: &Client) -> RiotStatus {
    let has_lockfile = read_riot_lockfile().is_ok();
    #[cfg(windows)]
    {
        let _ = local;
        // Direct kernel32 process snapshot: instant, zero network calls, zero false positives
        // when Riot Client is idling in the tray or running League/TFT, or after a stale lockfile.
        let (valorant_running, riot_client_running) = win32_window::detect_riot_processes();
        classify_riot_status(valorant_running, riot_client_running, has_lockfile)
    }
    #[cfg(not(windows))]
    {
        if !has_lockfile {
            return RiotStatus::Offline;
        }
        let valorant_running = has_active_valorant_session(local).await;
        let riot_client_running = valorant_running || fetch_local_entitlements(local).await.is_ok();
        classify_riot_status(valorant_running, riot_client_running, has_lockfile)
    }
}

async fn riot_authenticate(local: &Client, public: &Client) -> Result<RiotAuth> {
    // Re-read lockfile across brief retries to survive Riot Client startup / port rotation races.
    let ent = retry(2, Duration::from_millis(1200), || fetch_local_entitlements(local)).await?;
    let version = client_version(public).await?;
    Ok(RiotAuth { access_token: ent.access_token, token: ent.token, subject: ent.subject, version })
}

async fn latest_match_id(public: &Client, auth: &RiotAuth, api_region: &str) -> Result<String> {
    let url = format!(
        "https://pd.{api_region}.a.pvp.net/match-history/v1/history/{}?startIndex=0&endIndex=1",
        auth.subject
    );
    let v = riot_json(riot_headers(public.get(url), auth)).await?;
    v["History"][0]["MatchID"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("No recent match found on {api_region} server region"))
}

async fn fetch_match_context(
    public: &Client,
    auth: &RiotAuth,
    api_region: &str,
    match_id: &str,
) -> Result<MatchContext> {
    let url = format!("https://pd.{api_region}.a.pvp.net/match-details/v1/matches/{match_id}");
    let details = riot_json(riot_headers(public.get(url), auth)).await?;

    let map_name = map_name_from_id(details["matchInfo"]["mapId"].as_str().unwrap_or(""));
    let queue_name = queue_name_from_id(
        details["matchInfo"]["queueID"].as_str().unwrap_or(""),
        details["matchInfo"]["gameMode"].as_str().unwrap_or(""),
    );
    let server_region = details["matchInfo"]["gamePodId"]
        .as_str()
        .and_then(parse_game_pod_to_server)
        .map(String::from);

    let details_players = details["players"]
        .as_array()
        .filter(|arr| !arr.is_empty())
        .ok_or_else(|| anyhow!("Match details contained no players yet"))?;

    let puuids: Vec<String> = details_players
        .iter()
        .filter_map(|p| p["subject"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from))
        .collect();
    if puuids.is_empty() {
        return Err(anyhow!("Match details contained no player subjects yet"));
    }

    // Best-effort name-service enrichment with null-tolerant parsing + retry + targeted follow-up
    // for any players whose names were blank/hidden in match-details.
    let name_url = format!("https://pd.{api_region}.a.pvp.net/name-service/v2/players");
    let mut records: Vec<RiotNameRecord> = match retry(2, Duration::from_millis(900), || {
        riot_json(riot_headers(public.put(&name_url).json(&puuids), auth))
    })
    .await
    {
        Ok(names_val) => {
            let recs = parse_name_service_records(&names_val);
            if recs.len() < puuids.len() {
                log(format!(
                    "name-service returned {}/{} players; filling remaining names from match-details",
                    recs.len(),
                    puuids.len()
                ));
            }
            recs
        }
        Err(e) => {
            log(format!("name-service unavailable ({e}); using match-details player names directly"));
            Vec::new()
        }
    };

    let (mut my_agent, mut players) = resolve_match_players(details_players, &records, &auth.subject);

    // Targeted second pass for any players still missing a visible Riot ID
    let missing_puuids: Vec<String> = players
        .iter()
        .filter(|p| !is_visible_game_name(&p.record.game_name))
        .map(|p| p.record.subject.clone())
        .collect();
    if !missing_puuids.is_empty() {
        if let Ok(extra_val) = riot_json(riot_headers(public.put(&name_url).json(&missing_puuids), auth)).await {
            let extra_recs = parse_name_service_records(&extra_val);
            if !extra_recs.is_empty() {
                records.extend(extra_recs);
                let (re_agent, re_players) = resolve_match_players(details_players, &records, &auth.subject);
                my_agent = re_agent;
                players = re_players;
            }
        }
    }

    Ok(MatchContext {
        match_id: match_id.to_string(),
        map_name,
        queue_name,
        my_agent,
        server_region,
        players,
    })
}

/// For post-match scans the latest MatchID may still be the *previous* match while history
/// propagates, so poll until it changes instead of trusting a fixed sleep. When running in Auto
/// mode and the initial shard has no history (e.g., NA/EU/KR user on first run before the log
/// records a PD URL), automatically probes the other Riot shards once to find the active shard.
async fn fetch_match_and_names(
    local: &Client,
    public: &Client,
    api_region: &str,
    allow_region_fallback: bool,
    previous: Option<&str>,
    wait_for_new: bool,
) -> Result<(String, String, MatchContext)> {
    let auth = riot_authenticate(local, public).await?;
    let mut active_region = api_region.to_string();

    // Fast shard discovery on Auto mode if the initial shard has no match history at all
    if allow_region_fallback && latest_match_id(public, &auth, &active_region).await.is_err() {
        for cand in ["na", "eu", "ap", "kr"] {
            if cand.eq_ignore_ascii_case(&active_region) {
                continue;
            }
            if latest_match_id(public, &auth, cand).await.is_ok() {
                log(format!("Auto-discovered active Riot shard '{cand}' (was '{active_region}')"));
                active_region = cand.to_string();
                break;
            }
        }
    }

    let attempts = if wait_for_new { 8 } else { 2 };
    let match_id = retry(attempts, Duration::from_secs(3), || async {
        let id = latest_match_id(public, &auth, &active_region).await?;
        if wait_for_new && Some(id.as_str()) == previous {
            return Err(anyhow!("Match history has not updated yet"));
        }
        Ok(id)
    })
    .await?;
    let ctx = retry(3, Duration::from_secs(3), || {
        fetch_match_context(public, &auth, &active_region, &match_id)
    })
    .await?;
    Ok((auth.subject, active_region, ctx))
}

// ============================================================================
// STREAMING PLATFORM DETECTORS (YOUTUBE, TWITCH, KICK)
// ============================================================================

static PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Unambiguous leading stream tags ("TTV Jake", "YT_Rohan", "twitch.tv/shroud", "Kick-Kairo").
    Regex::new(r"(?i)^(?:ttv|twitch(?:\.tv)?|yt|youtube|kick)[\s._/-]+").unwrap()
});
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
    let trimmed = game_name.trim();
    let without_prefix = PREFIX_RE.replace(trimmed, "");
    let without_suffix = SUFFIX_RE.replace(without_prefix.trim(), "");
    let cleaned = without_suffix.trim().to_string();
    if cleaned.is_empty() {
        trimmed.to_string()
    } else {
        cleaned
    }
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

async fn http_get_text_status_lang(
    client: &Client,
    url: &str,
    accept_lang: &str,
) -> Result<(reqwest::StatusCode, String)> {
    let mut req = client
        .get(url)
        .header("User-Agent", BROWSER_UA)
        .header("Accept-Language", accept_lang);
    if host_of(url)
        .map(|h| {
            let h = h.trim_start_matches("www.").trim_start_matches("m.");
            h == "youtube.com" || h == "youtu.be"
        })
        .unwrap_or(false)
    {
        // Bypass YouTube EU/UK GDPR consent interstitial so keyless YouTube checks work globally.
        req = req.header("Cookie", "SOCS=CAI");
    }
    let resp = req.send().await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    Ok((status, body))
}

async fn http_get_text_status(client: &Client, url: &str) -> Result<(reqwest::StatusCode, String)> {
    http_get_text_status_lang(client, url, "en-US,en;q=0.9").await
}

async fn http_get_text_with_lang(client: &Client, url: &str, accept_lang: &str) -> Option<String> {
    let (status, body) = http_get_text_status_lang(client, url, accept_lang).await.ok()?;
    if !status.is_success() {
        return None;
    }
    Some(body)
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

// ============================================================================
// SELF-UPDATER (GITHUB RELEASES + SHA-256 VERIFICATION)
// ============================================================================

static RELEASE_SHA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)commit\s+`?([0-9a-f]{7,40})`?").unwrap());
static HEX_SHA256_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b([0-9a-f]{64})\b").unwrap());

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    bytes_to_hex(&hasher.finalize())
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(bytes_to_hex(&hasher.finalize()))
}

fn is_allowed_github_url(url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    host == "github.com" || host.ends_with(".githubusercontent.com")
}

fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let s = raw.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn has_newer_release(
    current_ver: &str,
    remote_tag: &str,
    expected_sha256: Option<&str>,
    current_exe_sha: Option<&str>,
    remote_sha: Option<&str>,
    current_build_sha: &str,
) -> bool {
    let semver_newer = match (parse_semver(current_ver), parse_semver(remote_tag)) {
        (Some(cur), Some(rem)) => rem > cur,
        _ => false,
    };
    let semver_older = match (parse_semver(current_ver), parse_semver(remote_tag)) {
        (Some(cur), Some(rem)) => rem < cur,
        _ => false,
    };

    if semver_newer {
        true
    } else if semver_older {
        false
    } else if let (Some(exp), Some(cur)) = (expected_sha256, current_exe_sha) {
        !exp.eq_ignore_ascii_case(cur)
    } else if let Some(rsha) = remote_sha {
        current_build_sha != "local" && !rsha.eq_ignore_ascii_case(current_build_sha)
    } else {
        false
    }
}

fn clean_release_changelog(raw_body: &str) -> String {
    let lines: Vec<String> = raw_body
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.trim().is_empty())
        .take(40)
        .collect();
    if lines.is_empty() {
        "No release notes published for this build.".to_string()
    } else {
        lines.join("\n")
    }
}

async fn check_for_app_update(client: &Client) -> UpdateState {
    let api_url = format!(
        "https://api.github.com/repos/{UPDATE_REPO_OWNER}/{UPDATE_REPO_NAME}/releases/latest"
    );
    let resp = match client
        .get(&api_url)
        .header("User-Agent", format!("ValorantStreamerRadar/{APP_VERSION}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return UpdateState::Error(format!("Could not reach GitHub: {}", short(&e.to_string(), 85))),
    };

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return UpdateState::Error(
            "No published GitHub release is available yet. Could not verify update status.".into(),
        );
    }
    if resp.status() == reqwest::StatusCode::FORBIDDEN || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return UpdateState::Error(
            "GitHub API rate limit reached. Try again later or open the Releases page.".into(),
        );
    }
    if !resp.status().is_success() {
        return UpdateState::Error(format!(
            "GitHub Releases check failed (HTTP {}).",
            resp.status().as_u16()
        ));
    }

    let v: Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => return UpdateState::Error(format!("Invalid GitHub response: {}", short(&e.to_string(), 75))),
    };

    let tag_name = v["tag_name"].as_str().unwrap_or("latest").to_string();
    let release_name = v["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&tag_name)
        .to_string();
    let html_url = v["html_url"]
        .as_str()
        .filter(|u| is_allowed_github_url(u))
        .map(String::from)
        .unwrap_or_else(|| format!("https://github.com/{UPDATE_REPO_OWNER}/{UPDATE_REPO_NAME}/releases"));
    let published_at = v["published_at"]
        .as_str()
        .unwrap_or("")
        .get(..10)
        .unwrap_or("")
        .to_string();
    let body = v["body"].as_str().unwrap_or("");
    let changelog = clean_release_changelog(body);
    let remote_sha = RELEASE_SHA_RE
        .captures(body)
        .map(|c| c[1].chars().take(7).collect::<String>());

    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let exe_asset = assets.iter().find(|a| {
        a["name"]
            .as_str()
            .map(|n| n.eq_ignore_ascii_case("ValorantStreamerRadar.exe") || n.to_ascii_lowercase().ends_with(".exe"))
            .unwrap_or(false)
    });

    let Some(exe_asset) = exe_asset else {
        return UpdateState::Error("Latest GitHub release has no Windows .exe asset yet.".into());
    };

    let download_url = exe_asset["browser_download_url"].as_str().unwrap_or("").to_string();
    if !is_allowed_github_url(&download_url) {
        return UpdateState::Error("Blocked untrusted release download URL.".into());
    }
    let size_bytes = exe_asset["size"].as_u64().unwrap_or(0);

    // Try GitHub's native asset digest first, then fall back to the uploaded .sha256 file.
    let mut expected_sha256: Option<String> = exe_asset["digest"]
        .as_str()
        .and_then(|d| HEX_SHA256_RE.captures(d))
        .map(|c| c[1].to_ascii_lowercase());

    if expected_sha256.is_none() {
        if let Some(sha_asset) = assets.iter().find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.to_ascii_lowercase().ends_with(".sha256"))
                .unwrap_or(false)
        }) {
            let sha_url = sha_asset["browser_download_url"].as_str().unwrap_or("");
            if is_allowed_github_url(sha_url) {
                if let Ok(r) = client
                    .get(sha_url)
                    .header("User-Agent", format!("ValorantStreamerRadar/{APP_VERSION}"))
                    .send()
                    .await
                {
                    if r.status().is_success() {
                        if let Ok(txt) = r.text().await {
                            expected_sha256 = HEX_SHA256_RE
                                .captures(&txt)
                                .map(|c| c[1].to_ascii_lowercase());
                        }
                    }
                }
            }
        }
    }

    let current_exe_sha = env::current_exe().ok().and_then(|p| sha256_file(&p));
    let has_update = has_newer_release(
        APP_VERSION,
        &tag_name,
        expected_sha256.as_deref(),
        current_exe_sha.as_deref(),
        remote_sha.as_deref(),
        APP_BUILD_SHA,
    );

    if has_update {
        UpdateState::Available(UpdateReleaseInfo {
            tag_name,
            release_name,
            changelog,
            download_url,
            html_url,
            expected_sha256,
            published_at,
            size_bytes,
            remote_sha,
        })
    } else {
        let build_note = match &remote_sha {
            Some(s) => format!("v{APP_VERSION} (build {s})"),
            None => format!("v{APP_VERSION} ({APP_BUILD_SHA})"),
        };
        UpdateState::UpToDate(format!("You are up to date on {build_note}."))
    }
}

#[cfg(windows)]
fn spawn_windows_self_replace(staged_exe: &Path, current_exe: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let pid = std::process::id();
    let esc = |p: &Path| p.to_string_lossy().replace('\'', "''");
    let ps_script = format!(
        "$p = {pid}; \
         $src = '{}'; \
         $dst = '{}'; \
         for ($i = 0; $i -lt 40; $i++) {{ \
           if (-not (Get-Process -Id $p -ErrorAction SilentlyContinue)) {{ break }} \
           Start-Sleep -Milliseconds 250 \
         }}; \
         Start-Sleep -Milliseconds 300; \
         Move-Item -LiteralPath $src -Destination $dst -Force; \
         Start-Process -FilePath $dst",
        esc(staged_exe),
        esc(current_exe),
    );
    let utf16: Vec<u8> = ps_script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = STANDARD.encode(utf16);

    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("Failed to launch Windows updater helper")?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_windows_self_replace(_staged_exe: &Path, _current_exe: &Path) -> Result<()> {
    Err(anyhow!("Automatic .exe replacement is supported on Windows only."))
}

async fn download_and_install_update(
    info: UpdateReleaseInfo,
    state: Arc<Mutex<AppState>>,
    ctx: egui::Context,
) {
    if !is_allowed_github_url(&info.download_url) {
        lock(&state).update_state = UpdateState::Error("Blocked untrusted download URL.".into());
        repaint(&ctx);
        return;
    }

    let size_hint = if info.size_bytes > 0 {
        format!(" ({})", format_bytes(info.size_bytes))
    } else {
        String::new()
    };
    lock(&state).update_state = UpdateState::Downloading(format!(
        "Downloading {}{}...",
        info.release_name, size_hint
    ));
    repaint(&ctx);

    let dl_client = match Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            lock(&state).update_state = UpdateState::Error(format!("HTTP client error: {e}"));
            repaint(&ctx);
            return;
        }
    };

    let resp = match dl_client
        .get(&info.download_url)
        .header("User-Agent", format!("ValorantStreamerRadar/{APP_VERSION}"))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            lock(&state).update_state = UpdateState::Error(format!(
                "Download failed (HTTP {}). Try 'Releases' to download manually.",
                r.status().as_u16()
            ));
            repaint(&ctx);
            return;
        }
        Err(e) => {
            lock(&state).update_state = UpdateState::Error(format!(
                "Download failed: {}",
                short(&e.to_string(), 80)
            ));
            repaint(&ctx);
            return;
        }
    };

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            lock(&state).update_state = UpdateState::Error(format!(
                "Download interrupted: {}",
                short(&e.to_string(), 80)
            ));
            repaint(&ctx);
            return;
        }
    };

    if bytes.len() < 512 * 1024 {
        lock(&state).update_state = UpdateState::Error(
            "Downloaded file is unexpectedly small. Aborting update for safety.".into(),
        );
        repaint(&ctx);
        return;
    }

    if let Some(expected) = &info.expected_sha256 {
        let actual = sha256_bytes(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            lock(&state).update_state = UpdateState::Error(
                "SHA-256 checksum mismatch! Update aborted for security.".into(),
            );
            repaint(&ctx);
            return;
        }
    }

    let current_exe = match env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            lock(&state).update_state = UpdateState::Error(format!("Cannot locate running exe: {e}"));
            repaint(&ctx);
            return;
        }
    };
    let parent_dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
    let staged_exe = parent_dir.join("ValorantStreamerRadar.update.exe");

    if let Err(e) = fs::write(&staged_exe, &bytes) {
        log(format!("Update staging write failed ({}): {e}", staged_exe.display()));
        lock(&state).update_state = UpdateState::Error(
            "Cannot write to app folder (permission denied). Click 'Releases' to download manually.".into(),
        );
        repaint(&ctx);
        return;
    }

    lock(&state).update_state = UpdateState::Installing(
        "Update verified! Replacing executable and restarting...".into(),
    );
    repaint(&ctx);

    if let Err(e) = spawn_windows_self_replace(&staged_exe, &current_exe) {
        let _ = fs::remove_file(&staged_exe);
        log(format!("Self-update launcher failed: {e:#}"));
        lock(&state).update_state = UpdateState::Error(format!(
            "Auto-install failed: {}. Download manually from Releases.",
            short(&e.to_string(), 65)
        ));
        repaint(&ctx);
        return;
    }

    quit_app(&ctx);
}

/// Verifies that a YouTube `videoRenderer` from keyless search is actively LIVE (not an
/// upcoming scheduled stream or ended VOD mixed into results).
fn is_yt_search_video_live(video: &Value) -> bool {
    if video.get("upcomingEventData").is_some() {
        return false;
    }
    let has_live_badge = video["badges"]
        .as_array()
        .map(|arr| {
            arr.iter().any(|b| {
                let r = &b["metadataBadgeRenderer"];
                let style = r["style"].as_str().unwrap_or("");
                let label = r["label"].as_str().unwrap_or("");
                style.eq_ignore_ascii_case("BADGE_STYLE_TYPE_LIVE_NOW")
                    || label.eq_ignore_ascii_case("LIVE")
            })
        })
        .unwrap_or(false);
    if has_live_badge {
        return true;
    }
    let has_live_overlay = video["thumbnailOverlays"]
        .as_array()
        .map(|arr| {
            arr.iter().any(|o| {
                o["thumbnailOverlayTimeStatusRenderer"]["style"]
                    .as_str()
                    .unwrap_or("")
                    .eq_ignore_ascii_case("LIVE")
            })
        })
        .unwrap_or(false);
    if has_live_overlay {
        return true;
    }
    video["viewCountText"]["runs"]
        .as_array()
        .map(|runs| {
            runs.iter().any(|r| {
                r["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains("watching")
            })
        })
        .unwrap_or(false)
}

/// Directory-linked YouTube channel: check its `/live` endpoint directly.
async fn check_youtube_channel_live(client: &Client, channel_url: &str) -> Option<(String, String)> {
    let live_tab = format!("{}/live", strip_query(channel_url).trim_end_matches('/'));
    let html = http_get_text(client, &live_tab).await?;
    let is_live = html.contains(r#""isLive":true"#) || html.contains(r#""isLiveNow":true"#);
    let is_only_upcoming = html.contains(r#""isUpcoming":true"#) && !html.contains(r#""isLiveNow":true"#);
    if !is_live || is_only_upcoming {
        return None;
    }
    // YouTube's `/live` endpoint sets canonical to `/watch?v=...` only when the channel itself is live.
    // Requiring a watch URL prevents sidebar recommended streams from false-positiving offline channels.
    let url = CANON_RE
        .captures(&html)
        .map(|c| c[1].to_string())
        .filter(|u| u.starts_with("https://www.youtube.com/watch"))?;
    let title = TITLE_RE
        .captures(&html)
        .map(|c| decode_entities(&c[1]))
        .unwrap_or_else(|| "Live Stream".to_string());
    Some((title, url))
}

/// Region-aware multi-candidate YouTube heuristic: scores live Valorant streams and rejects cross-region mismatches.
async fn check_youtube_live_candidates(
    client: &Client,
    game_name: &str,
    api_key: &str,
    region: &ServerRegionInfo,
) -> Vec<LikelyStreamerCandidate> {
    let cleaned = clean_ign(game_name);
    let name_norm = normalize(&cleaned);
    if name_norm.chars().count() < 4 || is_generic_ign(&name_norm) {
        return Vec::new();
    }
    let query = format!("{cleaned} Valorant");
    let mut raw_hits: Vec<(String, String, String)> = Vec::new();
    let mut used_api = false;

    if !api_key.trim().is_empty() {
        match client
            .get("https://www.googleapis.com/youtube/v3/search")
            .query(&[
                ("part", "snippet"),
                ("q", query.as_str()),
                ("type", "video"),
                ("eventType", "live"),
                ("regionCode", region.yt_region_code),
                ("maxResults", "10"),
                ("key", api_key.trim()),
            ])
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<Value>().await {
                    if let Some(items) = data["items"].as_array() {
                        used_api = true;
                        for item in items {
                            let video_id = item["id"]["videoId"].as_str().unwrap_or("");
                            let title = decode_entities(item["snippet"]["title"].as_str().unwrap_or(""));
                            let channel = decode_entities(item["snippet"]["channelTitle"].as_str().unwrap_or(""));
                            if !video_id.is_empty() && !channel.is_empty() {
                                raw_hits.push((
                                    channel,
                                    title,
                                    format!("https://www.youtube.com/watch?v={video_id}"),
                                ));
                            }
                        }
                    }
                }
            }
            Ok(resp) => log(format!("YouTube API returned {}; using keyless fallback", resp.status())),
            Err(e) => log(format!("YouTube API request failed: {e}")),
        }
    }

    if !used_api {
        let url = format!(
            "https://www.youtube.com/results?search_query={}&sp=EgJAAQ%3D%3D&gl={}",
            urlencode(&query),
            region.yt_region_code
        );
        if let Some(html) = http_get_text_with_lang(client, &url, region.accept_lang).await {
            if let Some(caps) = YT_DATA_RE.captures(&html) {
                if let Ok(data) = serde_json::from_str::<Value>(&caps[1]) {
                    if let Some(sections) = data["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]
                        ["sectionListRenderer"]["contents"]
                        .as_array()
                    {
                        for section in sections {
                            let Some(items) = section["itemSectionRenderer"]["contents"].as_array() else {
                                continue;
                            };
                            for item in items.iter().take(12) {
                                let Some(video) = item.get("videoRenderer") else {
                                    continue;
                                };
                                if !is_yt_search_video_live(video) {
                                    continue;
                                }
                                let video_id = video["videoId"].as_str().unwrap_or("");
                                let title = video["title"]["runs"][0]["text"].as_str().unwrap_or("");
                                let channel = video["ownerText"]["runs"][0]["text"].as_str().unwrap_or("");
                                if !video_id.is_empty() && !channel.is_empty() {
                                    raw_hits.push((
                                        decode_entities(channel),
                                        decode_entities(title),
                                        format!("https://www.youtube.com/watch?v={video_id}"),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut seen_urls = HashSet::new();
    let mut scored = Vec::new();
    for (channel, title, url) in raw_hits {
        if !seen_urls.insert(url.clone()) {
            continue;
        }
        if let Some(cand) = score_heuristic_candidate(
            "YouTube",
            &cleaned,
            &channel,
            &title,
            &url,
            None,
            false,
            region,
        ) {
            scored.push(cand);
        }
    }
    scored.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    scored.truncate(3);
    scored
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

/// Fetches live Twitch metadata including game name, stream language, and tags.
async fn fetch_twitch_stream_meta(
    client: &Client,
    login: &str,
    is_exact_login: bool,
    client_id: &str,
    bearer_token: Option<&str>,
) -> Option<(String, String, String, String, String, bool)> {
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
            .query(&[("user_login", login)])
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<Value>().await {
                    if let Some(stream) = data["data"].as_array().and_then(|a| a.first()) {
                        let game = stream["game_name"].as_str().unwrap_or("");
                        let is_val = game.eq_ignore_ascii_case("VALORANT");
                        if is_exact_login || is_val {
                            let display = stream["user_name"].as_str().unwrap_or(login).to_string();
                            let title = stream["title"].as_str().unwrap_or("Live on Twitch").to_string();
                            let lang = stream["language"].as_str().unwrap_or("").to_string();
                            let tags = stream["tags"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                })
                                .unwrap_or_default();
                            return Some((
                                display,
                                title,
                                format!("https://www.twitch.tv/{login}"),
                                lang,
                                tags,
                                is_val,
                            ));
                        }
                    }
                    return None;
                }
            }
            Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
                *lock(&TWITCH_TOKEN) = None;
            }
            _ => {}
        }
    }

    // 2. Keyless public web GraphQL fallback (includes broadcastSettings language & tags)
    let body = serde_json::json!({
        "query": "query($login: String!) { user(login: $login) { displayName broadcastSettings { language } stream { title game { name } tags { localizedName } } } }",
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
        let is_val = game.eq_ignore_ascii_case("VALORANT");
        if is_exact_login || is_val {
            let display = v["data"]["user"]["displayName"].as_str().unwrap_or(login).to_string();
            let title = stream["title"].as_str().unwrap_or("Live on Twitch").to_string();
            let lang = v["data"]["user"]["broadcastSettings"]["language"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let tags = stream["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t["localizedName"].as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            return Some((
                display,
                title,
                format!("https://www.twitch.tv/{login}"),
                lang,
                tags,
                is_val,
            ));
        }
    }
    None
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
    fetch_twitch_stream_meta(client, &login, is_exact_login, client_id, bearer_token)
        .await
        .map(|(d, t, u, _, _, _)| (d, t, u))
}

/// Batched Twitch live check for up to 4 candidate logins in ONE HTTP request (Helix multi-param
/// or aliased public GraphQL), returning all live VALORANT streams found.
async fn fetch_twitch_streams_batch(
    client: &Client,
    logins: &[String],
    client_id: &str,
    bearer_token: Option<&str>,
) -> Vec<(String, String, String, String, String, bool)> {
    let valid: Vec<String> = logins
        .iter()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| l.len() >= 4 && l.len() <= 25 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .take(4)
        .collect();
    if valid.is_empty() {
        return Vec::new();
    }

    // 1. Official Helix API (supports repeated `user_login` params in a single call)
    if let Some(token) = bearer_token {
        let params: Vec<(&str, &str)> = valid.iter().map(|l| ("user_login", l.as_str())).collect();
        match client
            .get("https://api.twitch.tv/helix/streams")
            .header("Client-Id", client_id.trim())
            .header("Authorization", format!("Bearer {token}"))
            .query(&params)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<Value>().await {
                    let mut hits = Vec::new();
                    if let Some(arr) = data["data"].as_array() {
                        for stream in arr {
                            let game = stream["game_name"].as_str().unwrap_or("");
                            let is_val = game.eq_ignore_ascii_case("VALORANT");
                            if !is_val {
                                continue;
                            }
                            let login = stream["user_login"].as_str().unwrap_or("").to_ascii_lowercase();
                            if login.is_empty() {
                                continue;
                            }
                            let display = stream["user_name"].as_str().unwrap_or(&login).to_string();
                            let title = stream["title"].as_str().unwrap_or("Live on Twitch").to_string();
                            let lang = stream["language"].as_str().unwrap_or("").to_string();
                            let tags = stream["tags"]
                                .as_array()
                                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" "))
                                .unwrap_or_default();
                            hits.push((display, title, format!("https://www.twitch.tv/{login}"), lang, tags, true));
                        }
                    }
                    return hits;
                }
            }
            Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
                *lock(&TWITCH_TOKEN) = None;
            }
            _ => {}
        }
    }

    // 2. Keyless public GraphQL aliased batch query (logins are strictly [a-z0-9_])
    let mut query = String::from("query { ");
    for (idx, login) in valid.iter().enumerate() {
        query.push_str(&format!(
            "u{idx}: user(login: \"{login}\") {{ displayName broadcastSettings {{ language }} stream {{ title game {{ name }} tags {{ localizedName }} }} }} "
        ));
    }
    query.push('}');
    let body = serde_json::json!({ "query": query });
    let Ok(resp) = client
        .post("https://gql.twitch.tv/gql")
        .header("Client-Id", "kimne78kx3ncx6brgo4mv6wki5h1ko")
        .json(&body)
        .send()
        .await
    else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(v) = resp.json::<Value>().await else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for (idx, login) in valid.iter().enumerate() {
        let user = &v["data"][format!("u{idx}")];
        let stream = &user["stream"];
        if !stream.is_object() {
            continue;
        }
        let game = stream["game"]["name"].as_str().unwrap_or("");
        if !game.eq_ignore_ascii_case("VALORANT") {
            continue;
        }
        let display = user["displayName"].as_str().unwrap_or(login).to_string();
        let title = stream["title"].as_str().unwrap_or("Live on Twitch").to_string();
        let lang = user["broadcastSettings"]["language"].as_str().unwrap_or("").to_string();
        let tags = stream["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t["localizedName"].as_str()).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        hits.push((display, title, format!("https://www.twitch.tv/{login}"), lang, tags, true));
    }
    hits
}

async fn check_twitch_live_candidates(
    client: &Client,
    game_name: &str,
    client_id: &str,
    bearer_token: Option<&str>,
    region: &ServerRegionInfo,
) -> Vec<LikelyStreamerCandidate> {
    let cleaned = clean_ign(game_name);
    let base: String = cleaned
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if base.len() < 4 || is_generic_ign(&normalize(&cleaned)) {
        return Vec::new();
    }

    let raw_login: String = game_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();

    let mut logins = Vec::new();
    let mut seen = HashSet::new();
    let mut push_login = |s: String| {
        if s.len() >= 4 && s.len() <= 25 && seen.insert(s.clone()) {
            logins.push(s);
        }
    };
    push_login(base.clone());
    if raw_login != base {
        push_login(raw_login);
    }
    if base.len() <= 18 && !base.ends_with("tv") && !base.ends_with("ttv") {
        push_login(format!("{base}tv"));
        push_login(format!("{base}_tv"));
        push_login(format!("{base}ttv"));
    }
    logins.truncate(4);

    let mut out = Vec::new();
    for (display, title, url, lang, tags, is_val) in
        fetch_twitch_streams_batch(client, &logins, client_id, bearer_token).await
    {
        let title_with_tags = if tags.is_empty() {
            title.clone()
        } else {
            format!("{title} {tags}")
        };
        let lang_opt = if lang.is_empty() { None } else { Some(lang.as_str()) };
        if let Some(mut cand) = score_heuristic_candidate(
            "Twitch",
            &cleaned,
            &display,
            &title_with_tags,
            &url,
            lang_opt,
            is_val,
            region,
        ) {
            cand.title = title;
            out.push(cand);
        }
    }
    out.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    out.truncate(3);
    out
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

async fn check_kick_live_candidates(
    client: &Client,
    game_name: &str,
    region: &ServerRegionInfo,
) -> Vec<LikelyStreamerCandidate> {
    let cleaned = clean_ign(game_name);
    let slug: String = cleaned
        .to_ascii_lowercase()
        .replace(' ', "_")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if slug.len() < 4 || is_generic_ign(&normalize(&cleaned)) {
        return Vec::new();
    }
    let resp = match client
        .get(format!("https://kick.com/api/v2/channels/{slug}"))
        .header("User-Agent", BROWSER_UA)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return Vec::new(),
    };
    let Ok(v) = resp.json::<Value>().await else {
        return Vec::new();
    };
    if !v["livestream"].is_object() {
        return Vec::new();
    }
    let title = v["livestream"]["session_title"].as_str().unwrap_or("Live on Kick").to_string();
    let channel = v["user"]["username"].as_str().unwrap_or(&slug).to_string();
    let lang = v["livestream"]["language"].as_str();
    let is_val = v["livestream"]["categories"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .any(|c| c["name"].as_str().unwrap_or("").eq_ignore_ascii_case("VALORANT"))
        })
        .unwrap_or(false);
    let url = format!("https://kick.com/{slug}");
    score_heuristic_candidate("Kick", &cleaned, &channel, &title, &url, lang, is_val, region)
        .into_iter()
        .collect()
}

/// Optional (off by default) live Twitch + YouTube Valorant streamer recommendations for the active server region.
async fn fetch_live_recommendations(
    client: &Client,
    region: &ServerRegionInfo,
    youtube_api_key: &str,
    twitch_client_id: &str,
    twitch_client_secret: &str,
) -> Result<Vec<RecommendedStreamer>> {
    let twitch_token = get_twitch_app_token(client, twitch_client_id, twitch_client_secret).await;

    // Fetch Twitch and YouTube recommendations concurrently
    let yt_fut = async {
        let mut list = Vec::new();
        let query = format!("Valorant live {}", region.search_region_term);
        let mut used_api = false;

        if !youtube_api_key.trim().is_empty() {
            if let Ok(resp) = client
                .get("https://www.googleapis.com/youtube/v3/search")
                .query(&[
                    ("part", "snippet"),
                    ("q", query.as_str()),
                    ("type", "video"),
                    ("eventType", "live"),
                    ("regionCode", region.yt_region_code),
                    ("maxResults", "8"),
                    ("key", youtube_api_key.trim()),
                ])
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(data) = resp.json::<Value>().await {
                        if let Some(items) = data["items"].as_array() {
                            used_api = true;
                            for item in items {
                                let vid = item["id"]["videoId"].as_str().unwrap_or("");
                                let title = decode_entities(item["snippet"]["title"].as_str().unwrap_or(""));
                                let channel = decode_entities(item["snippet"]["channelTitle"].as_str().unwrap_or(""));
                                if vid.is_empty() || channel.is_empty() {
                                    continue;
                                }
                                if let Some(z) = detect_text_geo_zone(&format!("{title} {channel}"), None) {
                                    if !zones_compatible(region.zone, z) {
                                        continue;
                                    }
                                }
                                list.push(RecommendedStreamer {
                                    platform: "YouTube".to_string(),
                                    channel,
                                    title,
                                    url: format!("https://www.youtube.com/watch?v={vid}"),
                                    viewers: None,
                                    region_label: region.short_label.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        if !used_api {
            let url = format!(
                "https://www.youtube.com/results?search_query={}&sp=EgJAAQ%3D%3D&gl={}",
                urlencode(&query),
                region.yt_region_code
            );
            if let Some(html) = http_get_text_with_lang(client, &url, region.accept_lang).await {
                if let Some(caps) = YT_DATA_RE.captures(&html) {
                    if let Ok(data) = serde_json::from_str::<Value>(&caps[1]) {
                        if let Some(sections) = data["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]
                            ["sectionListRenderer"]["contents"]
                            .as_array()
                        {
                            for section in sections {
                                let Some(items) = section["itemSectionRenderer"]["contents"].as_array() else {
                                    continue;
                                };
                                for item in items.iter().take(12) {
                                    let Some(video) = item.get("videoRenderer") else {
                                        continue;
                                    };
                                    if !is_yt_search_video_live(video) {
                                        continue;
                                    }
                                    let vid = video["videoId"].as_str().unwrap_or("");
                                    let title = decode_entities(video["title"]["runs"][0]["text"].as_str().unwrap_or(""));
                                    let channel = decode_entities(video["ownerText"]["runs"][0]["text"].as_str().unwrap_or(""));
                                    if vid.is_empty() || channel.is_empty() || !looks_like_valorant_stream(&title, &channel) {
                                        continue;
                                    }
                                    if let Some(z) = detect_text_geo_zone(&format!("{title} {channel}"), None) {
                                        if !zones_compatible(region.zone, z) {
                                            continue;
                                        }
                                    }
                                    let viewers = video["viewCountText"]["runs"][0]["text"]
                                        .as_str()
                                        .and_then(|s| s.replace(',', "").trim().parse::<u64>().ok());
                                    list.push(RecommendedStreamer {
                                        platform: "YouTube".to_string(),
                                        channel,
                                        title,
                                        url: format!("https://www.youtube.com/watch?v={vid}"),
                                        viewers,
                                        region_label: region.short_label.to_string(),
                                    });
                                    if list.len() >= 6 {
                                        break;
                                    }
                                }
                                if list.len() >= 6 {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        list
    };

    let tw_fut = async {
        let mut list = Vec::new();
        let mut used_helix = false;
        if let Some(tok) = twitch_token.as_deref() {
            if let Ok(resp) = client
                .get("https://api.twitch.tv/helix/streams")
                .header("Client-Id", twitch_client_id.trim())
                .header("Authorization", format!("Bearer {tok}"))
                .query(&[
                    ("game_id", "516575"),
                    ("language", region.twitch_lang),
                    ("first", "8"),
                ])
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(data) = resp.json::<Value>().await {
                        if let Some(arr) = data["data"].as_array() {
                            used_helix = !arr.is_empty();
                            for st in arr.iter().take(6) {
                                let login = st["user_login"].as_str().unwrap_or("");
                                let name = st["user_name"].as_str().unwrap_or(login);
                                let title = st["title"].as_str().unwrap_or("Live on Twitch");
                                let viewers = st["viewer_count"].as_u64();
                                if !login.is_empty() {
                                    list.push(RecommendedStreamer {
                                        platform: "Twitch".to_string(),
                                        channel: name.to_string(),
                                        title: title.to_string(),
                                        url: format!("https://www.twitch.tv/{login}"),
                                        viewers,
                                        region_label: region.short_label.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        if !used_helix {
            let body = serde_json::json!({
                "query": "query { game(name: \"VALORANT\") { streams(first: 24, options: { sort: VIEWER_COUNT }) { edges { node { title viewersCount broadcaster { login displayName broadcastSettings { language } } } } } } }"
            });
            if let Ok(resp) = client
                .post("https://gql.twitch.tv/gql")
                .header("Client-Id", "kimne78kx3ncx6brgo4mv6wki5h1ko")
                .json(&body)
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(v) = resp.json::<Value>().await {
                        if let Some(edges) = v["data"]["game"]["streams"]["edges"].as_array() {
                            let mut regional = Vec::new();
                            let mut fallback = Vec::new();
                            for edge in edges {
                                let node = &edge["node"];
                                let login = node["broadcaster"]["login"].as_str().unwrap_or("");
                                let name = node["broadcaster"]["displayName"].as_str().unwrap_or(login);
                                let title = node["title"].as_str().unwrap_or("Live on Twitch");
                                let lang = node["broadcaster"]["broadcastSettings"]["language"].as_str();
                                let viewers = node["viewersCount"].as_u64();
                                if login.is_empty() {
                                    continue;
                                }
                                let item = RecommendedStreamer {
                                    platform: "Twitch".to_string(),
                                    channel: name.to_string(),
                                    title: title.to_string(),
                                    url: format!("https://www.twitch.tv/{login}"),
                                    viewers,
                                    region_label: region.short_label.to_string(),
                                };
                                let z = detect_text_geo_zone(&format!("{title} {name}"), lang);
                                if z.map(|dz| zones_compatible(region.zone, dz)).unwrap_or(false)
                                    || lang.map(|l| l.eq_ignore_ascii_case(region.twitch_lang)).unwrap_or(false)
                                {
                                    regional.push(item);
                                } else if z.is_none() {
                                    fallback.push(item);
                                }
                            }
                            for item in regional.into_iter().chain(fallback.into_iter()).take(6) {
                                list.push(item);
                            }
                        }
                    }
                }
            }
        }
        list
    };

    let (yt_list, tw_list) = tokio::join!(yt_fut, tw_fut);
    let mut combined = Vec::new();
    let max_len = yt_list.len().max(tw_list.len());
    for i in 0..max_len {
        if let Some(item) = tw_list.get(i) {
            combined.push(item.clone());
        }
        if let Some(item) = yt_list.get(i) {
            combined.push(item.clone());
        }
    }
    if combined.is_empty() {
        return Err(anyhow!(
            "No live Valorant recommendations could be fetched for {} right now.",
            region.short_label
        ));
    }
    Ok(combined)
}

// ============================================================================
// DIRECTORY, NOTIFICATIONS & SCANNER
// ============================================================================

/// Previously seeded placeholder streamer IDs removed in v1.2.3; auto-purged from local caches.
const SEEDED_SAMPLE_STREAMER_IDS: &[&str] = &[
    "rohan-gaming-yt",
    "vexfps-twitch",
    "kairoclutch-kick",
    "harujp-twitch",
    "kuyamnl-yt",
    "lonestarval-twitch",
];

fn purge_seeded_sample_streamers(entries: Vec<DirectoryStreamer>) -> Vec<DirectoryStreamer> {
    entries
        .into_iter()
        .filter(|e| {
            let id_low = e.id.trim().to_ascii_lowercase();
            !SEEDED_SAMPLE_STREAMER_IDS.iter().any(|&seeded| id_low == seeded)
        })
        .collect()
}

fn load_clean_remote_directory_cache() -> Vec<DirectoryStreamer> {
    let raw: Vec<DirectoryStreamer> = load_json("directory_cache.json");
    let raw_len = raw.len();
    let cleaned = purge_seeded_sample_streamers(
        raw.iter().filter_map(sanitize_directory_entry).collect(),
    );
    if cleaned.len() != raw_len {
        save_json("directory_cache.json", &cleaned);
    }
    cleaned
}

async fn fetch_remote_directory(public: &Client, owner: &str, repo: &str) -> (Vec<DirectoryStreamer>, bool) {
    if !valid_github_segment(owner) || !valid_github_segment(repo) {
        return (load_clean_remote_directory_cache(), false);
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
            let clean = purge_seeded_sample_streamers(
                list.iter().filter_map(sanitize_directory_entry).collect(),
            );
            save_json("directory_cache.json", &clean);
            (clean, true)
        }
        None => {
            log("GitHub directory unavailable; using cached copy");
            (load_clean_remote_directory_cache(), false)
        }
    }
}

fn merge_directories(local: &[DirectoryStreamer], remote: &[DirectoryStreamer]) -> Vec<DirectoryStreamer> {
    let mut out: Vec<DirectoryStreamer> = Vec::new();
    let mut seen_riot: HashMap<(String, String), usize> = HashMap::new();
    let mut seen_url: HashMap<(String, String), usize> = HashMap::new();

    for entry in remote.iter().chain(local.iter()).filter_map(sanitize_directory_entry) {
        let rkey = (entry.riot_id.to_ascii_lowercase(), entry.platform.to_ascii_lowercase());
        let ukey = (
            entry.channel_url.trim_end_matches('/').to_ascii_lowercase(),
            entry.platform.to_ascii_lowercase(),
        );
        if let Some(&idx) = seen_riot.get(&rkey).or_else(|| seen_url.get(&ukey)) {
            // Merge unique aliases without duplicating records; preserve verified flag if either is verified
            let existing = &mut out[idx];
            existing.verified = existing.verified || entry.verified;
            if existing.server_region.is_empty() && !entry.server_region.is_empty() {
                existing.server_region = entry.server_region;
            }
            for alias in entry.aliases {
                if !alias.eq_ignore_ascii_case(&existing.riot_id)
                    && !existing.aliases.iter().any(|a| a.eq_ignore_ascii_case(&alias))
                {
                    existing.aliases.push(alias);
                }
            }
        } else {
            let idx = out.len();
            seen_riot.insert(rkey, idx);
            seen_url.insert(ukey, idx);
            out.push(entry);
        }
    }
    out
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
            let _ = key.set_value("IconBackgroundColor", &"FF000000");
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
    let tag = if entry.source == "Chat" { "CHAT LIVE" } else { "LIVE" };
    let title = format!("[{}] {}: {}{}", entry.platform, tag, entry.riot_id, relation);
    let line1 = if !entry.chat_message.is_empty() && entry.source == "Chat" {
        format!("Chat: \"{}\"", short(&entry.chat_message, 90))
    } else {
        format!("{} ({})", short(&entry.title, 90), entry.channel)
    };
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

/// Records new detections (deduplicated by match_id + riot_id + platform).
/// Only copies links & shows Windows toast popups for Confirmed or Probable detections (never Unverified).
fn record_and_notify(
    state: &Arc<Mutex<AppState>>,
    ctx: &egui::Context,
    entry: StreamerEntry,
    copy: bool,
    use_custom_app_id: bool,
) {
    let should_alert = !entry.confidence_tier.eq_ignore_ascii_case("Unverified");
    let is_new = {
        let mut s = lock(state);
        if let Some(existing) = s
            .history
            .iter_mut()
            .find(|e| e.match_id == entry.match_id && e.riot_id == entry.riot_id && e.platform == entry.platform)
        {
            if existing.chat_message.is_empty() && !entry.chat_message.is_empty() {
                existing.chat_message = entry.chat_message.clone();
                save_json("streamer_history.json", &s.history);
            }
            false
        } else {
            s.history.insert(0, entry.clone());
            s.history.truncate(MAX_HISTORY);
            save_json("streamer_history.json", &s.history);
            true
        }
    };
    repaint(ctx);

    if copy && should_alert {
        if let Ok(mut cb) = Clipboard::new() {
            let _ = cb.set_text(entry.url.clone());
        }
    }
    if is_new && should_alert {
        show_toast(&entry, use_custom_app_id);
    }
}

/// Evaluates whether a database entry's configured in-game server is compatible with the user's match server.
fn evaluate_directory_server_match(
    entry: &DirectoryStreamer,
    match_region: &ServerRegionInfo,
    match_kind: &str,
) -> (ConfidenceTier, u8, String, String) {
    if !entry.server_region.is_empty() {
        if let Some(db_reg) = server_region_by_code(&entry.server_region) {
            if db_reg.code == match_region.code {
                let tier = if entry.verified { ConfidenceTier::Confirmed } else { ConfidenceTier::Probable };
                let score = if entry.verified { 100 } else { 88 };
                return (
                    tier,
                    score,
                    "In-Game Server Confirmed".to_string(),
                    format!("{match_kind} + Verified {} server", match_region.short_label),
                );
            } else if zones_compatible(match_region.zone, db_reg.zone) {
                let tier = if entry.verified { ConfidenceTier::Confirmed } else { ConfidenceTier::Probable };
                let score = if entry.verified { 94 } else { 84 };
                return (
                    tier,
                    score,
                    "Compatible Nearby Server".to_string(),
                    format!("{match_kind} + Compatible server ({})", db_reg.short_label),
                );
            } else {
                // Cross-continent mismatch (e.g., NA database entry while playing on Mumbai): caution as Unverified
                return (
                    ConfidenceTier::Unverified,
                    48,
                    format!("Server Mismatch (DB: {})", db_reg.short_label),
                    format!(
                        "{match_kind}, but streamer plays on {} (your match: {})",
                        db_reg.short_label, match_region.short_label
                    ),
                );
            }
        }
    }

    if entry.verified {
        (
            ConfidenceTier::Confirmed,
            92,
            "Verified Database Entry (Server Any)".to_string(),
            format!("{match_kind} (community verified)"),
        )
    } else {
        (
            ConfidenceTier::Probable,
            80,
            "Local Entry (Server Unspecified)".to_string(),
            format!("{match_kind} (local custom entry)"),
        )
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
    region: &'static ServerRegionInfo,
    player_info: MatchPlayerInfo,
    chat_plug: Option<ChatStreamPlug>,
) -> Vec<StreamerEntry> {
    let player = player_info.record;
    let riot_id = format!("{}#{}", player.game_name, player.tag_line);
    let now = Local::now();
    let timestamp = now.format("%d %b %Y | %I:%M %p").to_string();
    let detected_at_iso = now.to_rfc3339();
    let team_relation = player_info.team_relation;
    let agent_name = player_info.agent_name;

    let make = |platform: &str,
                channel: String,
                title: String,
                url: String,
                source: &str,
                chat_msg: String,
                confidence: u8,
                confidence_tier: &str,
                server_match_status: String,
                match_reason: String,
                candidates: Vec<LikelyStreamerCandidate>| StreamerEntry {
        match_id: match_id.clone(),
        riot_id: riot_id.clone(),
        platform: platform.to_string(),
        channel,
        title,
        url,
        timestamp: timestamp.clone(),
        detected_at_iso: detected_at_iso.clone(),
        team_relation: team_relation.clone(),
        map_name: map_name.clone(),
        agent_name: agent_name.clone(),
        queue_name: queue_name.clone(),
        chat_message: chat_msg,
        source: source.to_string(),
        confidence,
        confidence_tier: confidence_tier.to_string(),
        server_match_status,
        match_reason,
        server_region: region.label.to_string(),
        candidates,
    };

    let mut out = Vec::new();
    let mut ran_heuristics = false;

    // Phase 1: exact Riot ID or known alias mapping from centralized/local streamer database
    let linked: Vec<(&DirectoryStreamer, &'static str)> = directory
        .iter()
        .filter_map(|d| directory_entry_matches_player(d, &riot_id).map(|kind| (d, kind)))
        .collect();
    let mut checked_dir_platforms: HashSet<&'static str> = HashSet::new();
    if !linked.is_empty() {
        for (d, match_kind) in linked {
            if let Some(cp) = canonical_platform(&d.platform) {
                checked_dir_platforms.insert(cp);
            }
            let handle = if !d.channel_id.is_empty() {
                d.channel_id.clone()
            } else {
                last_path_segment(&d.channel_url)
            };
            let hit = match d.platform.as_str() {
                "Twitch" => check_twitch_live(&client, &handle, true, &config.twitch_client_id, twitch_token.as_deref())
                    .await
                    .map(|(_, t, u)| (t, u)),
                "Kick" => check_kick_live(&client, &handle, true).await.map(|(_, t, u)| (t, u)),
                _ => check_youtube_channel_live(&client, &d.channel_url).await,
            };
            if let Some((title, url)) = hit {
                let (tier, conf, srv_status, reason) = evaluate_directory_server_match(d, region, match_kind);
                out.push(make(
                    &d.platform,
                    d.channel_name.clone(),
                    title,
                    url,
                    "Directory",
                    String::new(),
                    conf,
                    tier.as_str(),
                    srv_status,
                    reason,
                    Vec::new(),
                ));
            }
        }
    }

    // Phase 2: server-aware multi-signal heuristic fallback.
    // If a Directory streamer was offline on their registered platform, still check other platforms
    // without re-heuristically guessing on the already-verified offline platform.
    if out.is_empty() {
        ran_heuristics = true;
        let check_yt = !checked_dir_platforms.contains("YouTube");
        let check_tw = !checked_dir_platforms.contains("Twitch");
        let check_kick = !checked_dir_platforms.contains("Kick");
        let (yt_cands, tw_cands, kick_cands) = tokio::join!(
            async {
                if check_yt {
                    check_youtube_live_candidates(&client, &player.game_name, &config.youtube_api_key, region).await
                } else {
                    Vec::new()
                }
            },
            async {
                if check_tw {
                    check_twitch_live_candidates(
                        &client,
                        &player.game_name,
                        &config.twitch_client_id,
                        twitch_token.as_deref(),
                        region,
                    )
                    .await
                } else {
                    Vec::new()
                }
            },
            async {
                if check_kick {
                    check_kick_live_candidates(&client, &player.game_name, region).await
                } else {
                    Vec::new()
                }
            },
        );
        let mut all_cands: Vec<LikelyStreamerCandidate> = Vec::new();
        all_cands.extend(yt_cands);
        all_cands.extend(tw_cands);
        all_cands.extend(kick_cands);
        all_cands.sort_by(|a, b| b.confidence.cmp(&a.confidence));
        let mut dedup_urls = HashSet::new();
        all_cands.retain(|c| dedup_urls.insert(c.url.to_ascii_lowercase()));
        all_cands.truncate(3);

        if let Some(best) = all_cands.first().cloned() {
            let tier_str = if best.confidence_tier.is_empty() {
                "Unverified"
            } else {
                best.confidence_tier.as_str()
            };
            out.push(make(
                &best.platform,
                best.channel.clone(),
                best.title.clone(),
                best.url.clone(),
                "Heuristic",
                String::new(),
                best.confidence,
                tier_str,
                best.server_match_status.clone(),
                best.match_reason.clone(),
                all_cands,
            ));
        }
    }

    // Phase 3: Post-match in-game text chat stream plug ("im live on yt", dropped stream URL/handle)
    if let Some(plug) = chat_plug {
        let quote = short(&plug.raw_message, 140);
        if let Some(existing) = out.iter_mut().find(|e| e.platform.eq_ignore_ascii_case(&plug.platform)) {
            existing.chat_message = quote;
            existing.confidence = 100;
            existing.confidence_tier = ConfidenceTier::Confirmed.as_str().to_string();
            existing.server_match_status = "Confirmed in Match Chat".to_string();
            existing.source = "Chat".to_string();
        } else if let Some(raw_url) = plug.url {
            let fallback_channel = plug.handle.clone().unwrap_or_else(|| player.game_name.clone());
            let enriched = match plug.platform.as_str() {
                "Twitch" => {
                    let slug = plug.handle.clone().unwrap_or_else(|| last_path_segment(&raw_url));
                    check_twitch_live(&client, &slug, true, &config.twitch_client_id, twitch_token.as_deref())
                        .await
                }
                "Kick" => {
                    let slug = plug.handle.clone().unwrap_or_else(|| last_path_segment(&raw_url));
                    check_kick_live(&client, &slug, true).await
                }
                _ => {
                    let lower_u = raw_url.to_ascii_lowercase();
                    if lower_u.contains("/watch") || lower_u.contains("youtu.be/") || lower_u.contains("/live/") {
                        None
                    } else {
                        check_youtube_channel_live(&client, &raw_url)
                            .await
                            .map(|(t, u)| (fallback_channel.clone(), t, u))
                    }
                }
            };
            let (channel, title, final_url) = match enriched {
                Some((c, t, u)) => (c, t, u),
                None => (
                    fallback_channel,
                    format!("Shared {} stream link in match chat", plug.platform),
                    raw_url,
                ),
            };
            out.push(make(
                &plug.platform,
                channel,
                title,
                final_url,
                "Chat",
                quote,
                100,
                ConfidenceTier::Confirmed.as_str(),
                "Confirmed in Match Chat".to_string(),
                "Shared directly in match text chat".to_string(),
                Vec::new(),
            ));
        } else {
            let live_cands = if !ran_heuristics {
                match plug.platform.as_str() {
                    "Twitch" => {
                        check_twitch_live_candidates(
                            &client,
                            &player.game_name,
                            &config.twitch_client_id,
                            twitch_token.as_deref(),
                            region,
                        )
                        .await
                    }
                    "Kick" => check_kick_live_candidates(&client, &player.game_name, region).await,
                    _ => check_youtube_live_candidates(&client, &player.game_name, &config.youtube_api_key, region).await,
                }
            } else {
                Vec::new()
            };

            if let Some(best) = live_cands.first().cloned() {
                out.push(make(
                    &plug.platform,
                    best.channel.clone(),
                    best.title.clone(),
                    best.url.clone(),
                    "Chat",
                    quote,
                    best.confidence.max(85),
                    ConfidenceTier::Confirmed.as_str(),
                    "Confirmed in Match Chat".to_string(),
                    format!("Mentioned live in chat + {}", best.match_reason),
                    live_cands,
                ));
            } else {
                let cleaned = clean_ign(&player.game_name);
                let query_name = if cleaned.is_empty() { player.game_name.clone() } else { cleaned };
                let search_url = match plug.platform.as_str() {
                    "Twitch" => format!("https://www.twitch.tv/search?term={}", urlencode(&query_name)),
                    "Kick" => format!("https://kick.com/search?query={}", urlencode(&query_name)),
                    _ => format!(
                        "https://www.youtube.com/results?search_query={}&sp=EgJAAQ%3D%3D&gl={}",
                        urlencode(&format!("{query_name} Valorant")),
                        region.yt_region_code
                    ),
                };
                out.push(make(
                    &plug.platform,
                    query_name,
                    format!("Mentioned live on {} in match chat", plug.platform),
                    search_url,
                    "Chat",
                    quote,
                    82,
                    ConfidenceTier::Probable.as_str(),
                    "Chat Mention (Channel Link Unverified)".to_string(),
                    "Player announced live stream in match chat".to_string(),
                    Vec::new(),
                ));
            }
        }
    }

    out
}

async fn sync_directory_only(public_client: Client, state: Arc<Mutex<AppState>>, ctx: egui::Context) {
    {
        let mut s = lock(&state);
        s.is_syncing_directory = true;
    }
    repaint(&ctx);

    let (remote, ok) = fetch_remote_directory(&public_client, COMMUNITY_REPO_OWNER, COMMUNITY_REPO_NAME).await;
    {
        let mut s = lock(&state);
        s.remote_directory = remote;
        s.is_syncing_directory = false;
    }
    if ok {
        set_toast(&state, "Synced community streamer database!");
    } else {
        set_toast(&state, "Database unreachable - using cached directory");
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
    let (config, detected_server, detected_api_region, previous) = {
        let s = lock(&state);
        (
            s.config.clone(),
            s.detected_server.clone(),
            s.detected_api_region.clone(),
            s.last_scanned_match.clone(),
        )
    };
    let initial_region = effective_server_region(&config, &detected_server, &detected_api_region);
    let api_region = initial_region.api_region;
    set_status(
        &state,
        &ctx,
        format!("Syncing database & scanning last match ({})...", initial_region.label),
    );

    let allow_region_fallback = config.server_override.eq_ignore_ascii_case("auto");
    let fetched = fetch_match_and_names(
        &local_client,
        &public_client,
        api_region,
        allow_region_fallback,
        previous.as_deref(),
        trigger == ScanTrigger::PostMatch,
    )
    .await;

    let (my_puuid, resolved_api_region, match_ctx) = match fetched {
        Ok(v) => v,
        Err(e) => {
            log(format!("Match fetch failed on {api_region}: {e:#}"));
            set_status(&state, &ctx, format!("Could not fetch match: {}", short(&e.to_string(), 110)));
            return;
        }
    };
    let MatchContext {
        match_id,
        map_name,
        queue_name,
        my_agent,
        server_region: match_pod_server,
        players,
    } = match_ctx;

    {
        let mut s = lock(&state);
        if resolved_api_region != s.detected_api_region {
            s.detected_api_region = resolved_api_region.clone();
        }
        if let Some(pod_srv) = &match_pod_server {
            s.detected_server = pod_srv.clone();
        }
        s.last_scanned_match = Some(match_id.clone());
    }

    let effective_region = if config.server_override.eq_ignore_ascii_case("auto") {
        match_pod_server
            .as_deref()
            .and_then(server_region_by_code)
            .unwrap_or_else(|| {
                let srv = if resolved_api_region == detected_api_region {
                    detected_server.as_str()
                } else {
                    ""
                };
                effective_server_region(&config, srv, &resolved_api_region)
            })
    } else {
        initial_region
    };

    let display_map = if map_name.is_empty() { "Unknown Map" } else { map_name.as_str() };
    let display_queue = if queue_name.is_empty() { "Match" } else { queue_name.as_str() };
    set_status(
        &state,
        &ctx,
        format!("Scanning {display_queue} on {display_map} ({})...", effective_region.label),
    );

    let (remote, _) = fetch_remote_directory(&public_client, COMMUNITY_REPO_OWNER, COMMUNITY_REPO_NAME).await;
    let (local_dir, mut match_chat) = {
        let mut s = lock(&state);
        s.remote_directory = remote.clone();
        let chat_snapshot = s.chat_buffer.clone();
        if trigger == ScanTrigger::PostMatch {
            s.chat_buffer.clear();
        }
        (s.local_directory.clone(), chat_snapshot)
    };
    if config.capture_in_game_chat {
        if let Ok(fresh_msgs) = fetch_local_chat_messages(&local_client).await {
            merge_chat_messages(&mut match_chat, fresh_msgs);
        }
    } else {
        match_chat.clear();
    }

    let directory = Arc::new(merge_directories(&local_dir, &remote));
    let twitch_token =
        get_twitch_app_token(&public_client, &config.twitch_client_id, &config.twitch_client_secret).await;

    let mut players = players;
    // Recover any missing/hidden Riot IDs from captured match chat (matched by PUUID)
    if !match_chat.is_empty() {
        for p in &mut players {
            if is_visible_game_name(&p.record.game_name) {
                continue;
            }
            if let Some(msg) = match_chat.iter().find(|m| {
                !m.puuid.is_empty()
                    && m.puuid.eq_ignore_ascii_case(&p.record.subject)
                    && is_visible_game_name(&m.game_name)
            }) {
                p.record.game_name = msg.game_name.trim().to_string();
                if p.record.tag_line.trim().is_empty() && !msg.game_tag.trim().is_empty() {
                    p.record.tag_line = msg.game_tag.trim().to_string();
                }
            } else if match_chat.iter().any(|m| {
                !m.puuid.is_empty()
                    && m.puuid.eq_ignore_ascii_case(&p.record.subject)
                    && extract_chat_stream_plug(&m.body).is_some()
            }) {
                // Hidden/Streamer Mode player still plugged their stream in chat: synthesize label
                let label = if !p.agent_name.is_empty() {
                    format!("Hidden ({})", p.agent_name)
                } else {
                    "Hidden Player".to_string()
                };
                p.record.game_name = label;
            }
        }
    }

    // Exclude self, duplicates, (when enabled) party members, and hidden/blank Riot IDs
    let total_roster = players.len();
    let (scannable_players, party_skipped, hidden_skipped) =
        filter_scannable_players(players, &my_puuid, config.skip_party_members);
    let players_checked = scannable_players.len();
    log(format!(
        "Match roster {match_id}: total={total_roster}, checked={players_checked}, party_skipped={party_skipped}, hidden_skipped={hidden_skipped}"
    ));

    let sem = Arc::new(Semaphore::new(4));
    let mut set = JoinSet::new();
    for player_info in scannable_players {
        let mut best_plug: Option<ChatStreamPlug> = None;
        if config.capture_in_game_chat {
            for msg in &match_chat {
                let sender_matches = (!msg.puuid.is_empty() && msg.puuid.eq_ignore_ascii_case(&player_info.record.subject))
                    || (!msg.game_name.is_empty()
                        && msg.game_name.eq_ignore_ascii_case(&player_info.record.game_name)
                        && (msg.game_tag.is_empty()
                            || msg.game_tag.eq_ignore_ascii_case(&player_info.record.tag_line)));
                if !sender_matches {
                    continue;
                }
                if let Some(plug) = extract_chat_stream_plug(&msg.body) {
                    let take = match &best_plug {
                        None => true,
                        Some(prev) => plug.priority >= prev.priority,
                    };
                    if take {
                        best_plug = Some(plug);
                    }
                }
            }
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
            scan_player(
                client,
                cfg,
                dir,
                tok,
                mid,
                map_n,
                queue_n,
                effective_region,
                player_info,
                best_plug,
            )
            .await
        });
    }

    let mut found = 0usize;
    let mut copied_primary = false;
    while let Some(res) = set.join_next().await {
        match res {
            Ok(entries) => {
                for entry in entries {
                    let is_alertable = !entry.confidence_tier.eq_ignore_ascii_case("Unverified");
                    let should_copy = is_alertable && !copied_primary;
                    if should_copy {
                        copied_primary = true;
                    }
                    record_and_notify(&state, &ctx, entry, should_copy, true);
                    found += 1;
                }
            }
            Err(e) => log(format!("Player scan task failed: {e}")),
        }
    }

    let checked_time = Local::now().format("%d %b %Y | %I:%M %p").to_string();
    let summary = LastMatchSummary {
        match_id,
        queue_name: display_queue.to_string(),
        map_name: display_map.to_string(),
        my_agent,
        players_checked,
        party_skipped,
        hidden_skipped,
        streamers_found: found,
        scanned_at: checked_time,
        server_name: effective_region.label.to_string(),
    };
    save_json("last_match.json", &summary);
    lock(&state).last_match_summary = Some(summary);

    let mut skip_notes = Vec::new();
    if party_skipped > 0 {
        skip_notes.push(format!("{party_skipped} party skipped"));
    }
    if hidden_skipped > 0 {
        skip_notes.push(format!("{hidden_skipped} hidden"));
    }
    let extra_note = if skip_notes.is_empty() {
        String::new()
    } else {
        format!(", {}", skip_notes.join(", "))
    };
    let status = if found > 0 {
        format!(
            "Found {found} match result(s) in {display_queue} on {display_map} ({}{extra_note})!",
            effective_region.short_label
        )
    } else {
        format!(
            "Scanned {display_queue} on {display_map} ({players_checked} checked{extra_note}, {}) | No live streamers found",
            effective_region.short_label
        )
    };
    set_status(&state, &ctx, status);
}

// ============================================================================
// LOG WATCHER
// ============================================================================

static API_REGION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https://pd\.([a-z0-9]+)\.a\.pvp\.net").unwrap());
static POD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)aresriot\.[a-z0-9._-]+").unwrap());

/// Reads only *complete* lines appended after `start`. A half-written trailing line is left for
/// the next poll. Also extracts any Riot PD endpoint or game server pod seen in the log.
fn read_new_log_events(
    path: &Path,
    start: u64,
) -> std::io::Result<(Vec<String>, Option<String>, Option<String>, u64)> {
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
    let mut detected_api_region = None;
    let mut detected_server = None;

    for line in text.lines() {
        if line.contains("Reconcile") {
            reconcile_lines.push(line.to_string());
        }
        if line.contains(".a.pvp.net") {
            if let Some(caps) = API_REGION_RE.captures(line) {
                if let Some(s) = canonical_api_region(&caps[1]) {
                    detected_api_region = Some(s.to_string());
                }
            }
        }
        if line.contains("aresriot.") {
            if let Some(m) = POD_RE.find(line) {
                if let Some(srv) = parse_game_pod_to_server(m.as_str()) {
                    detected_server = Some(srv.to_string());
                }
            }
        }
    }
    Ok((reconcile_lines, detected_api_region, detected_server, start + end as u64))
}

/// Scans both the head (64 KB) and tail (512 KB) of `ShooterGame.log` at startup so long gaming
/// sessions still detect the latest server pod, Riot API shard, and whether Valorant is already
/// `InGame` when the app launches mid-match.
fn detect_initial_log_state(path: &Path) -> (Option<String>, Option<String>, bool) {
    let Ok(mut f) = File::open(path) else {
        return (None, None, false);
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut api_region = None;
    let mut server = None;
    let mut in_match = false;

    let scan_chunk = |text: &str, api_region: &mut Option<String>, server: &mut Option<String>, in_match: &mut bool| {
        for line in text.lines() {
            if line.contains(".a.pvp.net") {
                if let Some(caps) = API_REGION_RE.captures(line) {
                    if let Some(s) = canonical_api_region(&caps[1]) {
                        *api_region = Some(s.to_string());
                    }
                }
            }
            if line.contains("aresriot.") {
                if let Some(m) = POD_RE.find(line) {
                    if let Some(srv) = parse_game_pod_to_server(m.as_str()) {
                        *server = Some(srv.to_string());
                    }
                }
            }
            if line.contains("Reconcile") {
                if line.contains("InGame") {
                    *in_match = true;
                } else if line.contains("MainMenu") {
                    *in_match = false;
                }
            }
        }
    };

    // 1. Head (up to 64 KB) captures initial startup PD shard even if the log grew huge
    let mut head = Vec::new();
    if f.by_ref().take(64 * 1024).read_to_end(&mut head).is_ok() {
        let text = String::from_utf8_lossy(&head);
        scan_chunk(&text, &mut api_region, &mut server, &mut in_match);
    }

    // 2. Tail (last 512 KB) captures the latest server pod and live InGame/MainMenu state
    if len > 64 * 1024 {
        let tail_start = len.saturating_sub(512 * 1024);
        if f.seek(SeekFrom::Start(tail_start)).is_ok() {
            let mut tail = Vec::new();
            if f.read_to_end(&mut tail).is_ok() {
                let text = String::from_utf8_lossy(&tail);
                scan_chunk(&text, &mut api_region, &mut server, &mut in_match);
            }
        }
    }

    (api_region, server, in_match)
}

async fn watch_log(
    log_path: PathBuf,
    local_client: Client,
    state: Arc<Mutex<AppState>>,
    ctx: egui::Context,
    tx: mpsc::UnboundedSender<ScanTrigger>,
) {
    let (init_api_region, init_server, init_log_in_match) = detect_initial_log_state(&log_path);
    let init_riot_status = probe_riot_session(&local_client).await;
    let mut in_match = init_log_in_match && init_riot_status == RiotStatus::ValorantActive;
    {
        let mut s = lock(&state);
        s.riot_status = init_riot_status;
        if let Some(reg) = init_api_region {
            s.detected_api_region = reg;
        }
        if let Some(server) = init_server {
            s.detected_server = server;
        }
        if in_match {
            s.status = "In Match | Waiting for match end...".to_string();
        }
    }
    repaint(&ctx);

    let initial = fs::metadata(&log_path).ok();
    let mut last_pos = initial.as_ref().map(|m| m.len()).unwrap_or(0);
    let mut last_created = initial.and_then(|m| m.created().ok());
    let mut last_riot_status: Option<RiotStatus> = Some(init_riot_status);
    let mut last_probe: Option<Instant> = Some(Instant::now());
    let mut last_chat_poll: Option<Instant> = None;

    loop {
        let has_lockfile = read_riot_lockfile().is_ok();
        let should_probe = last_probe
            .map(|at| at.elapsed() >= Duration::from_secs(2))
            .unwrap_or(true);
        let next_status = if should_probe {
            last_probe = Some(Instant::now());
            probe_riot_session(&local_client).await
        } else {
            last_riot_status.unwrap_or(RiotStatus::Offline)
        };
        if last_riot_status != Some(next_status) {
            last_riot_status = Some(next_status);
            lock(&state).riot_status = next_status;
            repaint(&ctx);
        }

        // Silently buffer in-game chat messages while in match so we still have them after
        // Riot tears down the match chat room at MainMenu. Never notifies or displays during a match.
        if in_match && has_lockfile && lock(&state).config.capture_in_game_chat {
            let should_poll_chat = last_chat_poll
                .map(|at| at.elapsed() >= Duration::from_secs(4))
                .unwrap_or(true);
            if should_poll_chat {
                last_chat_poll = Some(Instant::now());
                if let Ok(msgs) = fetch_local_chat_messages(&local_client).await {
                    let mut s = lock(&state);
                    merge_chat_messages(&mut s.chat_buffer, msgs);
                }
            }
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
                Ok((lines, api_reg_opt, server_opt, new_pos)) => {
                    last_pos = new_pos;
                    if api_reg_opt.is_some() || server_opt.is_some() {
                        let mut changed = false;
                        {
                            let mut s = lock(&state);
                            if let Some(reg) = api_reg_opt {
                                if s.detected_api_region != reg {
                                    s.detected_api_region = reg;
                                    changed = true;
                                }
                            }
                            if let Some(srv) = server_opt {
                                if s.detected_server != srv {
                                    s.detected_server = srv;
                                    changed = true;
                                }
                            }
                        }
                        if changed {
                            repaint(&ctx);
                        }
                    }
                    for line in lines {
                        if line.contains("Pregame") {
                            lock(&state).chat_buffer.clear();
                        } else if line.contains("InGame") {
                            if !in_match {
                                in_match = true;
                                last_chat_poll = None;
                                // Keep any pregame chat messages from this lobby while clearing stale coregame chat.
                                lock(&state)
                                    .chat_buffer
                                    .retain(|m| m.cid.to_ascii_lowercase().contains("ares-pregame"));
                                set_status(&state, &ctx, "In Match | Waiting for match end...");
                            }
                        } else if in_match && line.contains("MainMenu") {
                            in_match = false;
                            // Capture any final "gg im live on yt" chat messages before Riot clears the room.
                            if has_lockfile && lock(&state).config.capture_in_game_chat {
                                if let Ok(msgs) = fetch_local_chat_messages(&local_client).await {
                                    let mut s = lock(&state);
                                    merge_chat_messages(&mut s.chat_buffer, msgs);
                                }
                            }
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

#[cfg(windows)]
mod win32_window {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicIsize, Ordering};

    const SW_SHOW: i32 = 5;
    const SW_RESTORE: i32 = 9;
    const WM_CLOSE: u32 = 0x0010;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;
    const DWMWA_BORDER_COLOR: u32 = 34;
    // COLORREF is 0x00BBGGRR -> matches CARD_BORDER (#1F1F26)
    const DWM_BORDER_COLORREF: u32 = 0x0026_1F1F;
    const WINDOW_TITLE: &str = "Valorant Streamer Radar";

    static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(
            lp_enum_func: unsafe extern "system" fn(isize, isize) -> i32,
            l_param: isize,
        ) -> i32;
        fn GetWindowThreadProcessId(h_wnd: isize, lpdw_process_id: *mut u32) -> u32;
        fn GetWindowTextW(h_wnd: isize, lp_string: *mut u16, n_max_count: i32) -> i32;
        fn IsWindow(h_wnd: isize) -> i32;
        fn IsIconic(h_wnd: isize) -> i32;
        fn ShowWindow(h_wnd: isize, n_cmd_show: i32) -> i32;
        fn BringWindowToTop(h_wnd: isize) -> i32;
        fn SetForegroundWindow(h_wnd: isize) -> i32;
        fn InvalidateRect(h_wnd: isize, lp_rect: *const c_void, b_erase: i32) -> i32;
        fn PostMessageW(h_wnd: isize, msg: u32, w_param: usize, l_param: isize) -> i32;
        fn SetWindowRgn(h_wnd: isize, h_rgn: isize, b_redraw: i32) -> i32;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            dw_attribute: u32,
            pv_attribute: *const c_void,
            cb_attribute: u32,
        ) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, cx: i32, cy: i32) -> isize;
    }

    struct SearchCtx {
        pid: u32,
        hwnd: isize,
    }

    unsafe extern "system" fn enum_windows_cb(hwnd: isize, l_param: isize) -> i32 {
        let ctx = &mut *(l_param as *mut SearchCtx);
        let mut win_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut win_pid);
        if win_pid != ctx.pid {
            return 1;
        }
        let mut buf = [0u16; 128];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            if title == WINDOW_TITLE {
                ctx.hwnd = hwnd;
                return 0; // stop enumerating
            }
        }
        1
    }

    fn find_main_hwnd() -> isize {
        let cached = MAIN_HWND.load(Ordering::Relaxed);
        if cached != 0 && unsafe { IsWindow(cached) } != 0 {
            return cached;
        }
        let mut ctx = SearchCtx {
            pid: std::process::id(),
            hwnd: 0,
        };
        unsafe {
            EnumWindows(enum_windows_cb, (&mut ctx as *mut SearchCtx) as isize);
        }
        if ctx.hwnd != 0 {
            MAIN_HWND.store(ctx.hwnd, Ordering::Relaxed);
        }
        ctx.hwnd
    }

    pub fn cache_main_hwnd() {
        let _ = find_main_hwnd();
    }

    pub fn apply_rounded_corners(width: u32, height: u32, scale: f64) {
        let hwnd = find_main_hwnd();
        if hwnd == 0 {
            return;
        }
        unsafe {
            let pref: u32 = DWMWCP_ROUND;
            let hr = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&pref as *const u32).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            );
            let border = DWM_BORDER_COLORREF;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                (&border as *const u32).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            );
            // Windows 10 fallback: DWM corner preference returns non-zero on Win10,
            // so clip the frameless window to an 8px-radius rounded rectangle via GDI.
            if hr != 0 && width > 0 && height > 0 {
                let diameter = ((16.0 * scale).round() as i32).max(12);
                let rgn = CreateRoundRectRgn(
                    0,
                    0,
                    width.saturating_add(1) as i32,
                    height.saturating_add(1) as i32,
                    diameter,
                    diameter,
                );
                if rgn != 0 {
                    SetWindowRgn(hwnd, rgn, 1);
                }
            }
        }
    }

    pub fn show_and_focus() {
        let hwnd = find_main_hwnd();
        if hwnd == 0 {
            return;
        }
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(dw_flags: u32, th32_process_id: u32) -> isize;
        fn Process32FirstW(h_snapshot: isize, lppe: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(h_snapshot: isize, lppe: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(h_object: isize) -> i32;
    }

    /// Inspects active Windows processes via Toolhelp32 without spawning any child processes.
    /// Returns `(valorant_game_running, riot_client_running)`.
    pub fn detect_riot_processes() -> (bool, bool) {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE || snap == 0 {
                return (false, false);
            }
            let mut entry: ProcessEntry32W = std::mem::zeroed();
            entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
            let mut valorant_running = false;
            let mut riot_client_running = false;

            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let nul = entry
                        .sz_exe_file
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.sz_exe_file.len());
                    let exe = String::from_utf16_lossy(&entry.sz_exe_file[..nul]);
                    if exe.eq_ignore_ascii_case("VALORANT-Win64-Shipping.exe") {
                        valorant_running = true;
                    } else if exe.eq_ignore_ascii_case("RiotClientServices.exe")
                        || exe.eq_ignore_ascii_case("RiotClientUx.exe")
                    {
                        riot_client_running = true;
                    }
                    if valorant_running && riot_client_running {
                        break;
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            (valorant_running, riot_client_running)
        }
    }

    pub fn post_close() {
        let hwnd = find_main_hwnd();
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
    }
}

#[cfg(not(windows))]
mod win32_window {
    pub fn cache_main_hwnd() {}
    pub fn apply_rounded_corners(_width: u32, _height: u32, _scale: f64) {}
    pub fn show_and_focus() {}
    pub fn post_close() {}
}

#[derive(Debug, Clone, Copy)]
enum AppUserEvent {
    RequestRepaint {
        when: Instant,
        cumulative_pass_nr: u64,
    },
    ShowWindow,
    Quit,
}

static EVENT_LOOP_PROXY: OnceLock<winit::event_loop::EventLoopProxy<AppUserEvent>> = OnceLock::new();

fn show_window(ctx: &egui::Context) {
    WINDOW_VISIBLE.store(true, Ordering::Relaxed);
    win32_window::show_and_focus();
    if let Some(proxy) = EVENT_LOOP_PROXY.get() {
        let _ = proxy.send_event(AppUserEvent::ShowWindow);
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

fn quit_app(ctx: &egui::Context) {
    QUIT.store(true, Ordering::Relaxed);
    if let Some(proxy) = EVENT_LOOP_PROXY.get() {
        let _ = proxy.send_event(AppUserEvent::Quit);
    }
    win32_window::post_close();
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    ctx.request_repaint();
    // Dropping out of run_app removes the tray icon cleanly; this is just a safety net.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        std::process::exit(0);
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    History,
    Directory,
    Discover,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeamFilter {
    All,
    Ally,
    Enemy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateRangeFilter {
    AllTime,
    Today,
    Last7Days,
    Last30Days,
}

impl DateRangeFilter {
    fn label(self) -> &'static str {
        match self {
            DateRangeFilter::AllTime => "All Time",
            DateRangeFilter::Today => "Today",
            DateRangeFilter::Last7Days => "Last 7 Days",
            DateRangeFilter::Last30Days => "Last 30 Days",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusFilter {
    All,
    Confirmed,
    Probable,
    Unverified,
    ChatPlug,
}

impl StatusFilter {
    fn label(self) -> &'static str {
        match self {
            StatusFilter::All => "All Statuses",
            StatusFilter::Confirmed => "Confirmed",
            StatusFilter::Probable => "Probable",
            StatusFilter::Unverified => "Unverified",
            StatusFilter::ChatPlug => "Chat Link",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectorySourceTab {
    Custom,
    Community,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoverPlatformFilter {
    All,
    Twitch,
    YouTube,
}

fn resolve_entry_tier(entry: &StreamerEntry) -> &'static str {
    if entry.confidence_tier.eq_ignore_ascii_case("Confirmed") {
        "Confirmed"
    } else if entry.confidence_tier.eq_ignore_ascii_case("Probable") {
        "Probable"
    } else if entry.confidence_tier.eq_ignore_ascii_case("Unverified") {
        "Unverified"
    } else if !entry.chat_message.is_empty()
        || entry.source.eq_ignore_ascii_case("chat_plug")
        || entry.source.eq_ignore_ascii_case("directory")
        || entry.confidence >= 85
    {
        "Confirmed"
    } else if entry.confidence >= 65 {
        "Probable"
    } else {
        "Unverified"
    }
}

fn entry_within_date_range(entry: &StreamerEntry, range: DateRangeFilter, now_local: chrono::DateTime<Local>) -> bool {
    if range == DateRangeFilter::AllTime {
        return true;
    }
    let parsed_date = if !entry.detected_at_iso.is_empty() {
        chrono::DateTime::parse_from_rfc3339(&entry.detected_at_iso)
            .ok()
            .map(|dt| dt.with_timezone(&Local).date_naive())
            .or_else(|| chrono::NaiveDate::parse_from_str(&entry.detected_at_iso, "%Y-%m-%d").ok())
    } else {
        None
    }
    .or_else(|| {
        // Legacy format fallback: "YYYY-MM-DD HH:MM AM/PM"
        entry
            .timestamp
            .get(0..10)
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    });

    let Some(entry_date) = parsed_date else {
        return false;
    };
    let today = now_local.date_naive();
    let days_ago = (today - entry_date).num_days();
    match range {
        DateRangeFilter::AllTime => true,
        DateRangeFilter::Today => days_ago == 0,
        DateRangeFilter::Last7Days => (0..=6).contains(&days_ago),
        DateRangeFilter::Last30Days => (0..=29).contains(&days_ago),
    }
}

#[allow(clippy::too_many_arguments)]
fn entry_matches_history_filters(
    entry: &StreamerEntry,
    search_query: &str,
    team_filter: TeamFilter,
    match_type_filter: &str,
    server_filter: &str,
    date_filter: DateRangeFilter,
    status_filter: StatusFilter,
    now_local: chrono::DateTime<Local>,
) -> bool {
    let team_ok = match team_filter {
        TeamFilter::All => true,
        TeamFilter::Ally => entry.team_relation.eq_ignore_ascii_case("Ally"),
        TeamFilter::Enemy => entry.team_relation.eq_ignore_ascii_case("Enemy"),
    };
    if !team_ok {
        return false;
    }

    if !match_type_filter.is_empty()
        && !match_type_filter.eq_ignore_ascii_case("all")
        && !entry.queue_name.eq_ignore_ascii_case(match_type_filter)
    {
        return false;
    }

    if !server_filter.is_empty() && !server_filter.eq_ignore_ascii_case("all") {
        let sf = server_filter.to_lowercase();
        let entry_srv = entry.server_region.to_lowercase();
        if !entry_srv.contains(&sf) && entry_srv != sf {
            return false;
        }
    }

    if !entry_within_date_range(entry, date_filter, now_local) {
        return false;
    }

    let tier = resolve_entry_tier(entry);
    let status_ok = match status_filter {
        StatusFilter::All => true,
        StatusFilter::Confirmed => tier == "Confirmed",
        StatusFilter::Probable => tier == "Probable",
        StatusFilter::Unverified => tier == "Unverified",
        StatusFilter::ChatPlug => !entry.chat_message.is_empty() || entry.source.eq_ignore_ascii_case("chat_plug"),
    };
    if !status_ok {
        return false;
    }

    let q = search_query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    entry.riot_id.to_lowercase().contains(&q)
        || entry.platform.to_lowercase().contains(&q)
        || entry.channel.to_lowercase().contains(&q)
        || entry.title.to_lowercase().contains(&q)
        || entry.chat_message.to_lowercase().contains(&q)
        || entry.map_name.to_lowercase().contains(&q)
        || entry.agent_name.to_lowercase().contains(&q)
        || entry.queue_name.to_lowercase().contains(&q)
        || entry.server_region.to_lowercase().contains(&q)
        || entry.match_reason.to_lowercase().contains(&q)
}

fn extract_channel_id_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let Some((_, rest)) = trimmed.split_once("://") else {
        return String::new();
    };
    let path = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
    let path = path.split('?').next().unwrap_or("");
    path.trim_matches('/').to_string()
}

fn format_viewers(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M watching", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K watching", count as f64 / 1_000.0)
    } else {
        format!("{count} watching")
    }
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

/// Smooth left-to-right ON/OFF toggle slider (replaces checkbox tick marks in Settings).
fn toggle_slider(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let desired_size = Vec2::new(46.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let how_on = ui.ctx().animate_bool_with_time(response.id, *on, 0.14);
    if ui.is_rect_visible(rect) {
        let radius = rect.height() * 0.5;
        let track_bg = Color32::from_rgb(
            (18.0 + (255.0 - 18.0) * how_on) as u8,
            (18.0 + (92.0 - 18.0) * how_on) as u8,
            (22.0 + (168.0 - 22.0) * how_on) as u8,
        );
        let border_col = if *on {
            ACCENT_BLUSH.linear_multiply(if response.hovered() { 0.95 } else { 0.7 })
        } else if response.hovered() {
            ACCENT_PINK.linear_multiply(0.55)
        } else {
            CARD_BORDER
        };
        let painter = ui.painter();
        let track_cr = CornerRadius::same(radius.round() as u8);
        painter.rect_filled(rect, track_cr, track_bg);
        painter.rect_stroke(rect, track_cr, Stroke::new(1.0, border_col), StrokeKind::Inside);

        let thumb_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let thumb_center = egui::pos2(thumb_x, rect.center().y);
        let thumb_r = radius - 3.0;
        painter.circle_filled(thumb_center, thumb_r, ACCENT_PEARL);
        if *on {
            painter.circle_stroke(thumb_center, thumb_r, Stroke::new(0.8, ACCENT_PINK.linear_multiply(0.45)));
        }
    }
    response
}

fn font_medium(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("medium".into()))
}

fn font_semibold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

fn font_bold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("bold".into()))
}

fn font_display(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("display".into()))
}

fn font_mono(size: f32) -> FontId {
    FontId::monospace(size)
}

fn font_mono_semi(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("mono_semi".into()))
}

fn install_preview_fonts(egui_ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "pjs_regular".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/PlusJakartaSans-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "pjs_medium".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/PlusJakartaSans-Medium.ttf"
        ))),
    );
    fonts.font_data.insert(
        "pjs_semibold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/PlusJakartaSans-SemiBold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "pjs_bold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/PlusJakartaSans-Bold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "barlow_bold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/BarlowCondensed-Bold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "jb_mono_regular".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "jb_mono_semibold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-SemiBold.ttf"
        ))),
    );

    let prop_fallbacks = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mono_fallbacks = fonts
        .families
        .get(&egui::FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        list.insert(0, "pjs_regular".to_owned());
    }
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        list.insert(0, "jb_mono_regular".to_owned());
    }

    let with_prop_fallback = |primary: &str| {
        let mut list = vec![primary.to_owned(), "pjs_regular".to_owned()];
        list.extend(prop_fallbacks.iter().cloned());
        list
    };
    let with_mono_fallback = |primary: &str| {
        let mut list = vec![primary.to_owned(), "jb_mono_regular".to_owned()];
        list.extend(mono_fallbacks.iter().cloned());
        list
    };

    fonts
        .families
        .insert(egui::FontFamily::Name("medium".into()), with_prop_fallback("pjs_medium"));
    fonts
        .families
        .insert(egui::FontFamily::Name("semibold".into()), with_prop_fallback("pjs_semibold"));
    fonts
        .families
        .insert(egui::FontFamily::Name("bold".into()), with_prop_fallback("pjs_bold"));

    let mut display_list = vec!["barlow_bold".to_owned(), "pjs_bold".to_owned()];
    display_list.extend(prop_fallbacks.iter().cloned());
    fonts
        .families
        .insert(egui::FontFamily::Name("display".into()), display_list);

    fonts
        .families
        .insert(egui::FontFamily::Name("mono_semi".into()), with_mono_fallback("jb_mono_semibold"));

    egui_ctx.set_fonts(fonts);
}

fn toggle_row(ui: &mut egui::Ui, on: &mut bool, title: &str, subtitle: &str) -> bool {
    let mut changed = false;
    Frame::NONE
        .fill(BG_DARK)
        .stroke(Stroke::new(1.0, if *on { ACCENT_PINK.linear_multiply(0.35) } else { CARD_BORDER }))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 104.0).max(180.0));
                    ui.label(RichText::new(title).font(font_semibold(13.5)).color(TEXT_PRIMARY));
                    if !subtitle.is_empty() {
                        ui.label(RichText::new(subtitle).color(TEXT_MUTED).size(12.5));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if toggle_slider(ui, on).changed() {
                        changed = true;
                    }
                    ui.add_space(6.0);
                    let state_text = if *on { "ON" } else { "OFF" };
                    let state_col = if *on { ACCENT_BLUSH } else { TEXT_MUTED };
                    ui.label(RichText::new(state_text).font(font_mono_semi(11.5)).color(state_col));
                });
            });
        });
    changed
}

/// Compact vector brand emblem for the custom AMOLED title bar (matches build.rs .exe & RadarIcon.tsx).
fn paint_brand_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let p = ui.painter();
    let box_rect = rect.shrink(0.5);
    let r = (box_rect.width() * 0.18).max(3.0).round() as u8;
    p.rect_filled(box_rect, CornerRadius::same(r), BG_DARK);
    p.rect_stroke(box_rect, CornerRadius::same(r), Stroke::new(1.3, ACCENT_PINK), StrokeKind::Inside);
    let cx = box_rect.center().x;
    let cy = box_rect.center().y;
    let half_w = box_rect.width() * 0.24;
    let half_h = box_rect.height() * 0.24;
    let l = egui::pos2(cx - half_w, cy - half_h);
    let b = egui::pos2(cx, cy + half_h);
    let r_pt = egui::pos2(cx + half_w, cy - half_h);
    let stroke = Stroke::new(1.9, ACCENT_PEARL);
    p.line_segment([l, b], stroke);
    p.line_segment([b, r_pt], stroke);
    p.circle_filled(egui::pos2(cx, cy - half_h + 0.8), 1.45, ACCENT_PINK);
}

/// Square icon-only Copy button (overlapping-sheets clipboard glyph) with hover tooltip.
fn paint_copy_icon_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let fill = if hovered { SURFACE_HOVER } else { BTN_SECONDARY };
    let border = if hovered { ACCENT_PINK.linear_multiply(0.55) } else { CARD_BORDER };
    let icon_col = if hovered { ACCENT_PEARL } else { TEXT_MUTED };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(6), fill);
    p.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, border), StrokeKind::Inside);

    let c = rect.center();
    let stroke = Stroke::new(1.4, icon_col);
    // Front sheet (bottom-right)
    let front = egui::Rect::from_min_size(egui::pos2(c.x - 2.5, c.y - 2.5), Vec2::new(8.5, 9.5));
    p.rect_stroke(front, CornerRadius::same(2), stroke, StrokeKind::Inside);
    // Back sheet outline (top-left L-edge)
    let bx = c.x - 5.5;
    let by = c.y - 6.0;
    p.line_segment([egui::pos2(bx, by + 7.5), egui::pos2(bx, by)], stroke);
    p.line_segment([egui::pos2(bx, by), egui::pos2(bx + 6.5, by)], stroke);

    resp.on_hover_text("Copy stream link")
}

/// Minimal chevron cue indicating expandable card details.
fn paint_chevron_icon(ui: &mut egui::Ui, expanded: bool, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
    let c = rect.center();
    let s = Stroke::new(1.5, color);
    if expanded {
        ui.painter().line_segment([egui::pos2(c.x - 4.0, c.y + 2.0), egui::pos2(c.x, c.y - 2.0)], s);
        ui.painter().line_segment([egui::pos2(c.x, c.y - 2.0), egui::pos2(c.x + 4.0, c.y + 2.0)], s);
    } else {
        ui.painter().line_segment([egui::pos2(c.x - 4.0, c.y - 2.0), egui::pos2(c.x, c.y + 2.0)], s);
        ui.painter().line_segment([egui::pos2(c.x, c.y + 2.0), egui::pos2(c.x + 4.0, c.y - 2.0)], s);
    }
}

/// Minimal vector sidebar icons matching the preview (Clock, Book, Compass, Gear).
fn paint_nav_icon(p: &egui::Painter, c: egui::Pos2, tab: ActiveTab, color: Color32) {
    let s = Stroke::new(1.4, color);
    match tab {
        ActiveTab::History => {
            p.circle_stroke(c, 5.8, s);
            p.line_segment([c, egui::pos2(c.x, c.y - 3.2)], s);
            p.line_segment([c, egui::pos2(c.x + 2.6, c.y + 1.4)], s);
        }
        ActiveTab::Directory => {
            let r = egui::Rect::from_center_size(c, Vec2::new(11.0, 10.5));
            p.rect_stroke(r, CornerRadius::same(2), s, StrokeKind::Inside);
            p.line_segment([egui::pos2(c.x, r.top() + 1.0), egui::pos2(c.x, r.bottom() - 1.0)], s);
        }
        ActiveTab::Discover => {
            p.circle_stroke(c, 5.8, s);
            p.line_segment([egui::pos2(c.x - 2.5, c.y + 2.5), egui::pos2(c.x + 2.5, c.y - 2.5)], s);
            p.circle_filled(egui::pos2(c.x + 2.2, c.y - 2.2), 1.1, color);
        }
        ActiveTab::Settings => {
            p.circle_stroke(c, 2.3, s);
            p.circle_stroke(c, 5.4, Stroke::new(1.3, color));
        }
    }
}

/// Custom frameless title bar Minimize button (vector-drawn dash).
fn paint_minimize_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(42.0, 38.0), egui::Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, CornerRadius::ZERO, SURFACE_HOVER);
    }
    let col = if hovered { TEXT_PRIMARY } else { TEXT_MUTED };
    let cy = rect.center().y + 1.0;
    ui.painter().line_segment(
        [egui::pos2(rect.center().x - 5.5, cy), egui::pos2(rect.center().x + 5.5, cy)],
        Stroke::new(1.5, col),
    );
    resp.on_hover_text("Minimize")
}

/// Custom frameless title bar Close button (hides window to system tray).
fn paint_close_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(42.0, 38.0), egui::Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(190, 24, 93));
    }
    let col = if hovered { Color32::WHITE } else { TEXT_MUTED };
    let c = rect.center();
    let d = 5.0;
    let stroke = Stroke::new(1.5, col);
    ui.painter().line_segment([egui::pos2(c.x - d, c.y - d), egui::pos2(c.x + d, c.y + d)], stroke);
    ui.painter().line_segment([egui::pos2(c.x + d, c.y - d), egui::pos2(c.x - d, c.y + d)], stroke);
    resp.on_hover_text("Hide to system tray")
}

/// Handles perimeter edge & corner resizing for the frameless custom Windows viewport.
fn handle_frameless_resize(ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let edge = 5.0;
    let corner = 10.0;
    let (hover_pos, primary_pressed) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_pressed()));
    let Some(pos) = hover_pos else {
        return;
    };
    let left = pos.x <= screen.left() + edge;
    let right = pos.x >= screen.right() - edge;
    let top = pos.y <= screen.top() + edge;
    let bottom = pos.y >= screen.bottom() - edge;
    let c_left = pos.x <= screen.left() + corner;
    let c_right = pos.x >= screen.right() - corner;
    let c_top = pos.y <= screen.top() + corner;
    let c_bottom = pos.y >= screen.bottom() - corner;

    let dir = if (top && c_left) || (left && c_top) {
        Some((egui::ResizeDirection::NorthWest, egui::CursorIcon::ResizeNwSe))
    } else if (top && c_right) || (right && c_top) {
        Some((egui::ResizeDirection::NorthEast, egui::CursorIcon::ResizeNeSw))
    } else if (bottom && c_left) || (left && c_bottom) {
        Some((egui::ResizeDirection::SouthWest, egui::CursorIcon::ResizeNeSw))
    } else if (bottom && c_right) || (right && c_bottom) {
        Some((egui::ResizeDirection::SouthEast, egui::CursorIcon::ResizeNwSe))
    } else if top {
        Some((egui::ResizeDirection::North, egui::CursorIcon::ResizeVertical))
    } else if bottom {
        Some((egui::ResizeDirection::South, egui::CursorIcon::ResizeVertical))
    } else if left {
        Some((egui::ResizeDirection::West, egui::CursorIcon::ResizeHorizontal))
    } else if right {
        Some((egui::ResizeDirection::East, egui::CursorIcon::ResizeHorizontal))
    } else {
        None
    };

    if let Some((resize_dir, cursor)) = dir {
        ctx.set_cursor_icon(cursor);
        if primary_pressed {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(resize_dir));
        }
    }
}

fn paint_top_gradient_bar(painter: &egui::Painter, screen: egui::Rect, corner_r: f32, height: f32) {
    let left = screen.left() + 1.0;
    let right = screen.right() - 1.0;
    let top_y = screen.top() + 1.0;
    let bottom_y = top_y + height;
    let width = (right - left).max(1.0);
    let steps = 64usize;
    let step_w = width / steps as f32;
    for i in 0..steps {
        let x0 = left + i as f32 * step_w;
        let x1 = (x0 + step_w + 0.6).min(right);
        let xm = (x0 + x1) * 0.5;
        let edge_d = (xm - screen.left()).min(screen.right() - xm).max(0.0);
        let curve_top = if edge_d < corner_r {
            screen.top() + corner_r - (corner_r * corner_r - (corner_r - edge_d).powi(2)).max(0.0).sqrt()
        } else {
            screen.top()
        };
        let y0 = top_y.max(curve_top + 0.7);
        if bottom_y > y0 {
            let t = (i as f32 + 0.5) / steps as f32;
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, bottom_y)),
                CornerRadius::ZERO,
                pink_white_color(t),
            );
        }
    }
}

fn segmented_tab_btn<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: &str,
) -> bool {
    let selected = *current == value;
    let fill = if selected { SURFACE_HOVER } else { Color32::TRANSPARENT };
    let text_col = if selected { ACCENT_PEARL } else { TEXT_MUTED };
    let font = if selected { font_semibold(13.0) } else { font_medium(13.0) };
    let stroke = if selected {
        Stroke::new(1.0, Color32::from_rgb(44, 44, 54))
    } else {
        Stroke::NONE
    };
    let btn = egui::Button::new(RichText::new(label).font(font).color(text_col))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(5))
        .min_size(Vec2::new(0.0, 28.0));
    let resp = ui.add(btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() && !selected {
        *current = value;
        return true;
    }
    false
}

fn gradient_button(ui: &mut egui::Ui, label: &str, size: Vec2, enabled: bool, font_size: f32) -> egui::Response {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter();
    let r = 6.0f32.min(rect.height() * 0.5);
    let cr = CornerRadius::same(r.round() as u8);

    if !enabled {
        painter.rect_filled(rect, cr, CARD_BORDER);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font_bold(font_size),
            TEXT_MUTED,
        );
        return response;
    }

    let hover_boost = if response.hovered() { 0.14 } else { 0.0 };
    let steps = 32usize;
    let step_w = rect.width() / steps as f32;
    for i in 0..steps {
        let x0 = rect.left() + i as f32 * step_w;
        let x1 = (x0 + step_w + 0.6).min(rect.right());
        let xm = (x0 + x1) * 0.5;
        let edge_d = (xm - rect.left()).min(rect.right() - xm).max(0.0);
        let inset = if edge_d < r {
            r - (r * r - (r - edge_d).powi(2)).max(0.0).sqrt()
        } else {
            0.0
        };
        let t = ((i as f32 + 0.5) / steps as f32 + hover_boost).clamp(0.0, 1.0);
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0, rect.top() + inset), egui::pos2(x1, rect.bottom() - inset)),
            CornerRadius::ZERO,
            pink_white_color(t),
        );
    }
    painter.rect_stroke(
        rect,
        cr,
        Stroke::new(1.0, ACCENT_PEARL.linear_multiply(if response.hovered() { 0.9 } else { 0.5 })),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font_bold(font_size),
        BG_DARK,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
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
            Frame::NONE
                .fill(LIVE_GREEN.linear_multiply(0.12))
                .stroke(Stroke::new(1.0, LIVE_GREEN.linear_multiply(0.55)))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        paint_check_icon(ui, LIVE_GREEN);
                        ui.label(RichText::new(msg).color(LIVE_GREEN).size(12.0));
                    });
                });
        }
        CheckState::Warn { msg, suggested_name } => {
            Frame::NONE
                .fill(ENEMY_AMBER.linear_multiply(0.12))
                .stroke(Stroke::new(1.0, ENEMY_AMBER.linear_multiply(0.55)))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        paint_warn_icon(ui, ENEMY_AMBER);
                        ui.label(RichText::new(msg).color(ENEMY_AMBER).size(12.0));
                    });
                    if let Some(name) = suggested_name {
                        ui.add_space(5.0);
                        let use_btn = egui::Button::new(
                            RichText::new(format!("Use detected name: {name}"))
                                .font(font_semibold(11.5))
                                .color(Color32::WHITE),
                        )
                        .fill(BTN_SECONDARY)
                        .stroke(Stroke::new(1.0, ENEMY_AMBER))
                        .corner_radius(CornerRadius::same(4));
                        if ui.add(use_btn).clicked() {
                            apply_name = Some(name.clone());
                        }
                    }
                });
        }
        CheckState::Err(msg) => {
            Frame::NONE
                .fill(ERR_ROSE.linear_multiply(0.12))
                .stroke(Stroke::new(1.0, ERR_ROSE.linear_multiply(0.6)))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        paint_cross_icon(ui, ERR_ROSE);
                        ui.label(RichText::new(msg).color(ERR_ROSE).size(12.0));
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
    // History filters (non-destructive)
    search_query: String,
    team_filter: TeamFilter,
    match_type_filter: String,
    server_filter: String,
    date_filter: DateRangeFilter,
    status_filter: StatusFilter,
    show_filters_popover: bool,
    expanded_cards: HashSet<String>,
    // Update banner changelog viewer
    show_update_changelog: bool,
    // Directory manager state
    dir_source_tab: DirectorySourceTab,
    dir_search: String,
    // Discover / Recommendations filter
    discover_platform: DiscoverPlatformFilter,
    discover_server_override: String,
    // Add / Edit modal state
    show_add_modal: bool,
    editing_original: Option<(String, String)>,
    input_riot_id: String,
    input_aliases: String,
    input_platform: String,
    input_channel_name: String,
    input_channel_url: String,
    input_server_region: String,
    modal_error: Option<String>,
    // Settings draft state
    cfg_draft: AppConfig,
    show_api_secrets: bool,
    settings_error: Option<String>,
}

impl StreamerApp {
    fn new(
        egui_ctx: &egui::Context,
        state: Arc<Mutex<AppState>>,
        tx: mpsc::UnboundedSender<ScanTrigger>,
        bg_tx: mpsc::UnboundedSender<BgTask>,
        start_hidden: bool,
    ) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG_DARK;
        visuals.window_fill = CARD_BG;
        visuals.window_stroke = Stroke::new(1.0, CARD_BORDER);
        visuals.window_corner_radius = CornerRadius::same(8);
        visuals.menu_corner_radius = CornerRadius::same(6);
        visuals.extreme_bg_color = Color32::from_rgb(8, 8, 11);
        visuals.faint_bg_color = CARD_BG;

        visuals.widgets.noninteractive.bg_fill = CARD_BG;
        visuals.widgets.noninteractive.weak_bg_fill = CARD_BG;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, CARD_BORDER);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);

        visuals.widgets.inactive.bg_fill = BTN_SECONDARY;
        visuals.widgets.inactive.weak_bg_fill = BTN_SECONDARY;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, CARD_BORDER);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(6);

        visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
        visuals.widgets.hovered.weak_bg_fill = SURFACE_HOVER;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_PINK.linear_multiply(0.55));
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, ACCENT_PEARL);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(6);

        visuals.widgets.active.bg_fill = Color32::from_rgb(28, 28, 36);
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(28, 28, 36);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_PINK.linear_multiply(0.8));
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, ACCENT_PEARL);
        visuals.widgets.active.corner_radius = CornerRadius::same(6);

        visuals.widgets.open.bg_fill = BTN_SECONDARY;
        visuals.widgets.open.weak_bg_fill = BTN_SECONDARY;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT_PINK.linear_multiply(0.7));
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, ACCENT_PEARL);
        visuals.widgets.open.corner_radius = CornerRadius::same(6);

        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(255, 92, 168, 56);
        visuals.selection.stroke = Stroke::new(1.0, ACCENT_PINK);
        egui_ctx.set_visuals(visuals);

        install_preview_fonts(egui_ctx);

        // Typography scale, weights, and input/dropdown sizing matching the preview
        egui_ctx.style_mut(|style| {
            style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
            style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(13.5));
            style.text_styles.insert(egui::TextStyle::Button, font_medium(13.0));
            style.text_styles.insert(egui::TextStyle::Monospace, font_mono(12.5));
            style.text_styles.insert(egui::TextStyle::Heading, font_bold(18.0));
            style.spacing.button_padding = Vec2::new(10.0, 6.0);
            style.spacing.interact_size.y = 30.0;
            style.spacing.combo_height = 240.0;
        });

        let initial_cfg = lock(&state).config.clone();

        Self {
            state,
            manual_scan_tx: tx,
            bg_tx,
            pending_initial_hide: start_hidden,
            active_tab: ActiveTab::History,
            search_query: String::new(),
            team_filter: TeamFilter::All,
            match_type_filter: "All".to_string(),
            server_filter: "All".to_string(),
            date_filter: DateRangeFilter::AllTime,
            status_filter: StatusFilter::All,
            show_filters_popover: false,
            expanded_cards: HashSet::new(),
            show_update_changelog: false,
            dir_source_tab: DirectorySourceTab::Custom,
            dir_search: String::new(),
            discover_platform: DiscoverPlatformFilter::All,
            discover_server_override: "auto".to_string(),
            show_add_modal: false,
            editing_original: None,
            input_riot_id: String::new(),
            input_aliases: String::new(),
            input_platform: "YouTube".to_string(),
            input_channel_name: String::new(),
            input_channel_url: String::new(),
            input_server_region: String::new(),
            modal_error: None,
            cfg_draft: initial_cfg,
            show_api_secrets: false,
            settings_error: None,
        }
    }

    fn active_popover_filter_count(&self) -> usize {
        let mut count = 0usize;
        if self.team_filter != TeamFilter::All {
            count += 1;
        }
        if !self.match_type_filter.eq_ignore_ascii_case("All") {
            count += 1;
        }
        if !self.server_filter.eq_ignore_ascii_case("All") {
            count += 1;
        }
        if self.date_filter != DateRangeFilter::AllTime {
            count += 1;
        }
        if self.status_filter != StatusFilter::All {
            count += 1;
        }
        count
    }

    fn has_active_history_filters(&self) -> bool {
        !self.search_query.trim().is_empty() || self.active_popover_filter_count() > 0
    }

    fn reset_history_filters(&mut self) {
        self.search_query.clear();
        self.team_filter = TeamFilter::All;
        self.match_type_filter = "All".to_string();
        self.server_filter = "All".to_string();
        self.date_filter = DateRangeFilter::AllTime;
        self.status_filter = StatusFilter::All;
    }

    fn discover_region(&self, active_region: &'static ServerRegionInfo) -> &'static ServerRegionInfo {
        if self.discover_server_override.eq_ignore_ascii_case("auto") {
            active_region
        } else {
            server_region_by_code(&self.discover_server_override).unwrap_or(active_region)
        }
    }

    fn trigger_recommendations_fetch(&self, region_code: &str, force: bool) {
        let cfg = {
            let mut s = lock(&self.state);
            if !s.config.enable_recommendations {
                return;
            }
            if !force {
                if let Some((cached_reg, fetched_at, cached_items, label)) = &s.recommendations_cache {
                    if cached_reg.eq_ignore_ascii_case(region_code)
                        && fetched_at.elapsed() < RECOMMENDATIONS_CACHE_TTL
                    {
                        s.recommendations = RecommendationsState::Loaded(cached_items.clone(), label.clone());
                        return;
                    }
                }
            }
            s.recommendations = RecommendationsState::Loading;
            s.config.clone()
        };
        let _ = self.bg_tx.send(BgTask::FetchRecommendations {
            server_region: region_code.to_string(),
            youtube_api_key: cfg.youtube_api_key,
            twitch_client_id: cfg.twitch_client_id,
            twitch_client_secret: cfg.twitch_client_secret,
            force,
        });
    }

    fn open_add_modal(&mut self, edit: Option<&DirectoryStreamer>) {
        if let Some(d) = edit {
            self.editing_original = Some((d.riot_id.clone(), d.platform.clone()));
            self.input_riot_id = d.riot_id.clone();
            self.input_aliases = d.aliases.join(", ");
            self.input_platform = d.platform.clone();
            self.input_channel_name = d.channel_name.clone();
            self.input_channel_url = d.channel_url.clone();
            self.input_server_region = d.server_region.clone();
        } else {
            self.editing_original = None;
            self.input_riot_id.clear();
            self.input_aliases.clear();
            self.input_platform = "YouTube".to_string();
            self.input_channel_name.clear();
            self.input_channel_url.clear();
            let active_code = {
                let s = lock(&self.state);
                effective_server_region(&s.config, &s.detected_server, &s.detected_api_region)
                    .code
                    .to_string()
            };
            self.input_server_region = active_code;
        }
        self.modal_error = None;
        lock(&self.state).streamer_verify = CheckState::Idle;
        self.show_add_modal = true;
    }

    fn open_add_modal_from_rec(&mut self, rec: &RecommendedStreamer, region_code: &str) {
        self.editing_original = None;
        self.input_riot_id.clear();
        self.input_aliases.clear();
        self.input_platform = rec.platform.clone();
        self.input_channel_name = rec.channel.clone();
        self.input_channel_url = rec.url.clone();
        self.input_server_region = region_code.to_string();
        self.modal_error = None;
        lock(&self.state).streamer_verify = CheckState::Idle;
        self.show_add_modal = true;
    }

    fn submit_modal(&mut self, open_github: bool) {
        let rid = self.input_riot_id.trim().to_string();
        let cname = self.input_channel_name.trim().to_string();
        let curl = self.input_channel_url.trim().to_string();
        let platform = self.input_platform.clone();
        let server_region = self.input_server_region.trim().to_string();
        let aliases = parse_aliases_csv(&self.input_aliases, &rid);

        if let Err(msg) = validate_submission(&rid, &platform, &cname, &curl) {
            self.modal_error = Some(msg);
            return;
        }

        let slug = format!(
            "{}-{}",
            platform.to_lowercase(),
            normalize(&cname)
        );
        let new_entry = DirectoryStreamer {
            id: slug,
            display_name: cname.clone(),
            riot_id: rid.clone(),
            aliases,
            platform: platform.clone(),
            channel_id: extract_channel_id_from_url(&curl),
            channel_name: cname,
            channel_url: curl.clone(),
            server_region,
            verified: false,
            submitted_at: Local::now().to_rfc3339(),
            source: if open_github { "community_submission".to_string() } else { "local".to_string() },
        };

        let norm_url = curl.trim_end_matches('/').to_lowercase();
        {
            let mut s = lock(&self.state);
            if let Some((orig_rid, orig_plat)) = self.editing_original.take() {
                s.local_directory
                    .retain(|x| !(x.riot_id.eq_ignore_ascii_case(&orig_rid) && x.platform == orig_plat));
            }
            // Prevent duplicates by both (riot_id, platform) and normalized (channel_url, platform).
            s.local_directory.retain(|x| {
                let same_rid = x.riot_id.eq_ignore_ascii_case(&rid) && x.platform.eq_ignore_ascii_case(&platform);
                let same_url = x.channel_url.trim_end_matches('/').to_lowercase() == norm_url
                    && x.platform.eq_ignore_ascii_case(&platform);
                !(same_rid || same_url)
            });
            s.local_directory.push(new_entry.clone());
            save_json("custom_streamers.json", &s.local_directory);
        }

        if open_github {
            set_toast(&self.state, format!("Saved {rid} locally & opened moderated community submission!"));
            let issue_title = format!("Add Streamer: {rid}");
            let json_payload = serde_json::to_string_pretty(&new_entry).unwrap_or_default();
            let issue_body = format!(
                "### New Streamer Submission\n\nThis submission will be reviewed by a maintainer before being merged into `streamers.json`.\n\n```json\n{json_payload}\n```"
            );
            let gh_issue_url = format!(
                "https://github.com/{COMMUNITY_REPO_OWNER}/{COMMUNITY_REPO_NAME}/issues/new?labels=streamer-database&title={}&body={}",
                urlencode(&issue_title),
                urlencode(&issue_body)
            );
            let _ = open::that(gh_issue_url);
        } else {
            set_toast(&self.state, format!("Saved {rid} to custom_streamers.json"));
        }

        self.input_riot_id.clear();
        self.input_aliases.clear();
        self.input_channel_name.clear();
        self.input_channel_url.clear();
        self.input_server_region.clear();
        self.modal_error = None;
        self.show_add_modal = false;
    }

    fn save_settings(&mut self) {
        self.cfg_draft.youtube_api_key = self.cfg_draft.youtube_api_key.trim().to_string();
        self.cfg_draft.twitch_client_id = self.cfg_draft.twitch_client_id.trim().to_string();
        self.cfg_draft.twitch_client_secret = self.cfg_draft.twitch_client_secret.trim().to_string();

        self.cfg_draft.use_custom_toast_app_id = true;

        save_json("config.json", &self.cfg_draft);
        set_autostart(self.cfg_draft.start_with_windows);
        register_toast_app_id(true);
        *lock(&TWITCH_TOKEN) = None; // reset cached token if Twitch keys changed
        {
            let mut s = lock(&self.state);
            s.config = self.cfg_draft.clone();
            if !s.config.enable_recommendations {
                s.recommendations = RecommendationsState::Idle;
                s.recommendations_cache = None;
            }
        }
        if !self.cfg_draft.enable_recommendations && self.active_tab == ActiveTab::Discover {
            self.active_tab = ActiveTab::History;
        }
        self.settings_error = None;
        set_toast(&self.state, "Settings saved to config.json!");
    }
}

impl StreamerApp {
    fn update_ui(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if QUIT.load(Ordering::Relaxed) {
                return; // let the real quit through
            }
            win32_window::cache_main_hwnd();
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            WINDOW_VISIBLE.store(false, Ordering::Relaxed);
            return;
        }

        if self.pending_initial_hide {
            self.pending_initial_hide = false;
            win32_window::cache_main_hwnd();
            WINDOW_VISIBLE.store(false, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }

        // Never render a blank screen if the OS window is actually focused/visible.
        if !WINDOW_VISIBLE.load(Ordering::Relaxed) {
            if ctx.input(|i| i.viewport().focused == Some(true)) {
                WINDOW_VISIBLE.store(true, Ordering::Relaxed);
            } else {
                return;
            }
        }

        handle_frameless_resize(ctx);

        let (
            status,
            is_scanning,
            is_syncing_dir,
            riot_status,
            active_region,
            detected_server_code,
            recs_enabled,
            recommendations,
            history,
            local_dir,
            remote_dir,
            toast,
            last_match_summary,
            streamer_verify,
            yt_key_check,
            twitch_key_check,
            update_state,
        ) = {
            let mut s = lock(&self.state);
            if matches!(&s.toast, Some((_, at)) if at.elapsed() > TOAST_TTL) {
                s.toast = None;
            }
            let region = effective_server_region(&s.config, &s.detected_server, &s.detected_api_region);
            (
                s.status.clone(),
                s.is_scanning,
                s.is_syncing_directory,
                s.riot_status,
                region,
                s.detected_server.clone(),
                s.config.enable_recommendations,
                s.recommendations.clone(),
                s.history.clone(),
                s.local_directory.clone(),
                s.remote_directory.clone(),
                s.toast.as_ref().map(|(m, _)| m.clone()),
                s.last_match_summary.clone(),
                s.streamer_verify.clone(),
                s.yt_key_check.clone(),
                s.twitch_key_check.clone(),
                s.update_state.clone(),
            )
        };
        if toast.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if !recs_enabled && self.active_tab == ActiveTab::Discover {
            self.active_tab = ActiveTab::History;
        }

        // Reserve 3.5px at the top for the curved Pink-to-Pearl foreground accent hairline
        egui::TopBottomPanel::top("accent_strip")
            .exact_height(3.5)
            .show_separator_line(false)
            .frame(Frame::NONE.fill(BG_DARK))
            .show(ctx, |_ui| {});

        // Custom 38px AMOLED Black draggable title bar (Minimize & Close-to-tray only)
        egui::TopBottomPanel::top("custom_titlebar")
            .exact_height(38.0)
            .show_separator_line(false)
            .frame(Frame::NONE.fill(BG_DARK))
            .show(ctx, |ui| {
                let bar_rect = ui.max_rect();
                let drag_rect = egui::Rect::from_min_max(
                    bar_rect.min,
                    egui::pos2((bar_rect.right() - 86.0).max(bar_rect.left()), bar_rect.bottom()),
                );
                let drag_resp = ui.interact(drag_rect, egui::Id::new("custom_titlebar_drag"), egui::Sense::drag());
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(12.0);
                    paint_brand_mark(ui, 18.0);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("VALORANT")
                            .font(font_display(16.5))
                            .color(ACCENT_PINK),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("RADAR")
                            .font(font_display(16.5))
                            .color(ACCENT_PEARL),
                    );
                    if let Some(msg) = &toast {
                        ui.add_space(10.0);
                        ui.label(RichText::new("/").color(CARD_BORDER).size(12.5));
                        ui.add_space(8.0);
                        ui.label(RichText::new(short(msg, 64)).color(ACCENT_BLUSH).size(12.5));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if paint_close_button(ui).clicked() {
                            win32_window::cache_main_hwnd();
                            WINDOW_VISIBLE.store(false, Ordering::Relaxed);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                        }
                        if paint_minimize_button(ui).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });

                // Subtle 1px bottom divider under the title bar only
                ui.painter().line_segment(
                    [
                        egui::pos2(bar_rect.left(), bar_rect.bottom() - 0.5),
                        egui::pos2(bar_rect.right(), bar_rect.bottom() - 0.5),
                    ],
                    Stroke::new(1.0, CARD_BORDER),
                );
            });

        // Left Sidebar Navigation Rail (236px AMOLED Black)
        egui::SidePanel::left("nav_sidebar")
            .exact_width(236.0)
            .resizable(false)
            .frame(
                Frame::NONE
                    .fill(BG_DARK)
                    .inner_margin(Margin::symmetric(14, 16))
                    .stroke(Stroke::new(1.0, CARD_BORDER)),
            )
            .show(ctx, |ui| {
                // Navigation items
                let dir_total = local_dir.len() + remote_dir.len();
                let mut nav_items: Vec<(ActiveTab, &str, String)> = vec![
                    (ActiveTab::History, "History", history.len().to_string()),
                    (ActiveTab::Directory, "Directory", dir_total.to_string()),
                ];
                if recs_enabled {
                    nav_items.push((ActiveTab::Discover, "Discover", "Live".to_string()));
                }
                nav_items.push((ActiveTab::Settings, "Settings", String::new()));

                for (tab, label, meta) in nav_items {
                    let selected = self.active_tab == tab;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), egui::Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let fill = if selected {
                        BTN_SECONDARY
                    } else if resp.hovered() {
                        CARD_BG
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                    // Match preview: crisp 2px vertical pink indicator on the left edge of the active tab
                    if selected {
                        let bar = egui::Rect::from_min_max(
                            egui::pos2(rect.left(), rect.top() + 6.0),
                            egui::pos2(rect.left() + 2.5, rect.bottom() - 6.0),
                        );
                        ui.painter().rect_filled(bar, CornerRadius::same(1), ACCENT_PINK);
                    }

                    let icon_col = if selected {
                        ACCENT_PINK
                    } else if resp.hovered() {
                        TEXT_PRIMARY
                    } else {
                        Color32::from_rgb(90, 98, 117)
                    };
                    paint_nav_icon(ui.painter(), egui::pos2(rect.left() + 18.0, rect.center().y), tab, icon_col);

                    let text_col = if selected {
                        ACCENT_PEARL
                    } else if resp.hovered() {
                        TEXT_PRIMARY
                    } else {
                        TEXT_MUTED
                    };
                    ui.painter().text(
                        egui::pos2(rect.left() + 34.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        if selected { font_semibold(13.5) } else { font_medium(13.5) },
                        text_col,
                    );
                    if !meta.is_empty() {
                        let meta_col = if tab == ActiveTab::Discover && recs_enabled {
                            LIVE_GREEN
                        } else if selected {
                            ACCENT_BLUSH
                        } else {
                            Color32::from_rgb(90, 98, 117)
                        };
                        ui.painter().text(
                            egui::pos2(rect.right() - 10.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            meta,
                            if tab == ActiveTab::Discover && recs_enabled {
                                font_mono_semi(12.0)
                            } else {
                                font_mono(12.0)
                            },
                            meta_col,
                        );
                    }
                    if resp.clicked() {
                        if tab == ActiveTab::Settings {
                            self.cfg_draft = lock(&self.state).config.clone();
                            self.settings_error = None;
                        } else if tab == ActiveTab::Discover
                            && recs_enabled
                            && matches!(recommendations, RecommendationsState::Idle)
                        {
                            let target = self.discover_region(active_region);
                            self.trigger_recommendations_fetch(target.code, false);
                        }
                        self.active_tab = tab;
                    }
                    ui.add_space(5.0);
                }

                // Bottom-anchored actions, Riot status, and update check
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    // Version & update trigger
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("v{APP_VERSION}")).font(font_mono(12.0)).color(TEXT_MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let upd_busy = matches!(
                                update_state,
                                UpdateState::Checking | UpdateState::Downloading(_) | UpdateState::Installing(_)
                            );
                            let upd_avail = matches!(update_state, UpdateState::Available(_));
                            let upd_lbl = match &update_state {
                                UpdateState::Checking => "Checking...",
                                UpdateState::Downloading(_) => "Downloading...",
                                UpdateState::Installing(_) => "Restarting...",
                                UpdateState::Available(_) => "Update Ready",
                                _ => "Check Updates",
                            };
                            let upd_btn = egui::Button::new(
                                RichText::new(upd_lbl)
                                    .font(font_medium(12.0))
                                    .color(if upd_avail { ACCENT_PINK } else { TEXT_MUTED }),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE);
                            if ui.add_enabled(!upd_busy, upd_btn).clicked() {
                                lock(&self.state).update_state = UpdateState::Checking;
                                let _ = self.bg_tx.send(BgTask::CheckForUpdates { silent: false });
                            }
                        });
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // Quiet secondary action: + Add Streamer
                    let add_btn = egui::Button::new(
                        RichText::new("+ Add Streamer").font(font_semibold(13.0)).color(TEXT_PRIMARY),
                    )
                        .fill(BTN_SECONDARY)
                        .stroke(Stroke::new(1.0, CARD_BORDER))
                        .corner_radius(CornerRadius::same(6))
                        .min_size(Vec2::new(ui.available_width(), 35.0));
                    if ui.add(add_btn).clicked() {
                        self.open_add_modal(None);
                    }

                    ui.add_space(6.0);

                    // Primary focal CTA: Check Last Match
                    let scan_lbl = if is_scanning { "Scanning..." } else { "Check Last Match" };
                    if gradient_button(ui, scan_lbl, Vec2::new(ui.available_width(), 38.0), !is_scanning, 13.5).clicked() {
                        let _ = self.manual_scan_tx.send(ScanTrigger::Manual);
                    }

                    ui.add_space(10.0);

                    // Compact live Riot & Server status block
                    Frame::NONE
                        .fill(CARD_BG)
                        .stroke(Stroke::new(1.0, CARD_BORDER))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(11, 9))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let (dot, glowing, riot_text) = match riot_status {
                                RiotStatus::ValorantActive => (LIVE_GREEN, true, "Valorant Active"),
                                RiotStatus::RiotClientOnly => (ENEMY_AMBER, true, "Riot Client Open"),
                                RiotStatus::Offline => (TEXT_MUTED, false, "Waiting for Valorant"),
                            };
                            ui.horizontal(|ui| {
                                paint_status_dot(ui, dot, glowing);
                                ui.label(RichText::new(riot_text).font(font_semibold(12.5)).color(TEXT_PRIMARY));
                            });
                            ui.add_space(3.0);
                            ui.label(
                                RichText::new(active_region.short_label)
                                    .font(font_mono(12.0))
                                    .color(ALLY_TEAL),
                            );
                            ui.label(
                                RichText::new(short(&status, 34))
                                    .size(11.5)
                                    .color(TEXT_MUTED),
                            )
                            .on_hover_text(status.as_str());
                        });
                });
            });

        // Add / Edit streamer modal
        if self.show_add_modal {
            let is_edit = self.editing_original.is_some();
            let modal_title = if is_edit {
                "Edit Custom Streamer Entry"
            } else {
                "Contribute Streamer to Database"
            };
            let (mut submit_gh, mut submit_local, mut cancel, mut trigger_verify) = (false, false, false, false);
            let mut apply_suggested_name: Option<String> = None;
            let mut changed_input = false;
            let is_verifying = matches!(streamer_verify, CheckState::Checking);

            egui::Window::new(modal_title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(540.0)
                .min_width(540.0)
                .max_width(540.0)
                .frame(
                    Frame::NONE
                        .fill(CARD_BG)
                        .stroke(Stroke::new(1.5, ACCENT_PINK))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::same(22)),
                )
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new("Link a Valorant Riot ID (and optional alt accounts) to a Stream Channel. Community submissions are moderated before merging.")
                            .color(TEXT_MUTED)
                            .size(12.0),
                    );
                    ui.add_space(12.0);

                    ui.label(RichText::new("Primary Riot ID (IGN#TAG)").font(font_semibold(13.0)).color(TEXT_PRIMARY));
                    ui.add_space(3.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.input_riot_id)
                                .hint_text("e.g. TylerFPS#NA1")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(10, 7)),
                        )
                        .changed()
                    {
                        changed_input = true;
                    }
                    ui.add_space(8.0);

                    ui.label(RichText::new("Known Aliases / Alt Riot IDs (Optional, comma-separated)").font(font_semibold(13.0)).color(TEXT_PRIMARY));
                    ui.add_space(3.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.input_aliases)
                                .hint_text("e.g. TylerAlt#NA1, TylerSmurf#USA")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(10, 7)),
                        )
                        .changed()
                    {
                        changed_input = true;
                    }
                    ui.add_space(8.0);

                    ui.label(RichText::new("Platform").font(font_semibold(13.0)).color(TEXT_PRIMARY));
                    ui.add_space(3.0);
                    Frame::NONE
                        .fill(BG_DARK)
                        .stroke(Stroke::new(1.0, CARD_BORDER))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                for plat in ["YouTube", "Twitch", "Kick"] {
                                    if segmented_tab_btn(ui, &mut self.input_platform, plat.to_string(), plat) {
                                        changed_input = true;
                                    }
                                }
                            });
                        });
                    ui.add_space(8.0);

                    ui.label(RichText::new("Primary Valorant Server (prevents cross-region false positives)").font(font_semibold(13.0)).color(TEXT_PRIMARY));
                    ui.add_space(3.0);
                    let selected_reg_label = if self.input_server_region.is_empty() {
                        "Any / Global".to_string()
                    } else {
                        server_region_by_code(&self.input_server_region)
                            .map(|r| r.label.to_string())
                            .unwrap_or_else(|| self.input_server_region.clone())
                    };
                    egui::ComboBox::from_id_salt("modal_server_region_combo")
                        .width(430.0)
                        .selected_text(selected_reg_label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.input_server_region, String::new(), "Any / Global");
                            for reg in SERVER_REGIONS {
                                ui.selectable_value(&mut self.input_server_region, reg.code.to_string(), reg.label);
                            }
                        });
                    ui.add_space(8.0);

                    ui.label(RichText::new("Channel Display Name (or leave blank & click Verify to auto-fill)").font(font_semibold(13.0)).color(TEXT_PRIMARY));
                    ui.add_space(3.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.input_channel_name)
                                .hint_text("e.g. TylerFPS")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(10, 7)),
                        )
                        .changed()
                    {
                        changed_input = true;
                    }
                    ui.add_space(8.0);

                    ui.label(RichText::new("Channel URL").font(font_semibold(13.0)).color(TEXT_PRIMARY));
                    ui.add_space(3.0);
                    let url_hint = match self.input_platform.as_str() {
                        "Twitch" => "e.g. https://www.twitch.tv/tenz",
                        "Kick" => "e.g. https://kick.com/masonclutch",
                        _ => "e.g. https://www.youtube.com/@TylerFPS",
                    };
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.input_channel_url)
                                .hint_text(url_hint)
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(10, 7)),
                        )
                        .changed()
                    {
                        changed_input = true;
                    }

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let verify_label = if is_verifying { "Verifying..." } else { "Verify Link & Name" };
                        let verify_btn = egui::Button::new(RichText::new(verify_label).font(font_semibold(12.0)).color(Color32::WHITE))
                            .fill(BTN_SECONDARY)
                            .stroke(Stroke::new(1.0, ALLY_TEAL))
                            .corner_radius(CornerRadius::same(6))
                            .min_size(Vec2::new(140.0, 30.0));
                        if ui.add_enabled(!is_verifying, verify_btn).clicked() {
                            trigger_verify = true;
                        }

                        let can_open = host_of(self.input_channel_url.trim())
                            .map(|h| host_allowed(&self.input_platform, &h))
                            .unwrap_or(false);
                        let open_btn = egui::Button::new(RichText::new("Open Link").color(TEXT_PRIMARY).size(12.0))
                            .fill(BTN_SECONDARY)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .corner_radius(CornerRadius::same(5))
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
                            paint_cross_icon(ui, ERR_ROSE);
                            ui.label(RichText::new(err).color(ERR_ROSE).size(12.0));
                        });
                    }

                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        submit_gh = gradient_button(ui, "Save & Submit to DB", Vec2::new(168.0, 34.0), true, 12.5).clicked();

                        ui.add_space(6.0);
                        let local_btn = egui::Button::new(RichText::new("Save Local").color(TEXT_PRIMARY))
                            .fill(BTN_SECONDARY)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .corner_radius(CornerRadius::same(6))
                            .min_size(Vec2::new(92.0, 34.0));
                        submit_local = ui.add(local_btn).clicked();

                        ui.add_space(6.0);
                        cancel = ui
                            .add(egui::Button::new("Cancel").min_size(Vec2::new(72.0, 34.0)).corner_radius(CornerRadius::same(6)))
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

        // Main panel (AMOLED Black canvas)
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(BG_DARK).inner_margin(Margin::symmetric(20, 16)))
            .show(ctx, |ui| {
                self.render_update_banner(ui, &update_state);
                match self.active_tab {
                    ActiveTab::History => self.render_history_tab(ui, &history, last_match_summary.as_ref()),
                    ActiveTab::Directory => self.render_directory_tab(ui, &local_dir, &remote_dir, is_syncing_dir),
                    ActiveTab::Discover => {
                        self.render_discover_tab(ui, recs_enabled, &recommendations, active_region)
                    }
                    ActiveTab::Settings => self.render_settings_tab(
                        ui,
                        &yt_key_check,
                        &twitch_key_check,
                        &update_state,
                        &detected_server_code,
                        active_region,
                    ),
                }
            });

        // Subtle 1px curved perimeter border + foreground Pink-to-Pearl top accent hairline
        let fg_painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("frameless_window_border"),
        ));
        let screen = ctx.screen_rect();
        fg_painter.rect_stroke(
            screen.shrink(0.5),
            CornerRadius::same(8),
            Stroke::new(1.0, CARD_BORDER),
            StrokeKind::Inside,
        );
        paint_top_gradient_bar(&fg_painter, screen, 8.0, 2.5);
    }
}

impl StreamerApp {
    fn render_update_banner(&mut self, ui: &mut egui::Ui, update_state: &UpdateState) {
        // Automatically hidden when UpToDate or Idle (no manual dismissal required for up-to-date state).
        match update_state {
            UpdateState::Available(info) => {
                Frame::NONE
                    .fill(CARD_BG)
                    .stroke(Stroke::new(1.2, ACCENT_PINK))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("UPDATE AVAILABLE")
                                            .font(font_bold(10.5))
                                            .color(ACCENT_PINK),
                                    );
                                    ui.label(
                                        RichText::new(format!("{} (Installed: v{APP_VERSION})", info.release_name))
                                            .font(font_semibold(13.0))
                                            .color(ACCENT_PEARL),
                                    );
                                });
                                let mut meta = Vec::new();
                                if !info.tag_name.is_empty() {
                                    meta.push(format!("Version {}", info.tag_name));
                                }
                                if let Some(sha) = &info.remote_sha {
                                    meta.push(format!("Build {sha}"));
                                }
                                if info.size_bytes > 0 {
                                    meta.push(format_bytes(info.size_bytes));
                                }
                                if !info.published_at.is_empty() {
                                    meta.push(info.published_at.clone());
                                }
                                if info.expected_sha256.is_some() {
                                    meta.push("SHA-256 verified".to_string());
                                }
                                if !meta.is_empty() {
                                    ui.label(RichText::new(meta.join("  |  ")).size(11.5).color(TEXT_MUTED));
                                }
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let rel_btn = egui::Button::new(RichText::new("Release Notes").size(11.5).color(TEXT_PRIMARY))
                                    .fill(BTN_SECONDARY)
                                    .stroke(Stroke::new(1.0, CARD_BORDER))
                                    .corner_radius(CornerRadius::same(5));
                                if ui.add(rel_btn).clicked() {
                                    let _ = open::that(&info.html_url);
                                }
                                ui.add_space(6.0);
                                let toggle_lbl = if self.show_update_changelog { "Hide Changelog" } else { "Show Changelog" };
                                let ch_btn = egui::Button::new(RichText::new(toggle_lbl).size(11.5).color(ACCENT_BLUSH))
                                    .fill(BTN_SECONDARY)
                                    .stroke(Stroke::new(1.0, CARD_BORDER))
                                    .corner_radius(CornerRadius::same(5));
                                if ui.add(ch_btn).clicked() {
                                    self.show_update_changelog = !self.show_update_changelog;
                                }
                                ui.add_space(6.0);
                                if gradient_button(ui, "Download & Install", Vec2::new(132.0, 28.0), true, 12.0).clicked() {
                                    let _ = self.bg_tx.send(BgTask::InstallUpdate(info.clone()));
                                }
                            });
                        });

                        if self.show_update_changelog && !info.changelog.is_empty() {
                            ui.add_space(8.0);
                            Frame::NONE
                                .fill(HEADER_BG)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!("RELEASE CHANGELOG ({})", info.tag_name))
                                            .font(font_bold(10.5))
                                            .color(ACCENT_BLUSH),
                                    );
                                    ui.add_space(4.0);
                                    egui::ScrollArea::vertical()
                                        .id_salt("update_changelog_scroll")
                                        .max_height(110.0)
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(&info.changelog).size(11.5).color(TEXT_PRIMARY));
                                        });
                                });
                        }
                    });
                ui.add_space(10.0);
            }
            UpdateState::Downloading(msg) | UpdateState::Installing(msg) => {
                Frame::NONE
                    .fill(CARD_BG)
                    .stroke(Stroke::new(1.0, ACCENT_BLUSH))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(14, 9))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.add_space(6.0);
                            ui.label(RichText::new(msg).font(font_semibold(12.5)).color(ACCENT_PEARL));
                        });
                    });
                ui.add_space(10.0);
            }
            UpdateState::Error(msg) if self.active_tab != ActiveTab::Settings => {
                Frame::NONE
                    .fill(CARD_BG)
                    .stroke(Stroke::new(1.0, ERR_ROSE))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            paint_warn_icon(ui, ERR_ROSE);
                            ui.label(RichText::new(msg).size(12.0).color(ERR_ROSE));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Retry").clicked() {
                                    lock(&self.state).update_state = UpdateState::Checking;
                                    let _ = self.bg_tx.send(BgTask::CheckForUpdates { silent: false });
                                }
                                if ui.small_button("Releases").clicked() {
                                    let _ = open::that(format!(
                                        "https://github.com/{UPDATE_REPO_OWNER}/{UPDATE_REPO_NAME}/releases"
                                    ));
                                }
                            });
                        });
                    });
                ui.add_space(10.0);
            }
            _ => {}
        }
    }

    fn render_history_card(&mut self, ui: &mut egui::Ui, entry: &StreamerEntry) {
        let badge = match entry.platform.as_str() {
            "Twitch" => TWITCH_PURPLE,
            "Kick" => KICK_GREEN,
            _ => ACCENT_PINK,
        };
        let tier = resolve_entry_tier(entry);
        let tier_color = match tier {
            "Confirmed" => LIVE_GREEN,
            "Probable" => ENEMY_AMBER,
            _ => TEXT_MUTED,
        };
        let card_key = format!("{}|{}|{}", entry.match_id, entry.riot_id, entry.platform);
        let is_expanded = self.expanded_cards.contains(&card_key);
        let mut button_rects: Vec<egui::Rect> = Vec::with_capacity(4);
        let mut button_clicked = false;

        let border_col = if is_expanded {
            ACCENT_PINK.linear_multiply(0.55)
        } else if !entry.chat_message.is_empty() {
            ACCENT_PINK.linear_multiply(0.4)
        } else {
            CARD_BORDER
        };

        let frame_resp = Frame::NONE
            .fill(CARD_BG)
            .stroke(Stroke::new(1.0, border_col))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                // Collapsed primary row: Riot ID, Platform, Channel, Confidence tier ONLY + icon Copy + Watch
                ui.horizontal(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.set_max_width((ui.available_width() - 156.0).max(180.0));
                        ui.spacing_mut().item_spacing.x = 7.0;
                        ui.label(
                            RichText::new(&entry.riot_id)
                                .font(font_bold(15.0))
                                .color(ACCENT_PEARL),
                        );
                        ui.label(RichText::new("/").color(CARD_BORDER).size(12.5));
                        ui.label(RichText::new(&entry.platform).font(font_bold(12.5)).color(badge));
                        ui.label(RichText::new(&entry.channel).font(font_medium(13.5)).color(TEXT_PRIMARY));
                        ui.label(RichText::new("/").color(CARD_BORDER).size(12.5));
                        let conf_label = if entry.confidence > 0 {
                            format!("{tier} \u{00B7} {}%", entry.confidence)
                        } else {
                            tier.to_string()
                        };
                        ui.label(RichText::new(conf_label).font(font_mono_semi(12.0)).color(tier_color));
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let watch_resp = gradient_button(ui, "Watch", Vec2::new(82.0, 32.0), true, 13.0)
                            .on_hover_text(entry.url.as_str());
                        button_rects.push(watch_resp.rect);
                        if watch_resp.clicked() {
                            button_clicked = true;
                            let _ = open::that(&entry.url);
                        }

                        let copy_resp = paint_copy_icon_button(ui);
                        button_rects.push(copy_resp.rect);
                        if copy_resp.clicked() {
                            button_clicked = true;
                            if let Ok(mut cb) = Clipboard::new() {
                                let _ = cb.set_text(entry.url.clone());
                            }
                            set_toast(&self.state, "Copied link to clipboard!");
                        }

                        paint_chevron_icon(ui, is_expanded, if is_expanded { ACCENT_BLUSH } else { TEXT_MUTED });
                    });
                });

                // Expanded Details drawer (all secondary match & verification metadata tucked cleanly here)
                if is_expanded {
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(8.0);

                    if !entry.title.is_empty() {
                        ui.label(RichText::new(&entry.title).font(font_medium(13.5)).color(TEXT_PRIMARY));
                        ui.add_space(6.0);
                    }

                    // Match metadata: Team, Agent, Map, Queue, Server, Timestamp
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let mut wrote_any = false;
                        if !entry.team_relation.is_empty() {
                            let tc = if entry.team_relation == "Ally" { ALLY_TEAL } else { ENEMY_AMBER };
                            ui.label(RichText::new(&entry.team_relation).font(font_semibold(12.0)).color(tc));
                            wrote_any = true;
                        }
                        let mut meta: Vec<&str> = Vec::new();
                        if !entry.agent_name.is_empty() {
                            meta.push(&entry.agent_name);
                        }
                        if !entry.map_name.is_empty() {
                            meta.push(&entry.map_name);
                        }
                        if !entry.queue_name.is_empty() {
                            meta.push(&entry.queue_name);
                        }
                        if !entry.server_region.is_empty() {
                            meta.push(&entry.server_region);
                        }
                        if !entry.timestamp.is_empty() {
                            meta.push(&entry.timestamp);
                        }
                        if !meta.is_empty() {
                            if wrote_any {
                                ui.label(RichText::new("/").size(12.0).color(CARD_BORDER));
                            }
                            ui.label(RichText::new(meta.join("  /  ")).font(font_mono(11.5)).color(TEXT_MUTED));
                        }
                    });

                    if !entry.chat_message.is_empty() {
                        ui.add_space(6.0);
                        Frame::NONE
                            .fill(BG_DARK)
                            .stroke(Stroke::new(1.0, ACCENT_PINK.linear_multiply(0.3)))
                            .corner_radius(CornerRadius::same(5))
                            .inner_margin(Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("In-Game Chat: \"{}\"", entry.chat_message))
                                        .color(ACCENT_BLUSH)
                                        .size(12.5),
                                );
                            });
                    }

                    if !entry.server_match_status.is_empty() || !entry.match_reason.is_empty() {
                        ui.add_space(6.0);
                        let mut parts: Vec<&str> = Vec::new();
                        if !entry.server_match_status.is_empty() {
                            parts.push(&entry.server_match_status);
                        }
                        if !entry.match_reason.is_empty() {
                            parts.push(&entry.match_reason);
                        }
                        ui.label(RichText::new(parts.join(" - ")).color(TEXT_MUTED).size(12.5));
                    }

                    if entry.candidates.len() > 1 {
                        ui.add_space(8.0);
                        Frame::NONE
                            .fill(BG_DARK)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("RANKED CANDIDATES ({})", entry.candidates.len()))
                                        .font(font_semibold(11.0))
                                        .color(TEXT_MUTED),
                                );
                                ui.add_space(4.0);
                                for (idx, cand) in entry.candidates.iter().enumerate() {
                                    let c_badge = match cand.platform.as_str() {
                                        "Twitch" => TWITCH_PURPLE,
                                        "Kick" => KICK_GREEN,
                                        _ => ACCENT_PINK,
                                    };
                                    let c_tier = if !cand.confidence_tier.is_empty() {
                                        cand.confidence_tier.as_str()
                                    } else if cand.confidence >= 85 {
                                        "Confirmed"
                                    } else if cand.confidence >= 65 {
                                        "Probable"
                                    } else {
                                        "Unverified"
                                    };
                                    let c_conf_col = match c_tier {
                                        "Confirmed" => LIVE_GREEN,
                                        "Probable" => ENEMY_AMBER,
                                        _ => TEXT_MUTED,
                                    };
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!("#{}", idx + 1))
                                                .font(font_mono_semi(11.5))
                                                .color(if idx == 0 { ACCENT_BLUSH } else { TEXT_MUTED }),
                                        );
                                        ui.label(
                                            RichText::new(format!("{c_tier} ({}%)", cand.confidence))
                                                .font(font_mono_semi(11.5))
                                                .color(c_conf_col),
                                        );
                                        ui.label(RichText::new(&cand.platform).font(font_semibold(12.0)).color(c_badge));
                                        ui.label(RichText::new(&cand.channel).font(font_medium(12.5)).color(TEXT_PRIMARY));
                                        if !cand.region_hint.is_empty() {
                                            ui.label(RichText::new(&cand.region_hint).font(font_mono(11.5)).color(ALLY_TEAL));
                                        }
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let open_cand = egui::Button::new(
                                                RichText::new("Open").size(12.0).color(ACCENT_BLUSH),
                                            )
                                            .fill(BTN_SECONDARY)
                                            .stroke(Stroke::new(1.0, CARD_BORDER))
                                            .corner_radius(CornerRadius::same(4));
                                            let open_resp = ui.add(open_cand).on_hover_text(cand.url.as_str());
                                            button_rects.push(open_resp.rect);
                                            if open_resp.clicked() {
                                                button_clicked = true;
                                                let _ = open::that(&cand.url);
                                            }
                                        });
                                    });
                                    if idx + 1 < entry.candidates.len() {
                                        ui.add_space(3.0);
                                    }
                                }
                            });
                    }
                }
            });

        // Allow clicking anywhere on the card body outside action buttons to toggle Details
        let card_rect = frame_resp.response.rect;
        let pointer_pos = ui.input(|i| i.pointer.interact_pos());
        let pointer_on_button = pointer_pos
            .map(|pos| button_rects.iter().any(|r| r.contains(pos)))
            .unwrap_or(false);
        if ui.rect_contains_pointer(card_rect) && !pointer_on_button {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if !is_expanded {
                ui.painter().rect_stroke(
                    card_rect,
                    CornerRadius::same(8),
                    Stroke::new(1.0, Color32::from_rgb(43, 43, 54)),
                    StrokeKind::Inside,
                );
            }
            if !button_clicked && ui.input(|i| i.pointer.primary_clicked()) {
                if is_expanded {
                    self.expanded_cards.remove(&card_key);
                } else {
                    self.expanded_cards.insert(card_key);
                }
            }
        }
    }

    fn render_history_tab(
        &mut self,
        ui: &mut egui::Ui,
        history: &[StreamerEntry],
        last_match: Option<&LastMatchSummary>,
    ) {
        let active_filter_count = self.active_popover_filter_count();

        // Single clean toolbar row: Search + Collapsible Filters trigger + Clear
        ui.horizontal(|ui| {
            let reserved = if history.is_empty() { 134.0 } else { 214.0 };
            let search_w = (ui.available_width() - reserved).max(180.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Search Riot ID, channel, map, server, or title...")
                    .desired_width(search_w)
                    .margin(Margin::symmetric(12, 8)),
            );
            ui.add_space(10.0);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !history.is_empty() {
                    let clear = egui::Button::new(RichText::new("Clear").font(font_medium(13.0)).color(TEXT_MUTED))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0, CARD_BORDER))
                        .corner_radius(CornerRadius::same(6))
                        .min_size(Vec2::new(64.0, 34.0));
                    if ui.add(clear).clicked() {
                        let mut s = lock(&self.state);
                        s.history.clear();
                        save_json("streamer_history.json", &s.history);
                    }
                    ui.add_space(8.0);
                }

                let filter_label = if active_filter_count > 0 {
                    format!("Filters ({active_filter_count})")
                } else {
                    "Filters".to_string()
                };
                let filter_active = self.show_filters_popover || active_filter_count > 0;
                let filter_btn = egui::Button::new(
                    RichText::new(filter_label)
                        .font(font_medium(13.0))
                        .color(if filter_active { ACCENT_PEARL } else { TEXT_PRIMARY }),
                )
                .fill(if filter_active { SURFACE_HOVER } else { CARD_BG })
                .stroke(Stroke::new(
                    1.0,
                    if filter_active { ACCENT_PINK.linear_multiply(0.65) } else { CARD_BORDER },
                ))
                .corner_radius(CornerRadius::same(6))
                .min_size(Vec2::new(98.0, 34.0));
                if ui.add(filter_btn).clicked() {
                    self.show_filters_popover = !self.show_filters_popover;
                }
            });
        });

        // Collapsible Filters Popover Panel
        if self.show_filters_popover {
            ui.add_space(8.0);
            let mut queue_options: Vec<String> = vec!["All".to_string()];
            for qn in ["Competitive", "Unrated", "Swiftplay", "Deathmatch", "Team Deathmatch", "Spike Rush", "Premier"] {
                queue_options.push(qn.to_string());
            }
            for e in history {
                if !e.queue_name.is_empty() && !queue_options.iter().any(|x| x.eq_ignore_ascii_case(&e.queue_name)) {
                    queue_options.push(e.queue_name.clone());
                }
            }

            let mut server_options: Vec<String> = vec!["All".to_string()];
            for e in history {
                if !e.server_region.is_empty() && !server_options.iter().any(|x| x.eq_ignore_ascii_case(&e.server_region)) {
                    server_options.push(e.server_region.clone());
                }
            }
            if let Some(m) = last_match {
                if !m.server_name.is_empty() && !server_options.iter().any(|x| x.eq_ignore_ascii_case(&m.server_name)) {
                    server_options.push(m.server_name.clone());
                }
            }
            for reg in SERVER_REGIONS {
                if !server_options.iter().any(|x| x.eq_ignore_ascii_case(reg.short_label)) {
                    server_options.push(reg.short_label.to_string());
                }
            }

            let selected_match_type = self.match_type_filter.clone();
            let selected_server = self.server_filter.clone();

            Frame::NONE
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, ACCENT_PINK.linear_multiply(0.35)))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Team").font(font_medium(13.0)).color(TEXT_MUTED));
                        ui.add_space(6.0);
                        Frame::NONE
                            .fill(BG_DARK)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::same(3))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    for (f, label) in [
                                        (TeamFilter::All, "All"),
                                        (TeamFilter::Ally, "Ally"),
                                        (TeamFilter::Enemy, "Enemy"),
                                    ] {
                                        segmented_tab_btn(ui, &mut self.team_filter, f, label);
                                    }
                                });
                            });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let done_btn = egui::Button::new(RichText::new("Done").color(TEXT_PRIMARY).size(13.0))
                                .fill(BTN_SECONDARY)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .corner_radius(CornerRadius::same(5));
                            if ui.add(done_btn).clicked() {
                                self.show_filters_popover = false;
                            }
                            if self.has_active_history_filters() {
                                ui.add_space(6.0);
                                let reset = egui::Button::new(RichText::new("Reset").color(ACCENT_BLUSH).size(13.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, ACCENT_PINK.linear_multiply(0.45)))
                                    .corner_radius(CornerRadius::same(5));
                                if ui.add(reset).clicked() {
                                    self.reset_history_filters();
                                }
                            }
                        });
                    });

                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Match").color(TEXT_MUTED).size(13.0));
                        egui::ComboBox::from_id_salt("history_match_type_filter")
                            .width(136.0)
                            .selected_text(selected_match_type)
                            .show_ui(ui, |ui| {
                                for opt in &queue_options {
                                    ui.selectable_value(&mut self.match_type_filter, opt.clone(), opt.as_str());
                                }
                            });

                        ui.add_space(6.0);
                        ui.label(RichText::new("Server").color(TEXT_MUTED).size(13.0));
                        egui::ComboBox::from_id_salt("history_server_filter")
                            .width(140.0)
                            .selected_text(selected_server)
                            .show_ui(ui, |ui| {
                                for opt in &server_options {
                                    ui.selectable_value(&mut self.server_filter, opt.clone(), opt.as_str());
                                }
                            });

                        ui.add_space(6.0);
                        ui.label(RichText::new("Date").color(TEXT_MUTED).size(13.0));
                        egui::ComboBox::from_id_salt("history_date_filter")
                            .width(114.0)
                            .selected_text(self.date_filter.label())
                            .show_ui(ui, |ui| {
                                for opt in [
                                    DateRangeFilter::AllTime,
                                    DateRangeFilter::Today,
                                    DateRangeFilter::Last7Days,
                                    DateRangeFilter::Last30Days,
                                ] {
                                    ui.selectable_value(&mut self.date_filter, opt, opt.label());
                                }
                            });

                        ui.add_space(6.0);
                        ui.label(RichText::new("Status").color(TEXT_MUTED).size(13.0));
                        egui::ComboBox::from_id_salt("history_status_filter")
                            .width(122.0)
                            .selected_text(self.status_filter.label())
                            .show_ui(ui, |ui| {
                                for opt in [
                                    StatusFilter::All,
                                    StatusFilter::Confirmed,
                                    StatusFilter::Probable,
                                    StatusFilter::Unverified,
                                    StatusFilter::ChatPlug,
                                ] {
                                    ui.selectable_value(&mut self.status_filter, opt, opt.label());
                                }
                            });
                    });
                });
        }

        ui.add_space(12.0);

        let now_local = Local::now();
        let filtered: Vec<&StreamerEntry> = history
            .iter()
            .filter(|e| {
                entry_matches_history_filters(
                    e,
                    &self.search_query,
                    self.team_filter,
                    &self.match_type_filter,
                    &self.server_filter,
                    self.date_filter,
                    self.status_filter,
                    now_local,
                )
            })
            .collect();

        // Separate Latest Match entries from Previous History entries without duplicating or altering order
        let latest_match_id = last_match.map(|m| m.match_id.as_str()).filter(|id| !id.is_empty());
        let (latest_entries, previous_entries): (Vec<&StreamerEntry>, Vec<&StreamerEntry>) = match latest_match_id {
            Some(mid) => filtered.into_iter().partition(|e| e.match_id == mid),
            None => (Vec::new(), filtered),
        };

        let filters_active = self.has_active_history_filters();

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Subsection 1: Latest Match (matches preview header weight, colors, and monospace stats)
            if let Some(m) = last_match {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    paint_status_dot(ui, ACCENT_PINK, false);
                    ui.label(RichText::new("LATEST MATCH").font(font_bold(11.5)).color(ACCENT_BLUSH));
                    ui.label(RichText::new("/").size(12.5).color(CARD_BORDER));
                    ui.label(RichText::new(&m.queue_name).font(font_semibold(13.0)).color(ACCENT_PEARL));
                    ui.label(RichText::new(&m.map_name).font(font_medium(13.0)).color(TEXT_PRIMARY));
                    if !m.server_name.is_empty() {
                        ui.label(RichText::new("/").size(12.0).color(CARD_BORDER));
                        ui.label(RichText::new(&m.server_name).font(font_semibold(12.5)).color(ALLY_TEAL));
                    }
                    ui.label(RichText::new("/").size(12.0).color(CARD_BORDER));
                    let mut count_parts = vec![format!("{} scanned", m.players_checked)];
                    if m.party_skipped > 0 {
                        count_parts.push(format!("{} party skipped", m.party_skipped));
                    }
                    if m.hidden_skipped > 0 {
                        count_parts.push(format!("{} hidden", m.hidden_skipped));
                    }
                    let summary_counts = count_parts.join(", ");
                    let count_lbl = ui.label(RichText::new(summary_counts).font(font_mono(12.0)).color(TEXT_MUTED));
                    if m.hidden_skipped > 0 {
                        count_lbl.on_hover_text(
                            "Riot returned blank or invisible names (e.g. Streamer Mode / privacy) for these player(s).",
                        );
                    }
                    ui.label(RichText::new("/").size(12.0).color(CARD_BORDER));
                    if m.streamers_found > 0 {
                        ui.label(
                            RichText::new(format!("{} detected", m.streamers_found))
                                .font(font_mono_semi(12.0))
                                .color(LIVE_GREEN),
                        );
                    } else {
                        ui.label(RichText::new("0 detected").font(font_mono(12.0)).color(TEXT_MUTED));
                    }
                });

                ui.add_space(9.0);
                if !latest_entries.is_empty() {
                    for entry in &latest_entries {
                        self.render_history_card(ui, entry);
                        ui.add_space(9.0);
                    }
                } else {
                    let msg = if filters_active && m.streamers_found > 0 {
                        "Latest match detections hidden by active filters."
                    } else {
                        "No live streamers detected in your latest match."
                    };
                    ui.label(RichText::new(msg).color(TEXT_MUTED).size(13.0));
                    ui.add_space(6.0);
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(10.0);
            }

            // Subsection 2: Previous History
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("PREVIOUS HISTORY ({})", previous_entries.len()))
                        .font(font_semibold(11.5))
                        .color(TEXT_MUTED),
                );
            });
            ui.add_space(9.0);

            if previous_entries.is_empty() && latest_entries.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    if history.is_empty() {
                        ui.label(RichText::new("No Streamers Recorded Yet").font(font_bold(17.0)).color(TEXT_PRIMARY));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Play a match or click '+ Add Streamer' in the sidebar to link a Riot ID.")
                                .color(TEXT_MUTED)
                                .size(13.5),
                        );
                    } else {
                        ui.label(RichText::new("No Records Match Your Filters").font(font_bold(17.0)).color(TEXT_PRIMARY));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Your saved history is intact. Adjust or reset filters to view older records.")
                                .color(TEXT_MUTED)
                                .size(14.0),
                        );
                        ui.add_space(10.0);
                        if gradient_button(ui, "Reset Filters", Vec2::new(132.0, 34.0), true, 13.5).clicked() {
                            self.reset_history_filters();
                        }
                    }
                });
            } else if previous_entries.is_empty() {
                ui.label(RichText::new("No earlier history records match your current filters.").color(TEXT_MUTED).size(13.5));
            } else {
                for entry in previous_entries {
                    self.render_history_card(ui, entry);
                    ui.add_space(9.0);
                }
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
            Frame::NONE
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        segmented_tab_btn(
                            ui,
                            &mut self.dir_source_tab,
                            DirectorySourceTab::Custom,
                            &format!("My Custom Streamers ({})", local_dir.len()),
                        );
                        segmented_tab_btn(
                            ui,
                            &mut self.dir_source_tab,
                            DirectorySourceTab::Community,
                            &format!("Community Database ({})", remote_dir.len()),
                        );
                    });
                });
            ui.add_space(10.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.dir_search)
                    .hint_text("Filter Riot ID, alias, channel, or server...")
                    .desired_width(230.0)
                    .margin(Margin::symmetric(10, 7)),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let sync_label = if is_syncing { "Syncing..." } else { "Sync Database" };
                let sync_btn = egui::Button::new(RichText::new(sync_label).font(font_medium(13.0)).color(TEXT_PRIMARY))
                    .fill(BTN_SECONDARY)
                    .stroke(Stroke::new(1.0, CARD_BORDER))
                    .corner_radius(CornerRadius::same(6))
                    .min_size(Vec2::new(124.0, 34.0));
                if ui.add_enabled(!is_syncing, sync_btn).clicked() {
                    let _ = self.manual_scan_tx.send(ScanTrigger::RefreshDirectory);
                }
            });
        });

        ui.add_space(6.0);
        ui.label(
            RichText::new("Community contributions are moderated before being merged into the central streamers.json database.")
                .color(TEXT_MUTED)
                .size(13.0),
        );
        ui.add_space(10.0);

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
                    || d.display_name.to_lowercase().contains(&q)
                    || d.channel_name.to_lowercase().contains(&q)
                    || d.platform.to_lowercase().contains(&q)
                    || d.server_region.to_lowercase().contains(&q)
                    || d.aliases.iter().any(|a| a.to_lowercase().contains(&q))
            })
            .collect();

        if filtered.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                let msg = match self.dir_source_tab {
                    DirectorySourceTab::Custom => "No custom streamers saved yet. Click '+ Add Streamer' to add one.",
                    DirectorySourceTab::Community => "Community database cache is empty. Click 'Sync Database' to fetch the latest streamers.json.",
                };
                ui.label(RichText::new(msg).color(TEXT_MUTED).size(14.5));
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
                    _ => ACCENT_PINK,
                };
                Frame::NONE
                    .fill(CARD_BG)
                    .stroke(Stroke::new(1.0, CARD_BORDER))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 7.0;
                                    ui.label(RichText::new(&d.riot_id).font(font_bold(15.0)).color(ACCENT_PEARL));
                                    ui.label(RichText::new("/").color(CARD_BORDER).size(12.5));
                                    ui.label(RichText::new(&d.platform).font(font_bold(12.5)).color(badge));
                                    ui.label(RichText::new(&d.channel_name).font(font_medium(13.5)).color(TEXT_PRIMARY));
                                    if d.verified {
                                        ui.label(RichText::new("/").color(CARD_BORDER).size(12.5));
                                        ui.label(RichText::new("Verified").font(font_mono_semi(12.0)).color(LIVE_GREEN));
                                    }
                                    if !d.server_region.is_empty() {
                                        let reg_label = server_region_by_code(&d.server_region)
                                            .map(|r| r.short_label)
                                            .unwrap_or(&d.server_region);
                                        ui.label(RichText::new("/").color(CARD_BORDER).size(12.5));
                                        ui.label(RichText::new(reg_label).font(font_mono(12.0)).color(ALLY_TEAL));
                                    }
                                });
                                if !d.aliases.is_empty() {
                                    ui.label(
                                        RichText::new(format!("Aliases: {}", d.aliases.join(", ")))
                                            .color(ACCENT_BLUSH)
                                            .size(12.5),
                                    );
                                }
                                ui.label(RichText::new(&d.channel_url).color(TEXT_MUTED).size(12.5));
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if self.dir_source_tab == DirectorySourceTab::Custom {
                                    let del = egui::Button::new(RichText::new("Delete").color(ERR_ROSE).size(13.0))
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::new(1.0, CARD_BORDER))
                                        .corner_radius(CornerRadius::same(5));
                                    if ui.add(del).clicked() {
                                        to_delete = Some((d.riot_id.clone(), d.platform.clone()));
                                    }
                                    let edit = egui::Button::new(RichText::new("Edit").color(TEXT_PRIMARY).size(13.0))
                                        .fill(BTN_SECONDARY)
                                        .stroke(Stroke::new(1.0, CARD_BORDER))
                                        .corner_radius(CornerRadius::same(5));
                                    if ui.add(edit).clicked() {
                                        to_edit = Some(d.clone());
                                    }
                                }
                                let open_btn = egui::Button::new(RichText::new("Open").color(TEXT_PRIMARY).size(13.0))
                                    .fill(BTN_SECONDARY)
                                    .stroke(Stroke::new(1.0, CARD_BORDER))
                                    .corner_radius(CornerRadius::same(5));
                                if ui.add(open_btn).clicked() {
                                    let _ = open::that(&d.channel_url);
                                }
                            });
                        });
                    });
                ui.add_space(9.0);
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

    fn render_discover_tab(
        &mut self,
        ui: &mut egui::Ui,
        recs_enabled: bool,
        recommendations: &RecommendationsState,
        active_region: &'static ServerRegionInfo,
    ) {
        let target_region = self.discover_region(active_region);

        if !recs_enabled {
            ui.add_space(36.0);
            Frame::NONE
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(24))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Live Streamer Recommendations are OFF")
                                    .font(font_bold(17.5))
                                    .color(ACCENT_PEARL),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!(
                                    "Disabled by default to save internet bandwidth and external API quota (zero background requests while OFF). Enable to browse optional Twitch and YouTube Valorant streams for {} (or any Valorant server you choose).",
                                    target_region.label
                                ))
                                .color(TEXT_MUTED)
                                .size(14.0),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("Note: Recommended streams are regional discovery suggestions and are never treated as confirmed player detections from your matches.")
                                    .color(ENEMY_AMBER)
                                    .size(13.0),
                            );
                        });
                    });
                    ui.add_space(16.0);
                    let mut enable_now = false;
                    if toggle_row(
                        ui,
                        &mut enable_now,
                        "Enable Live Streamer Recommendations (Twitch & YouTube)",
                        "Uses additional internet bandwidth when opened or refreshed (5-minute local cache).",
                    ) && enable_now
                    {
                        self.cfg_draft.enable_recommendations = true;
                        {
                            let mut s = lock(&self.state);
                            s.config.enable_recommendations = true;
                            save_json("config.json", &s.config);
                        }
                        self.trigger_recommendations_fetch(target_region.code, false);
                    }
                });
            return;
        }

        let prev_server = self.discover_server_override.clone();
        let discover_selected_label = if self.discover_server_override.eq_ignore_ascii_case("auto") {
            format!("Active Server ({})", active_region.short_label)
        } else {
            target_region.label.to_string()
        };

        ui.horizontal(|ui| {
            Frame::NONE
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for (plat, lbl) in [
                            (DiscoverPlatformFilter::All, "All Platforms"),
                            (DiscoverPlatformFilter::Twitch, "Twitch"),
                            (DiscoverPlatformFilter::YouTube, "YouTube"),
                        ] {
                            segmented_tab_btn(ui, &mut self.discover_platform, plat, lbl);
                        }
                    });
                });
            ui.add_space(10.0);
            ui.label(RichText::new("Server").font(font_medium(13.0)).color(TEXT_MUTED));
            egui::ComboBox::from_id_salt("discover_server_region_combo")
                .width(210.0)
                .selected_text(RichText::new(discover_selected_label).font(font_medium(13.0)).color(ALLY_TEAL))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.discover_server_override,
                        "auto".to_string(),
                        format!("Active Server ({})", active_region.short_label),
                    );
                    for reg in SERVER_REGIONS {
                        ui.selectable_value(
                            &mut self.discover_server_override,
                            reg.code.to_string(),
                            reg.label,
                        );
                    }
                });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut keep_on = true;
                if toggle_slider(ui, &mut keep_on).changed() && !keep_on {
                    self.cfg_draft.enable_recommendations = false;
                    {
                        let mut s = lock(&self.state);
                        s.config.enable_recommendations = false;
                        s.recommendations = RecommendationsState::Idle;
                        s.recommendations_cache = None;
                        save_json("config.json", &s.config);
                    }
                    self.active_tab = ActiveTab::History;
                    return;
                }
                ui.label(RichText::new("ON").font(font_semibold(12.0)).color(ACCENT_BLUSH));
                ui.add_space(8.0);

                let busy = matches!(recommendations, RecommendationsState::Loading);
                let ref_lbl = if busy { "Refreshing..." } else { "Refresh Live" };
                if gradient_button(ui, ref_lbl, Vec2::new(124.0, 32.0), !busy, 13.5).clicked() {
                    let chosen = self.discover_region(active_region);
                    self.trigger_recommendations_fetch(chosen.code, true);
                }
            });
        });

        let target_region = if self.discover_server_override != prev_server {
            let next_target = self.discover_region(active_region);
            self.trigger_recommendations_fetch(next_target.code, false);
            next_target
        } else {
            target_region
        };

        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Discovery Suggestions for {} (Not Matched to Your Lobby)  |  Changing server here does not affect match detection.",
                target_region.label
            ))
            .color(TEXT_MUTED)
            .size(12.5),
        );
        ui.add_space(10.0);

        match recommendations {
            RecommendationsState::Idle => {
                ui.add_space(50.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Click 'Refresh Live' to load regional Twitch & YouTube Valorant streams.").color(TEXT_MUTED).size(14.5));
                });
            }
            RecommendationsState::Loading => {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("Fetching live Twitch & YouTube Valorant streamers for {}...", target_region.short_label))
                            .color(TEXT_MUTED)
                            .size(14.5),
                    );
                });
            }
            RecommendationsState::Error(err) => {
                Frame::NONE
                    .fill(ERR_ROSE.linear_multiply(0.12))
                    .stroke(Stroke::new(1.0, ERR_ROSE.linear_multiply(0.6)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            paint_cross_icon(ui, ERR_ROSE);
                            ui.label(RichText::new(err).color(ERR_ROSE).size(13.5));
                        });
                    });
            }
            RecommendationsState::Loaded(items, fetched_at) => {
                let filtered: Vec<&RecommendedStreamer> = items
                    .iter()
                    .filter(|r| match self.discover_platform {
                        DiscoverPlatformFilter::All => true,
                        DiscoverPlatformFilter::Twitch => r.platform.eq_ignore_ascii_case("Twitch"),
                        DiscoverPlatformFilter::YouTube => r.platform.eq_ignore_ascii_case("YouTube"),
                    })
                    .collect();

                ui.label(
                    RichText::new(format!(
                        "Showing {} recommended streams for {}  |  Updated {}",
                        filtered.len(),
                        target_region.short_label,
                        fetched_at
                    ))
                    .color(TEXT_MUTED)
                    .size(13.0),
                );
                ui.add_space(8.0);

                if filtered.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No live Valorant streams found for this server/platform filter. Try another server or Refresh.")
                                .color(TEXT_MUTED)
                                .size(14.5),
                        );
                    });
                    return;
                }

                let mut prefill_rec: Option<RecommendedStreamer> = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for rec in filtered {
                        let badge = if rec.platform.eq_ignore_ascii_case("Twitch") {
                            TWITCH_PURPLE
                        } else {
                            ACCENT_PINK
                        };
                        Frame::NONE
                            .fill(CARD_BG)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(16, 14))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_max_width((ui.available_width() - 224.0).max(180.0));
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing.x = 7.0;
                                            paint_status_dot(ui, LIVE_GREEN, true);
                                            ui.label(RichText::new(&rec.channel).font(font_bold(15.0)).color(ACCENT_PEARL));
                                            ui.label(RichText::new("/").size(12.5).color(CARD_BORDER));
                                            ui.label(RichText::new(&rec.platform).font(font_bold(12.5)).color(badge));
                                            if let Some(v) = rec.viewers {
                                                ui.label(RichText::new("/").size(12.5).color(CARD_BORDER));
                                                ui.label(RichText::new(format_viewers(v)).size(13.0).color(LIVE_GREEN));
                                            }
                                            ui.label(RichText::new("/").size(12.5).color(CARD_BORDER));
                                            ui.label(RichText::new(&rec.region_label).size(12.5).color(TEXT_MUTED));
                                        });
                                        ui.add_space(4.0);
                                        ui.label(RichText::new(&rec.title).color(TEXT_PRIMARY).size(14.0));
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if gradient_button(ui, "Watch", Vec2::new(88.0, 32.0), true, 13.5)
                                            .on_hover_text(rec.url.as_str())
                                            .clicked()
                                        {
                                            let _ = open::that(&rec.url);
                                        }
                                        let save_btn = egui::Button::new(RichText::new("+ Directory").color(TEXT_PRIMARY).size(13.0))
                                            .fill(BTN_SECONDARY)
                                            .stroke(Stroke::new(1.0, CARD_BORDER))
                                            .corner_radius(CornerRadius::same(6))
                                            .min_size(Vec2::new(98.0, 32.0));
                                        if ui.add(save_btn).clicked() {
                                            prefill_rec = Some(rec.clone());
                                        }
                                    });
                                });
                            });
                        ui.add_space(9.0);
                    }
                });

                if let Some(rec) = prefill_rec {
                    self.open_add_modal_from_rec(&rec, target_region.code);
                }
            }
        }
    }

    fn render_settings_tab(
        &mut self,
        ui: &mut egui::Ui,
        yt_check: &CheckState,
        twitch_check: &CheckState,
        update_state: &UpdateState,
        detected_server_code: &str,
        active_region: &ServerRegionInfo,
    ) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Card 1: Valorant Servers & Detection (GitHub owner/repo intentionally hidden from end users)
            Frame::NONE
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Valorant Servers & Region Detection")
                            .font(font_bold(15.5))
                            .color(ACCENT_PEARL),
                    );
                    ui.label(
                        RichText::new(
                            "Matches streamers to the exact Valorant server you play on to prevent cross-region false positives (e.g. US streamers showing up in Mumbai lobbies).",
                        )
                        .color(TEXT_MUTED)
                        .size(13.0),
                    );
                    ui.add_space(10.0);

                    let selected_label = if self.cfg_draft.server_override.eq_ignore_ascii_case("auto") {
                        format!("Auto-detect from Riot log (Current: {})", active_region.label)
                    } else {
                        server_region_by_code(&self.cfg_draft.server_override)
                            .map(|r| r.label.to_string())
                            .unwrap_or_else(|| self.cfg_draft.server_override.clone())
                    };

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Active Server").font(font_semibold(13.5)).color(TEXT_PRIMARY));
                        ui.add_space(10.0);
                        egui::ComboBox::from_id_salt("settings_server_region_combo")
                            .width(380.0)
                            .selected_text(selected_label)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.cfg_draft.server_override,
                                    "auto".to_string(),
                                    "Auto-detect (ShooterGame.log server pod)",
                                );
                                for reg in SERVER_REGIONS {
                                    ui.selectable_value(
                                        &mut self.cfg_draft.server_override,
                                        reg.code.to_string(),
                                        reg.label,
                                    );
                                }
                            });
                    });

                    ui.add_space(8.0);
                    let detected_info = server_region_by_code(detected_server_code)
                        .map(|r| r.label)
                        .unwrap_or("Waiting for game server pod in ShooterGame.log");
                    ui.label(
                        RichText::new(format!("Detected server pod: {detected_info}"))
                            .font(font_mono(12.0))
                            .color(ALLY_TEAL),
                    );
                });

            ui.add_space(12.0);

            // Card 2: Features & Windows Integration (left-to-right ON/OFF toggle sliders)
            Frame::NONE
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Features & Windows Integration")
                            .font(font_bold(15.5))
                            .color(ACCENT_PEARL),
                    );
                    ui.add_space(10.0);

                    toggle_row(
                        ui,
                        &mut self.cfg_draft.skip_party_members,
                        "Skip Party Members During Scans (Save API Calls)",
                        "ON by default. Uses match-details partyId (zero extra Riot calls) to skip scanning your premade teammates on YouTube, Twitch, and Kick.",
                    );
                    ui.add_space(8.0);
                    if toggle_row(
                        ui,
                        &mut self.cfg_draft.enable_recommendations,
                        "Live Streamer Recommendations (Twitch & YouTube)",
                        "OFF by default to save internet bandwidth. Turn ON to show the Discover tab and browse optional regional Valorant streams.",
                    ) {
                        let enabled = self.cfg_draft.enable_recommendations;
                        {
                            let mut s = lock(&self.state);
                            s.config.enable_recommendations = enabled;
                            if !enabled {
                                s.recommendations = RecommendationsState::Idle;
                                s.recommendations_cache = None;
                            }
                            save_json("config.json", &s.config);
                        }
                        if !enabled && self.active_tab == ActiveTab::Discover {
                            self.active_tab = ActiveTab::History;
                        }
                        ui.ctx().request_repaint();
                    }
                    ui.add_space(8.0);
                    toggle_row(
                        ui,
                        &mut self.cfg_draft.capture_in_game_chat,
                        "Post-Match In-Game Chat Stream Plugs",
                        "Reads local post-match text chat for YouTube, Twitch, or Kick links posted by players.",
                    );
                    ui.add_space(8.0);
                    toggle_row(
                        ui,
                        &mut self.cfg_draft.start_with_windows,
                        "Start Silently with Windows",
                        "Launches minimized in the system tray at Windows logon (HKCU Run).",
                    );
                });

            ui.add_space(12.0);

            // Card 3: Optional Platform API Keys
            Frame::NONE
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Optional Platform API Keys").font(font_bold(15.5)).color(ACCENT_PEARL));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            toggle_slider(ui, &mut self.show_api_secrets);
                            ui.add_space(6.0);
                            ui.label(RichText::new("Show keys").color(TEXT_MUTED).size(12.5));
                        });
                    });
                    ui.label(RichText::new("Leave blank to run 100% keyless on YouTube, Twitch, and Kick, or add free keys for faster official API checks.").color(TEXT_MUTED).size(13.0));
                    ui.add_space(10.0);

                    // YouTube Data API v3 Key row
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("YouTube Data API v3 Key").font(font_semibold(13.5)).color(TEXT_PRIMARY));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let yt_busy = matches!(yt_check, CheckState::Checking);
                            let test_yt = egui::Button::new(RichText::new(if yt_busy { "Testing..." } else { "Test Key" }).font(font_medium(12.5)).color(TEXT_PRIMARY))
                                .fill(BTN_SECONDARY)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .corner_radius(CornerRadius::same(6))
                                .min_size(Vec2::new(78.0, 28.0));
                            if ui.add_enabled(!yt_busy, test_yt).clicked() {
                                lock(&self.state).yt_key_check = CheckState::Checking;
                                let _ = self.bg_tx.send(BgTask::TestYouTubeKey(self.cfg_draft.youtube_api_key.clone()));
                            }
                            let get_yt = egui::Button::new(RichText::new("Get YouTube Key").font(font_medium(12.5)).color(ACCENT_BLUSH))
                                .fill(BTN_SECONDARY)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .corner_radius(CornerRadius::same(6))
                                .min_size(Vec2::new(124.0, 28.0));
                            if ui
                                .add(get_yt)
                                .on_hover_text("Opens Google Cloud Console -> Credentials")
                                .clicked()
                            {
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
                                .margin(Margin::symmetric(10, 7)),
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
                        ui.label(RichText::new("Twitch Helix API (Client ID & Secret)").font(font_semibold(13.5)).color(TEXT_PRIMARY));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let tw_busy = matches!(twitch_check, CheckState::Checking);
                            let test_tw = egui::Button::new(RichText::new(if tw_busy { "Testing..." } else { "Test Twitch" }).font(font_medium(12.5)).color(TEXT_PRIMARY))
                                .fill(BTN_SECONDARY)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .corner_radius(CornerRadius::same(6))
                                .min_size(Vec2::new(88.0, 28.0));
                            if ui.add_enabled(!tw_busy, test_tw).clicked() {
                                lock(&self.state).twitch_key_check = CheckState::Checking;
                                let _ = self.bg_tx.send(BgTask::TestTwitchKeys {
                                    client_id: self.cfg_draft.twitch_client_id.clone(),
                                    client_secret: self.cfg_draft.twitch_client_secret.clone(),
                                });
                            }
                            let get_tw = egui::Button::new(RichText::new("Get Twitch Keys").font(font_medium(12.5)).color(ACCENT_BLUSH))
                                .fill(BTN_SECONDARY)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .corner_radius(CornerRadius::same(6))
                                .min_size(Vec2::new(124.0, 28.0));
                            if ui.add(get_tw).on_hover_text("Opens Twitch Developer Console -> Applications").clicked() {
                                let _ = open::that("https://dev.twitch.tv/console/apps");
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new("Twitch Client ID").font(font_medium(12.5)).color(TEXT_MUTED));
                    ui.add_space(4.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cfg_draft.twitch_client_id)
                                .password(!self.show_api_secrets)
                                .hint_text("Optional Helix Client ID")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(10, 7)),
                        )
                        .changed()
                    {
                        lock(&self.state).twitch_key_check = CheckState::Idle;
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("Twitch Client Secret").font(font_medium(12.5)).color(TEXT_MUTED));
                    ui.add_space(4.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cfg_draft.twitch_client_secret)
                                .password(!self.show_api_secrets)
                                .hint_text("Optional Helix Client Secret")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(10, 7)),
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
                            .font(font_semibold(13.0))
                            .color(ACCENT_BLUSH),
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        Frame::NONE
                            .fill(BG_DARK)
                            .stroke(Stroke::new(1.0, CARD_BORDER))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::same(14))
                            .show(ui, |ui| {
                                ui.label(RichText::new("1. YouTube Data API v3 (Free - 10,000 units/day, no credit card needed)").font(font_semibold(13.5)).color(ACCENT_PINK));
                                ui.add_space(3.0);
                                ui.label(RichText::new("1) Click 'Get YouTube Key' above to open Google Cloud Console and sign in.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("2) Click the project picker (top-left) -> New Project -> name it 'Valorant-Radar' -> Create, then select it.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("3) Open left menu -> APIs & Services -> Library -> search 'YouTube Data API v3' -> click Enable.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("4) Open APIs & Services -> Credentials -> + CREATE CREDENTIALS -> API key.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("5) (Recommended) Click Edit API key -> API restrictions -> Restrict key -> YouTube Data API v3 -> Save.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("6) Copy the AIza... key, paste it above, click 'Test Key', then click 'Save Settings' below.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("Tip: If Test Key says the API is not enabled, wait 1-2 minutes after pressing Enable and retry.").color(TEXT_MUTED).size(12.5));

                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.label(RichText::new("2. Twitch Helix API (Free - requires 2FA on your Twitch account)").font(font_semibold(13.5)).color(TWITCH_PURPLE));
                                ui.add_space(3.0);
                                ui.label(RichText::new("1) Enable Two-Factor Authentication (2FA) at twitch.tv/settings/security (required by Twitch).").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("2) Click 'Get Twitch Keys' above to open Twitch Developer Console -> + Register Your Application.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("3) Fill in: Name = 'Valorant Streamer Radar' (add numbers if taken), OAuth Redirect URL = http://localhost (click Add).").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("4) Set Category = 'Application Integration' and Client Type = 'Confidential' (important!), then click Create.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("5) Click Manage -> copy Client ID into the box above.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("6) Click 'New Secret' -> confirm -> copy the secret immediately (shown once!) into Client Secret above.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("7) Click 'Test Twitch' to verify live OAuth works, then click 'Save Settings' below.").color(TEXT_PRIMARY).size(13.0));
                                ui.label(RichText::new("Common mistake: Picking 'Public' instead of 'Confidential' hides the New Secret button.").color(TEXT_MUTED).size(12.5));

                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(6.0);
                                ui.label(RichText::new("Kick does not need an API key. Leaving all keys blank still works via keyless lookups.").color(TEXT_MUTED).size(12.5));
                                ui.label(RichText::new("Privacy: API keys are stored strictly locally in %APPDATA%\\ValorantStreamerRadar\\config.json and are never uploaded.").color(ALLY_TEAL).size(12.5));
                            });
                    });
                });

            ui.add_space(12.0);

            // Card 4: App Updates & Version
            Frame::NONE
                .fill(CARD_BG)
                .stroke(Stroke::new(1.0, CARD_BORDER))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("App Updates & Version")
                                    .font(font_bold(15.5))
                                    .color(ACCENT_PEARL),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Installed: v{APP_VERSION} (build {APP_BUILD_SHA})"
                                ))
                                .font(font_mono(12.5))
                                .color(TEXT_MUTED),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let open_rel = egui::Button::new(RichText::new("Open Releases").color(TEXT_PRIMARY).size(13.5))
                                .fill(BTN_SECONDARY)
                                .stroke(Stroke::new(1.0, CARD_BORDER))
                                .corner_radius(CornerRadius::same(6))
                                .min_size(Vec2::new(118.0, 34.0));
                            if ui.add(open_rel).clicked() {
                                let _ = open::that(format!(
                                    "https://github.com/{UPDATE_REPO_OWNER}/{UPDATE_REPO_NAME}/releases"
                                ));
                            }
                            ui.add_space(8.0);
                            let busy = matches!(
                                update_state,
                                UpdateState::Checking | UpdateState::Downloading(_) | UpdateState::Installing(_)
                            );
                            let check_lbl = if matches!(update_state, UpdateState::Checking) {
                                "Checking..."
                            } else {
                                "Check for Updates"
                            };
                            if gradient_button(ui, check_lbl, Vec2::new(152.0, 34.0), !busy, 13.5).clicked() {
                                lock(&self.state).update_state = UpdateState::Checking;
                                let _ = self.bg_tx.send(BgTask::CheckForUpdates { silent: false });
                            }
                        });
                    });

                    ui.add_space(12.0);
                    toggle_row(
                        ui,
                        &mut self.cfg_draft.check_updates_on_boot,
                        "Check for Updates on Boot",
                        "Silently checks GitHub Releases on launch and only displays the update notice when you open the GUI window.",
                    );

                    match update_state {
                        UpdateState::UpToDate(msg) => {
                            ui.add_space(8.0);
                            ui.label(RichText::new(msg).color(LIVE_GREEN).size(12.0));
                        }
                        UpdateState::Available(info) => {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("New release ready: {} ({})", info.release_name, info.tag_name))
                                        .font(font_semibold(12.5))
                                        .color(ACCENT_BLUSH),
                                );
                                ui.add_space(8.0);
                                if gradient_button(ui, "Download & Install", Vec2::new(132.0, 26.0), true, 11.5).clicked() {
                                    let _ = self.bg_tx.send(BgTask::InstallUpdate(info.clone()));
                                }
                            });
                            if !info.changelog.is_empty() {
                                ui.add_space(6.0);
                                ui.label(RichText::new(&info.changelog).color(TEXT_MUTED).size(11.5));
                            }
                        }
                        UpdateState::Downloading(msg) | UpdateState::Installing(msg) => {
                            ui.add_space(8.0);
                            ui.label(RichText::new(msg).color(ACCENT_PEARL).size(12.0));
                        }
                        UpdateState::Error(msg) => {
                            ui.add_space(8.0);
                            ui.label(RichText::new(msg).color(ERR_ROSE).size(12.0));
                        }
                        _ => {}
                    }
                });

            if let Some(err) = &self.settings_error {
                ui.add_space(8.0);
                ui.label(RichText::new(err).color(ERR_ROSE).size(12.5));
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if gradient_button(ui, "Save Settings", Vec2::new(136.0, 36.0), true, 13.0).clicked() {
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

// ============================================================================
// PURE WIN32 GDI SOFTWARE WINDOW HOST (ZERO DXGI / OPENGL / VULKAN / G-SYNC)
// ============================================================================

struct SoftwareWindowHost {
    softbuffer_context: softbuffer::Context<winit::event_loop::OwnedDisplayHandle>,
    viewport_builder: egui::ViewportBuilder,
    egui_ctx: egui::Context,
    app: StreamerApp,
    renderer: EguiSoftwareRender,
    window: Option<Rc<winit::window::Window>>,
    surface: Option<softbuffer::Surface<winit::event_loop::OwnedDisplayHandle, Rc<winit::window::Window>>>,
    egui_winit: Option<egui_winit::State>,
    viewport_info: egui::ViewportInfo,
    surface_size: (u32, u32),
    next_repaint: Option<Instant>,
}

impl SoftwareWindowHost {
    fn new(
        softbuffer_context: softbuffer::Context<winit::event_loop::OwnedDisplayHandle>,
        viewport_builder: egui::ViewportBuilder,
        egui_ctx: egui::Context,
        app: StreamerApp,
    ) -> Self {
        Self {
            softbuffer_context,
            viewport_builder,
            egui_ctx,
            app,
            renderer: EguiSoftwareRender::new(ColorFieldOrder::Bgra),
            window: None,
            surface: None,
            egui_winit: None,
            viewport_info: egui::ViewportInfo::default(),
            surface_size: (0, 0),
            next_repaint: None,
        }
    }

    fn render_frame(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if QUIT.load(Ordering::Relaxed) {
            if let Some(window) = &self.window {
                window.set_visible(false);
            }
            event_loop.exit();
            return;
        }
        if !WINDOW_VISIBLE.load(Ordering::Relaxed) {
            return;
        }
        let (Some(window), Some(surface), Some(egui_winit)) = (
            self.window.as_ref(),
            self.surface.as_mut(),
            self.egui_winit.as_mut(),
        ) else {
            return;
        };

        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let (Some(w_nz), Some(h_nz)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if self.surface_size != (size.width, size.height) {
            if let Err(e) = surface.resize(w_nz, h_nz) {
                log(format!("softbuffer resize failed: {e}"));
                return;
            }
            self.surface_size = (size.width, size.height);
        }

        let raw_input = egui_winit.take_egui_input(window);
        let mut full_output = self.egui_ctx.run(raw_input, |ctx| {
            self.app.update_ui(ctx);
        });
        egui_winit.handle_platform_output(window, full_output.platform_output);

        if let Some(vp_out) = full_output.viewport_output.remove(&egui::ViewportId::ROOT) {
            let mut should_exit = false;
            let mut filtered_cmds: Vec<egui::ViewportCommand> = Vec::with_capacity(vp_out.commands.len());
            for cmd in vp_out.commands {
                match cmd {
                    egui::ViewportCommand::Close => {
                        if QUIT.load(Ordering::Relaxed) {
                            should_exit = true;
                        } else {
                            win32_window::cache_main_hwnd();
                            WINDOW_VISIBLE.store(false, Ordering::Relaxed);
                            window.set_visible(false);
                        }
                    }
                    egui::ViewportCommand::CancelClose => {}
                    egui::ViewportCommand::Visible(false) => {
                        win32_window::cache_main_hwnd();
                        WINDOW_VISIBLE.store(false, Ordering::Relaxed);
                        window.set_visible(false);
                    }
                    egui::ViewportCommand::Visible(true) => {
                        WINDOW_VISIBLE.store(true, Ordering::Relaxed);
                        window.set_visible(true);
                    }
                    other => filtered_cmds.push(other),
                }
            }
            if !filtered_cmds.is_empty() {
                let mut actions = Vec::new();
                egui_winit::process_viewport_commands(
                    &self.egui_ctx,
                    &mut self.viewport_info,
                    filtered_cmds,
                    window,
                    &mut actions,
                );
            }
            if should_exit {
                window.set_visible(false);
                event_loop.exit();
                return;
            }
            if WINDOW_VISIBLE.load(Ordering::Relaxed) {
                if vp_out.repaint_delay.is_zero() {
                    window.request_redraw();
                } else if vp_out.repaint_delay < Duration::from_secs(3600) {
                    let when = Instant::now() + vp_out.repaint_delay;
                    self.next_repaint = Some(match self.next_repaint {
                        Some(prev) => prev.min(when),
                        None => when,
                    });
                }
            }
        }

        if !WINDOW_VISIBLE.load(Ordering::Relaxed) {
            return;
        }

        let clipped_primitives = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        match surface.buffer_mut() {
            Ok(mut buffer) => {
                // Pure AMOLED Black (#000000) background
                buffer.fill(0);
                let mut buf_ref = BufferMutRef::new(
                    bytemuck::cast_slice_mut(&mut *buffer),
                    size.width as usize,
                    size.height as usize,
                );
                self.renderer.render(
                    &mut buf_ref,
                    &clipped_primitives,
                    &full_output.textures_delta,
                    full_output.pixels_per_point,
                );
                if let Err(e) = buffer.present() {
                    log(format!("softbuffer present failed: {e}"));
                }
            }
            Err(e) => {
                log(format!("softbuffer buffer_mut failed: {e}"));
            }
        }
    }
}

impl winit::application::ApplicationHandler<AppUserEvent> for SoftwareWindowHost {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        match egui_winit::create_window(&self.egui_ctx, event_loop, &self.viewport_builder) {
            Ok(win) => {
                let win = Rc::new(win);
                let surface = match softbuffer::Surface::new(&self.softbuffer_context, win.clone()) {
                    Ok(s) => s,
                    Err(e) => {
                        log(format!("Failed to create Win32 GDI softbuffer surface: {e}"));
                        event_loop.exit();
                        return;
                    }
                };
                let egui_winit = egui_winit::State::new(
                    self.egui_ctx.clone(),
                    egui::ViewportId::ROOT,
                    &*win,
                    Some(win.scale_factor() as f32),
                    None,
                    None,
                );
                win32_window::cache_main_hwnd();
                let init_size = win.inner_size();
                win32_window::apply_rounded_corners(init_size.width, init_size.height, win.scale_factor());
                if WINDOW_VISIBLE.load(Ordering::Relaxed) {
                    win.request_redraw();
                } else {
                    win.set_visible(false);
                }
                self.window = Some(win);
                self.surface = Some(surface);
                self.egui_winit = Some(egui_winit);
                log("Initialized pure Win32 GDI software UI (zero DXGI/OpenGL/G-SYNC)");
            }
            Err(e) => {
                log(format!("Failed to create application window: {e}"));
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: AppUserEvent) {
        if event_loop.exiting() {
            return;
        }
        match event {
            AppUserEvent::ShowWindow => {
                WINDOW_VISIBLE.store(true, Ordering::Relaxed);
                if let Some(window) = &self.window {
                    window.set_visible(true);
                    window.set_minimized(false);
                    let cur_size = window.inner_size();
                    win32_window::apply_rounded_corners(cur_size.width, cur_size.height, window.scale_factor());
                    window.focus_window();
                    window.request_redraw();
                }
            }
            AppUserEvent::Quit => {
                QUIT.store(true, Ordering::Relaxed);
                if let Some(window) = &self.window {
                    window.set_visible(false);
                }
                event_loop.exit();
            }
            AppUserEvent::RequestRepaint { when, cumulative_pass_nr } => {
                if !WINDOW_VISIBLE.load(Ordering::Relaxed) {
                    return;
                }
                let current_pass = self.egui_ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT);
                if current_pass == cumulative_pass_nr || current_pass == cumulative_pass_nr.saturating_add(1) {
                    if when <= Instant::now() {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    } else {
                        self.next_repaint = Some(match self.next_repaint {
                            Some(prev) => prev.min(when),
                            None => when,
                        });
                    }
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if event_loop.exiting() {
            return;
        }
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };
        if window_id != window.id() {
            return;
        }

        match event {
            winit::event::WindowEvent::CloseRequested => {
                if QUIT.load(Ordering::Relaxed) {
                    window.set_visible(false);
                    event_loop.exit();
                } else {
                    win32_window::cache_main_hwnd();
                    WINDOW_VISIBLE.store(false, Ordering::Relaxed);
                    window.set_visible(false);
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                self.render_frame(event_loop);
            }
            other => {
                if let winit::event::WindowEvent::Resized(new_size) = &other {
                    win32_window::apply_rounded_corners(
                        new_size.width,
                        new_size.height,
                        window.scale_factor(),
                    );
                }
                if matches!(other, winit::event::WindowEvent::Focused(true)) {
                    WINDOW_VISIBLE.store(true, Ordering::Relaxed);
                }
                if let Some(egui_winit) = self.egui_winit.as_mut() {
                    let resp = egui_winit.on_window_event(&window, &other);
                    if resp.repaint && WINDOW_VISIBLE.load(Ordering::Relaxed) {
                        window.request_redraw();
                    }
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if event_loop.exiting() {
            return;
        }
        if QUIT.load(Ordering::Relaxed) {
            if let Some(window) = &self.window {
                window.set_visible(false);
            }
            event_loop.exit();
            return;
        }
        if !WINDOW_VISIBLE.load(Ordering::Relaxed) {
            self.next_repaint = None;
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            return;
        }
        if let Some(when) = self.next_repaint {
            let now = Instant::now();
            if now >= when {
                self.next_repaint = None;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            } else {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(when));
            }
        } else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        }
    }
}

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

    let mut config = {
        let path = data_dir().join("config.json");
        if !path.exists() && !exe_dir().join("config.json").exists() {
            save_json("config.json", &AppConfig::default());
        }
        load_json::<AppConfig>("config.json")
    };
    config.use_custom_toast_app_id = true;
    set_autostart(config.start_with_windows);
    register_toast_app_id(true);

    let saved_last_match: Option<LastMatchSummary> = load_json("last_match.json");
    let initial_last_match_id = saved_last_match.as_ref().map(|m| m.match_id.clone());

    let state = Arc::new(Mutex::new(AppState {
        status: "Idle | Monitoring ShooterGame.log".to_string(),
        is_scanning: false,
        is_syncing_directory: false,
        riot_status: RiotStatus::Offline,
        detected_api_region: "ap".to_string(),
        detected_server: String::new(),
        config: config.clone(),
        history: load_json("streamer_history.json"),
        local_directory: load_json("custom_streamers.json"),
        remote_directory: load_clean_remote_directory_cache(),
        toast: None,
        last_scanned_match: initial_last_match_id,
        last_match_summary: saved_last_match,
        chat_buffer: Vec::new(),
        yt_key_check: CheckState::Idle,
        twitch_key_check: CheckState::Idle,
        streamer_verify: CheckState::Idle,
        update_state: UpdateState::Idle,
        notified_update_key: None,
        recommendations: RecommendationsState::Idle,
        recommendations_cache: None,
    }));

    let tray_menu = Menu::new();
    let open_item = MenuItem::new("Open Streamer History", true, None);
    let scan_item = MenuItem::new("Check Last Match Now", true, None);
    let update_item = MenuItem::new("Check for Updates", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let _ = tray_menu.append_items(&[&open_item, &scan_item, &update_item, &quit_item]);
    let (open_id, scan_id, update_id, quit_id) = (
        open_item.id().clone(),
        scan_item.id().clone(),
        update_item.id().clone(),
        quit_item.id().clone(),
    );

    // Keep alive for the whole run; dropped (icon removed) when event_loop.run_app returns.
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

    let viewport_builder = egui::ViewportBuilder::default()
        .with_title("Valorant Streamer Radar")
        .with_icon(window_icon)
        .with_decorations(false)
        .with_resizable(true)
        .with_inner_size([1120.0, 740.0])
        .with_min_inner_size([860.0, 560.0])
        .with_visible(!start_hidden);

    let event_loop: winit::event_loop::EventLoop<AppUserEvent> =
        winit::event_loop::EventLoop::with_user_event()
            .build()
            .map_err(|e| anyhow!("Failed to create event loop: {e}"))?;
    let proxy = event_loop.create_proxy();
    let _ = EVENT_LOOP_PROXY.set(proxy.clone());

    let softbuffer_context = softbuffer::Context::new(event_loop.owned_display_handle())
        .map_err(|e| anyhow!("Failed to initialize Win32 GDI softbuffer context: {e}"))?;

    let ctx = egui::Context::default();
    let repaint_proxy = proxy.clone();
    ctx.set_request_repaint_callback(move |info| {
        let _ = repaint_proxy.send_event(AppUserEvent::RequestRepaint {
            when: Instant::now() + info.delay,
            cumulative_pass_nr: info.current_cumulative_pass_nr,
        });
    });

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
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => {
            show_window(&ctx_tray);
        }
        _ => {}
    }));
    let ctx_menu = ctx.clone();
    let tx_menu = scan_tx.clone();
    let bg_menu = bg_tx.clone();
    let st_menu = state.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            show_window(&ctx_menu);
        } else if event.id == scan_id {
            let _ = tx_menu.send(ScanTrigger::Manual);
        } else if event.id == update_id {
            lock(&st_menu).update_state = UpdateState::Checking;
            let _ = bg_menu.send(BgTask::CheckForUpdates { silent: false });
            show_window(&ctx_menu);
        } else if event.id == quit_id {
            quit_app(&ctx_menu);
        }
    }));

    let state_bg = state.clone();
    let ctx_bg = ctx.clone();
    let tx_bg = scan_tx.clone();
    let bg_startup = bg_tx.clone();

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

            // Non-blocking worker for API key tests, verification, updates, & live recommendations.
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
                            BgTask::CheckForUpdates { silent } => {
                                let res = check_for_app_update(&client).await;
                                let mut s = lock(&st);
                                match res {
                                    UpdateState::Available(info) => {
                                        let key = format!(
                                            "{}|{}",
                                            info.tag_name,
                                            info.remote_sha.clone().unwrap_or_default()
                                        );
                                        let already_notified =
                                            s.notified_update_key.as_deref() == Some(key.as_str());
                                        s.update_state = UpdateState::Available(info);
                                        if !already_notified {
                                            s.notified_update_key = Some(key);
                                        }
                                    }
                                    UpdateState::UpToDate(msg) => {
                                        s.notified_update_key = None;
                                        // Auto-hide notification when no update is available; only show inline in Settings on manual check
                                        s.update_state = if silent {
                                            UpdateState::Idle
                                        } else {
                                            UpdateState::UpToDate(msg)
                                        };
                                    }
                                    UpdateState::Error(msg) => {
                                        // Never misreport failed checks as up-to-date; quiet on background startup, visible on manual check
                                        if !silent {
                                            s.update_state = UpdateState::Error(msg);
                                        } else {
                                            log(format!("Silent update check failed: {msg}"));
                                        }
                                    }
                                    other => {
                                        s.update_state = other;
                                    }
                                }
                                drop(s);
                                repaint(&cx);
                            }
                            BgTask::InstallUpdate(info) => {
                                download_and_install_update(info, st, cx).await;
                            }
                            BgTask::FetchRecommendations {
                                server_region,
                                youtube_api_key,
                                twitch_client_id,
                                twitch_client_secret,
                                force,
                            } => {
                                // Guard: zero network calls if recommendations were turned OFF before task ran
                                if !lock(&st).config.enable_recommendations {
                                    lock(&st).recommendations = RecommendationsState::Idle;
                                    repaint(&cx);
                                    return;
                                }
                                if !force {
                                    let cached = {
                                        let s = lock(&st);
                                        s.recommendations_cache.as_ref().and_then(|(reg, at, list, lbl)| {
                                            if reg.eq_ignore_ascii_case(&server_region)
                                                && at.elapsed() < RECOMMENDATIONS_CACHE_TTL
                                            {
                                                Some((list.clone(), lbl.clone()))
                                            } else {
                                                None
                                            }
                                        })
                                    };
                                    if let Some((list, lbl)) = cached {
                                        lock(&st).recommendations = RecommendationsState::Loaded(list, lbl);
                                        repaint(&cx);
                                        return;
                                    }
                                }
                                let reg = server_region_by_code(&server_region)
                                    .unwrap_or(&SERVER_REGIONS[0]);
                                match fetch_live_recommendations(
                                    &client,
                                    reg,
                                    &youtube_api_key,
                                    &twitch_client_id,
                                    &twitch_client_secret,
                                )
                                .await
                                {
                                    Ok(list) => {
                                        let at = Local::now().format("%I:%M %p").to_string();
                                        let mut s = lock(&st);
                                        s.recommendations_cache = Some((
                                            reg.code.to_string(),
                                            Instant::now(),
                                            list.clone(),
                                            at.clone(),
                                        ));
                                        s.recommendations = RecommendationsState::Loaded(list, at);
                                    }
                                    Err(e) => {
                                        lock(&st).recommendations = RecommendationsState::Error(format!(
                                            "Could not fetch recommendations: {}",
                                            short(&e.to_string(), 120)
                                        ));
                                    }
                                }
                                repaint(&cx);
                            }
                        }
                    });
                }
            });

            // Optional silent update check on boot (ON by default; never pops a toast or forces window open,
            // only appears inside the GUI when the user opens the window).
            if lock(&state_bg).config.check_updates_on_boot {
                let _ = bg_startup.send(BgTask::CheckForUpdates { silent: true });
            }

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

            watch_log(valorant_log_path(), local_client, state_bg, ctx_bg, tx_bg).await;
        });
    });
    if let Err(e) = spawned {
        log(format!("Failed to spawn background thread: {e}"));
    }

    let app = StreamerApp::new(&ctx, state, scan_tx, bg_tx, start_hidden);
    let mut host = SoftwareWindowHost::new(softbuffer_context, viewport_builder, ctx, app);
    event_loop
        .run_app(&mut host)
        .map_err(|e| anyhow!("GUI error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(
        match_id: &str,
        riot_id: &str,
        queue: &str,
        server: &str,
        tier: &str,
        iso: &str,
        chat: &str,
    ) -> StreamerEntry {
        StreamerEntry {
            match_id: match_id.to_string(),
            riot_id: riot_id.to_string(),
            platform: "Twitch".to_string(),
            channel: "SampleChan".to_string(),
            title: "VALORANT Ranked".to_string(),
            url: "https://www.twitch.tv/samplechan".to_string(),
            timestamp: "2026-06-15 08:30 PM".to_string(),
            detected_at_iso: iso.to_string(),
            team_relation: "Enemy".to_string(),
            map_name: "Ascent".to_string(),
            agent_name: "Jett".to_string(),
            queue_name: queue.to_string(),
            chat_message: chat.to_string(),
            source: if !chat.is_empty() { "chat_plug".to_string() } else { "heuristic".to_string() },
            confidence: if tier == "Confirmed" { 96 } else if tier == "Probable" { 78 } else { 54 },
            confidence_tier: tier.to_string(),
            server_match_status: "In-Game Server Match".to_string(),
            match_reason: "Test".to_string(),
            server_region: server.to_string(),
            candidates: Vec::new(),
        }
    }

    #[test]
    fn test_version_comparison_detects_newer_and_avoids_false_updates() {
        assert!(has_newer_release("1.2.0", "v1.2.1", None, None, None, "abc1234"));
        assert!(has_newer_release("1.0.1", "v1.2.0", None, None, None, "abc1234"));
        assert!(!has_newer_release("1.2.0", "v1.2.0", None, None, None, "abc1234"));
        assert!(!has_newer_release("1.2.0", "v1.0.1", None, None, None, "abc1234"));
    }

    #[test]
    fn test_riot_status_avoids_valorant_active_false_positives() {
        // Riot Client open in tray or playing another Riot game must NOT report ValorantActive
        assert_eq!(classify_riot_status(false, true, true), RiotStatus::RiotClientOnly);
        // Stale lockfile with neither process running must report Offline
        assert_eq!(classify_riot_status(false, false, true), RiotStatus::Offline);
        // Everything closed must report Offline
        assert_eq!(classify_riot_status(false, false, false), RiotStatus::Offline);
        // Only actual VALORANT-Win64-Shipping.exe running reports ValorantActive
        assert_eq!(classify_riot_status(true, true, true), RiotStatus::ValorantActive);
        assert_eq!(classify_riot_status(true, false, false), RiotStatus::ValorantActive);
    }

    #[test]
    fn test_server_pod_parsing_includes_manila_austin_tokyo_mumbai() {
        assert_eq!(parse_game_pod_to_server("aresriot.aws-ap-south-1-mumbai-1"), Some("ap-mumbai"));
        assert_eq!(parse_game_pod_to_server("aresriot.aws-ap-southeast-manila-1"), Some("ap-manila"));
        assert_eq!(parse_game_pod_to_server("aresriot.aws-ap-northeast-1-tokyo-2"), Some("ap-tokyo"));
        assert_eq!(parse_game_pod_to_server("aresriot.aws-us-central-austin-1"), Some("na-austin"));
        assert_eq!(parse_game_pod_to_server("aresriot.aws-us-central-dallas-1"), Some("na-austin"));
        assert_eq!(server_region_by_code("manila").map(|r| r.code), Some("ap-manila"));
        assert_eq!(server_region_by_code("austin").map(|r| r.code), Some("na-austin"));
        assert_eq!(server_region_by_code("tokyo").map(|r| r.code), Some("ap-tokyo"));
    }

    #[test]
    fn test_mumbai_server_rejects_american_streamer_false_positive() {
        let mumbai = server_region_by_code("ap-mumbai").expect("mumbai server");
        let rejected = score_heuristic_candidate(
            "Twitch",
            "KairoFPS",
            "KairoFPS",
            "VALORANT NA Ranked Grind | Texas / US East Radiant",
            "https://www.twitch.tv/kairofps",
            Some("en"),
            true,
            mumbai,
        );
        assert!(rejected.is_none(), "NA streamer must be rejected on Mumbai server");
    }

    #[test]
    fn test_unknown_region_never_becomes_confirmed_match() {
        let mumbai = server_region_by_code("ap-mumbai").expect("mumbai server");
        let res = score_heuristic_candidate(
            "Twitch",
            "UniqueViper99",
            "UniqueViper99",
            "VALORANT Chill Ranked Games",
            "https://www.twitch.tv/uniqueviper99",
            None,
            true,
            mumbai,
        )
        .expect("exact handle + confirmed VALORANT should still surface as Unverified");
        assert_eq!(res.confidence_tier, "Unverified");
        assert!(res.confidence <= 62);
    }

    #[test]
    fn test_mumbai_explicit_server_cue_becomes_probable_not_confirmed() {
        let mumbai = server_region_by_code("ap-mumbai").expect("mumbai server");
        let res = score_heuristic_candidate(
            "YouTube",
            "RohanClutch",
            "RohanClutch",
            "VALORANT Mumbai Server Ranked Live | Immortal Grind",
            "https://www.youtube.com/watch?v=test123",
            Some("hi"),
            true,
            mumbai,
        )
        .expect("matching Mumbai server cue should score as Probable");
        assert_eq!(res.confidence_tier, "Probable");
        assert!(res.confidence >= 68);
    }

    #[test]
    fn test_generic_ign_cannot_trigger_heuristic_detection() {
        let mumbai = server_region_by_code("ap-mumbai").expect("mumbai server");
        let res = score_heuristic_candidate(
            "Twitch",
            "Shadow",
            "Shadow",
            "VALORANT Mumbai Server Live",
            "https://www.twitch.tv/shadow",
            Some("en"),
            true,
            mumbai,
        );
        assert!(res.is_none(), "Generic IGN 'Shadow' must be rejected");
    }

    #[test]
    fn test_directory_aliases_and_cross_region_caution() {
        let entry = DirectoryStreamer {
            id: "twitch-tenz".to_string(),
            display_name: "TenZ".to_string(),
            riot_id: "TenZ#0505".to_string(),
            aliases: vec!["SEN TenZ#NA1".to_string()],
            platform: "Twitch".to_string(),
            channel_id: "tenz".to_string(),
            channel_name: "TenZ".to_string(),
            channel_url: "https://www.twitch.tv/tenz".to_string(),
            server_region: "na-us-east".to_string(),
            verified: true,
            submitted_at: String::new(),
            source: "community".to_string(),
        };
        assert_eq!(
            directory_entry_matches_player(&entry, "TenZ#0505"),
            Some("Exact Riot ID in streamer database")
        );
        assert_eq!(
            directory_entry_matches_player(&entry, "SEN TenZ#NA1"),
            Some("Known streamer alias in database")
        );
        assert_eq!(directory_entry_matches_player(&entry, "Random#IND"), None);

        let mumbai = server_region_by_code("ap-mumbai").expect("mumbai");
        let (tier_mismatch, _, _, _) = evaluate_directory_server_match(&entry, mumbai, "Exact Riot ID in streamer database");
        assert_eq!(tier_mismatch, ConfidenceTier::Unverified);

        let us_east = server_region_by_code("na-us-east").expect("us-east");
        let (tier_match, score, _, _) = evaluate_directory_server_match(&entry, us_east, "Exact Riot ID in streamer database");
        assert_eq!(tier_match, ConfidenceTier::Confirmed);
        assert_eq!(score, 100);
    }

    #[test]
    fn test_map_name_from_id_maps_plummet_to_summit() {
        assert_eq!(map_name_from_id("/Game/Maps/Plummet/Plummet"), "Summit");
        assert_eq!(map_name_from_id("/Game/Maps/Rook/Rook"), "Corrode");
        assert_eq!(map_name_from_id("/Game/Maps/Ascent/Ascent"), "Ascent");
    }

    #[test]
    fn test_party_member_exclusion_and_dynamic_changes() {
        let mk = |sub: &str, name: &str, party: &str, is_party: bool| MatchPlayerInfo {
            record: RiotNameRecord {
                subject: sub.to_string(),
                game_name: name.to_string(),
                tag_line: "IND".to_string(),
            },
            team_relation: "Ally".to_string(),
            agent_name: "Omen".to_string(),
            party_id: party.to_string(),
            is_party_member: is_party,
        };

        let players = vec![
            mk("me", "MySelf", "party-1", false),
            mk("p1", "FriendOne", "party-1", true),
            mk("p2", "FriendTwo", "party-1", true),
            mk("r1", "RandomAlly", "party-2", false),
            mk("e1", "EnemyStreamer", "party-3", false),
        ];

        let (scannable_skip, skipped, hidden_skip) = filter_scannable_players(players.clone(), "me", true);
        assert_eq!(skipped, 2);
        assert_eq!(hidden_skip, 0);
        assert_eq!(scannable_skip.len(), 2);
        assert!(scannable_skip.iter().all(|p| !p.is_party_member));

        let (scannable_all, skipped_none, hidden_none) = filter_scannable_players(players, "me", false);
        assert_eq!(skipped_none, 0);
        assert_eq!(hidden_none, 0);
        assert_eq!(scannable_all.len(), 4);
    }

    #[test]
    fn test_history_filters_combine_cleanly() {
        let now = Local::now();
        let today_iso = now.to_rfc3339();
        let old_iso = (now - chrono::Duration::days(15)).to_rfc3339();

        let e_today = sample_entry("m1", "Rohan#IND", "Competitive", "Mumbai (India)", "Confirmed", &today_iso, "");
        let e_old = sample_entry("m0", "TokyoPro#JP1", "Unrated", "Tokyo (Japan)", "Probable", &old_iso, "");
        let e_chat = sample_entry("m2", "Plugger#PH1", "Swiftplay", "Manila (Philippines)", "Confirmed", &today_iso, "live on twitch.tv/plugger");

        assert!(entry_matches_history_filters(&e_today, "rohan", TeamFilter::Enemy, "Competitive", "Mumbai", DateRangeFilter::Today, StatusFilter::Confirmed, now));
        assert!(!entry_matches_history_filters(&e_old, "", TeamFilter::All, "All", "All", DateRangeFilter::Last7Days, StatusFilter::All, now));
        assert!(entry_matches_history_filters(&e_old, "", TeamFilter::All, "Unrated", "Tokyo", DateRangeFilter::Last30Days, StatusFilter::Probable, now));
        assert!(entry_matches_history_filters(&e_chat, "", TeamFilter::All, "Swiftplay", "Manila", DateRangeFilter::Today, StatusFilter::ChatPlug, now));
        assert!(!entry_matches_history_filters(&e_today, "", TeamFilter::All, "All", "All", DateRangeFilter::AllTime, StatusFilter::ChatPlug, now));
    }

    #[test]
    fn test_legacy_config_shard_override_alias_deserializes_cleanly() {
        let legacy_json = r#"{"shard_override":"ap-manila","enable_recommendations":false}"#;
        let cfg: AppConfig = serde_json::from_str(legacy_json).expect("legacy config must deserialize");
        assert_eq!(cfg.server_override, "ap-manila");
        assert!(cfg.skip_party_members, "skip_party_members should default to true");
        assert!(!cfg.enable_recommendations, "recommendations must default to OFF");
    }

    #[test]
    fn test_resolve_match_players_keeps_all_players_when_name_service_is_partial() {
        let details_players = vec![
            serde_json::json!({"subject": "ME-UUID", "gameName": "LocalPlayer", "tagLine": "IND", "teamId": "Blue", "characterId": "", "partyId": "party-real-1"}),
            serde_json::json!({"subject": "p1", "gameName": "DuoMate", "tagLine": "IND", "teamId": "Blue", "characterId": "", "partyId": "party-real-1"}),
            serde_json::json!({"subject": "p2", "gameName": "SoloAlly", "tagLine": "AP", "teamId": "Blue", "characterId": "", "partyId": "party-other"}),
            serde_json::json!({"subject": "p3", "gameName": "MissingFromNameService", "tagLine": "TTV", "teamId": "Red", "characterId": "", "partyId": "party-enemy"}),
            serde_json::json!({"subject": "p4", "gameName": "EnemyWithSamePartyId", "tagLine": "YT", "teamId": "Red", "characterId": "", "partyId": "party-real-1"}),
        ];
        // Simulate name-service returning only 2 records and one with a blank GameName
        let name_records = vec![
            RiotNameRecord { subject: "me-uuid".to_string(), game_name: "LocalPlayer".to_string(), tag_line: "IND".to_string() },
            RiotNameRecord { subject: "p2".to_string(), game_name: String::new(), tag_line: String::new() },
        ];
        let (_, players) = resolve_match_players(&details_players, &name_records, "me-uuid");
        let (scannable, party_skipped, hidden_skipped) = filter_scannable_players(players, "me-uuid", true);

        assert_eq!(party_skipped, 1, "Only true same-team duo mate should be skipped");
        assert_eq!(hidden_skipped, 0);
        assert_eq!(scannable.len(), 3, "All 3 non-party players (including omitted name-service and enemy) must be scanned");
        assert!(scannable.iter().any(|p| p.record.game_name == "SoloAlly"));
        assert!(scannable.iter().any(|p| p.record.game_name == "MissingFromNameService"));
        assert!(scannable.iter().any(|p| p.record.game_name == "EnemyWithSamePartyId"));
    }

    #[test]
    fn test_parse_name_service_records_handles_nulls_and_display_name() {
        let payload = serde_json::json!([
            {"Subject": "u1", "GameName": null, "TagLine": null, "DisplayName": "StreamerOne#NA1"},
            {"puuid": "u2", "gameName": "NormalGuy", "tagLine": "IND"},
            {"Subject": "u3", "GameName": "\u{3164}", "TagLine": "0000"}
        ]);
        let recs = parse_name_service_records(&payload);
        assert_eq!(recs.len(), 3, "Null fields must not fail parsing for the whole batch");
        assert_eq!(recs[0].game_name, "StreamerOne");
        assert_eq!(recs[0].tag_line, "NA1");
        assert_eq!(recs[1].subject, "u2");
        assert_eq!(recs[1].game_name, "NormalGuy");
        assert!(!is_visible_game_name(&recs[2].game_name));
    }

    #[test]
    fn test_nil_party_id_and_ffa_never_skip_non_party_players() {
        let nil = "00000000-0000-0000-0000-000000000000";
        assert!(!is_valid_party_id(""));
        assert!(!is_valid_party_id(nil));
        assert!(is_valid_party_id("a1b2c3d4-0000-0000-0000-000000000001"));

        let dm_players: Vec<Value> = (0..10)
            .map(|i| {
                serde_json::json!({
                    "subject": format!("sub-{i}"),
                    "gameName": format!("Player{i}"),
                    "tagLine": "IND",
                    "teamId": "Blue",
                    "characterId": "",
                    "partyId": nil
                })
            })
            .collect();
        let (_, players) = resolve_match_players(&dm_players, &[], "sub-0");
        let (scannable, party_skipped, hidden_skipped) = filter_scannable_players(players, "sub-0", true);
        assert_eq!(party_skipped, 0, "Nil partyId must never skip any players");
        assert_eq!(hidden_skipped, 0);
        assert_eq!(scannable.len(), 9, "All 9 other players must be checked");
    }

    #[test]
    fn test_purge_seeded_sample_streamers_removes_legacy_entries() {
        let mk = |id: &str, riot: &str| DirectoryStreamer {
            id: id.to_string(),
            display_name: riot.to_string(),
            riot_id: riot.to_string(),
            aliases: Vec::new(),
            platform: "Twitch".to_string(),
            channel_id: id.to_string(),
            channel_name: riot.to_string(),
            channel_url: format!("https://www.twitch.tv/{id}"),
            server_region: "ap-mumbai".to_string(),
            verified: true,
            submitted_at: String::new(),
            source: "community-verified".to_string(),
        };
        let entries = vec![
            mk("rohan-gaming-yt", "RohanYT#IND"),
            mk("vexfps-twitch", "VexFPS#MUM"),
            mk("real-streamer-twitch", "RealStreamer#IND"),
        ];
        let cleaned = purge_seeded_sample_streamers(entries);
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].id, "real-streamer-twitch");
    }

    #[test]
    fn test_check_updates_on_boot_defaults_true_for_new_and_legacy_configs() {
        assert!(AppConfig::default().check_updates_on_boot);
        let legacy: AppConfig = serde_json::from_str("{}").expect("empty legacy config");
        assert!(legacy.check_updates_on_boot);
        let disabled: AppConfig =
            serde_json::from_str(r#"{"check_updates_on_boot": false}"#).expect("disabled config");
        assert!(!disabled.check_updates_on_boot);
    }

    #[test]
    fn test_clean_ign_strips_leading_and_trailing_platform_tags() {
        assert_eq!(clean_ign("TTV JakeFPS"), "JakeFPS");
        assert_eq!(clean_ign("YT_Rohan"), "Rohan");
        assert_eq!(clean_ign("twitch.tv/shroud"), "shroud");
        assert_eq!(clean_ign("Kick-Kairo"), "Kairo");
        assert_eq!(clean_ign("RohanYT"), "Rohan");
        assert_eq!(clean_ign("Desktop"), "Desktop");
    }

    #[test]
    fn test_chat_parsing_handles_nulls_and_excludes_party_chat() {
        let payload = serde_json::json!({
            "messages": [
                {"id": null, "cid": "123@ares-coregame.ap.pvp.net", "puuid": "p1", "game_name": null, "body": "live on twitch.tv/foo", "type": "groupchat"},
                {"id": "2", "cid": "456@ares-parties.ap.pvp.net", "puuid": "p2", "game_name": "Friend", "body": "check twitch.tv/bar", "type": "groupchat"}
            ]
        });
        let msgs = parse_chat_messages(&payload);
        assert_eq!(msgs.len(), 2);
        assert!(is_match_chat_message(&msgs[0]));
        assert!(!is_match_chat_message(&msgs[1]), "Party chat must never leak into match lobby scan");
    }

    #[test]
    fn test_client_version_log_fallback_formats_shipping_correctly() {
        let line = "[2026.05.01] LogShooter: CI server version: release-10.08-9-3416843\n";
        assert_eq!(
            parse_client_version_from_log_text(line).as_deref(),
            Some("release-10.08-shipping-9-3416843")
        );
        let already = "CI server version: release-10.08-shipping-9-3416843";
        assert_eq!(
            parse_client_version_from_log_text(already).as_deref(),
            Some("release-10.08-shipping-9-3416843")
        );
    }

    #[test]
    fn test_youtube_live_search_filter_and_exact_handle_unverified_scoring() {
        let live_video = serde_json::json!({
            "videoId": "abc",
            "badges": [{"metadataBadgeRenderer": {"style": "BADGE_STYLE_TYPE_LIVE_NOW", "label": "LIVE"}}]
        });
        let upcoming_video = serde_json::json!({
            "videoId": "def",
            "upcomingEventData": {"startTime": "1790000000"},
            "badges": [{"metadataBadgeRenderer": {"label": "UPCOMING"}}]
        });
        let vod_video = serde_json::json!({"videoId": "ghi"});
        assert!(is_yt_search_video_live(&live_video));
        assert!(!is_yt_search_video_live(&upcoming_video));
        assert!(!is_yt_search_video_live(&vod_video));

        let mumbai = server_region_by_code("ap-mumbai").expect("mumbai");
        let exact_yt = score_heuristic_candidate(
            "YouTube",
            "UniqueValCreator",
            "UniqueValCreator",
            "VALORANT Ranked Grind!",
            "https://www.youtube.com/watch?v=abc",
            None,
            false,
            mumbai,
        )
        .expect("Exact-handle YouTube Valorant stream should surface as Unverified");
        assert_eq!(exact_yt.confidence_tier, "Unverified");
        assert!(exact_yt.confidence <= 58);
    }
}


