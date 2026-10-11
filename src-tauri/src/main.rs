#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Utc;
use log::{error, info, warn};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::Manager;
use tauri_plugin_liquid_glass::{GlassMaterialVariant, LiquidGlassConfig, LiquidGlassExt};
use tokio::time;

const API_URL_COM: &str = "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains";
const API_URL_IO: &str = "https://api.minimax.io/v1/api/openplatform/coding_plan/remains";
// Keychain layout: each Minimax endpoint has its own keychain item so the
// stored keys are named subbar-minimax (for `.com`) and subbar-minimaxi (for
// `.io`).
const KEYRING_SERVICE_COM: &str = "subbar-minimax";
const KEYRING_SERVICE_IO: &str = "subbar-minimaxi";
const KEYRING_SERVICE_OCG_WS: &str = "subbar-ocg-ws";
const KEYRING_SERVICE_OCG_COOKIE: &str = "subbar-ocg-cookie";
const KEYRING_USER: &str = "subbar";

// OCG reads usage from the OpenCode console API in-process and has no in-app
// API key. When the console rejects the session cookie / has no usable
// credentials, the tray title shows this clear status instead of a raw error
// marker.
const OCG_NO_AUTH_TITLE: &str = "unauth";

// SEC-6-18: widget dimensions — single source of truth shared with
// `src/style.css` (`.widget` rule). The CSS defines an inner content area of
// 191×191 logical px; the liquid-glass `corner_radius: 24.0` adds padding so
// the actual window outer is 212×235 logical px. Update both together.
const WIDGET_INNER_WIDTH: f64 = 191.0;
const WIDGET_INNER_HEIGHT: f64 = 191.0;
const WIDGET_OUTER_WIDTH: f64 = 212.0;

struct AppState {
    api_key_com: Mutex<String>,
    api_key_io: Mutex<String>,
    ocg_workspace_id: Mutex<String>,
    ocg_auth_cookie: Mutex<String>,
    endpoint: Mutex<String>,
    refresh_interval_secs: Mutex<u64>,
    last_used_5h: Mutex<u32>,
    last_used_week: Mutex<u32>,
    last_used_month: Mutex<u32>,
}

fn keyring_entry(service: &str, user: &str) -> Option<keyring::Entry> {
    match keyring::Entry::new(service, user) {
        Ok(entry) => Some(entry),
        Err(e) => {
            warn!("keyring entry creation failed for {}: {:?}", user, e);
            None
        }
    }
}

fn load_key_from_service(service: &str, user: &str) -> String {
    keyring_entry(service, user)
        .and_then(|entry| entry.get_password().ok())
        .unwrap_or_default()
}

fn load_key_from_keyring(service: &str, user: &str) -> String {
    load_key_from_service(service, user)
}

fn save_key_to_keyring(service: &str, user: &str, key: &str) {
    if key.is_empty() {
        let _ = keyring_entry(service, user).and_then(|entry| entry.delete_credential().ok());
    } else {
        if let Some(entry) = keyring_entry(service, user) {
            if let Err(e) = entry.set_password(key) {
                error!("keyring write failed for {}: {:?}", user, e);
            }
        }
    }
}

fn com_keyring() -> (&'static str, &'static str) {
    (KEYRING_SERVICE_COM, KEYRING_USER)
}

fn io_keyring() -> (&'static str, &'static str) {
    (KEYRING_SERVICE_IO, KEYRING_USER)
}

fn ocg_ws_keyring() -> (&'static str, &'static str) {
    (KEYRING_SERVICE_OCG_WS, KEYRING_USER)
}

fn ocg_cookie_keyring() -> (&'static str, &'static str) {
    (KEYRING_SERVICE_OCG_COOKIE, KEYRING_USER)
}

fn endpoint_keyring(endpoint: &str) -> Option<(&'static str, &'static str)> {
    match endpoint {
        "com" => Some(com_keyring()),
        "io" => Some(io_keyring()),
        _ => None,
    }
}

fn migrate_config_json_if_needed(data_dir: &PathBuf) {
    let path = data_dir.join("config.json");
    if !path.exists() {
        return;
    }
    info!("Migrating legacy config.json to keychain...");
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            let com = v
                .get("api_key_com")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("api_key").and_then(|x| x.as_str()))
                .unwrap_or("")
                .to_string();
            let io = v
                .get("api_key_io")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();

            if !com.is_empty() {
                let (s, u) = com_keyring();
                save_key_to_keyring(s, u, &com);
            }
            if !io.is_empty() {
                let (s, u) = io_keyring();
                save_key_to_keyring(s, u, &io);
            }
            info!("Migration complete. Deleting config.json.");
        }
    }
    if let Err(e) = fs::remove_file(&path) {
        warn!("Failed to delete config.json: {:?}", e);
    }
}

#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// Pick the stored API key for the given endpoint. Pure so the com/io routing is
// unit-testable and both endpoints behave identically.
fn select_api_key(endpoint: &str, com_key: &str, io_key: &str) -> String {
    match endpoint {
        "ocg" => String::new(),
        "io" => io_key.to_string(),
        _ => com_key.to_string(),
    }
}

#[tauri::command]
fn get_api_key(endpoint: String, reveal: Option<bool>, state: tauri::State<AppState>) -> String {
    // OCG reads from the OpenCode Go credentials and has no API key of its own.
    let com = state
        .api_key_com
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let io = state
        .api_key_io
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let key = select_api_key(&endpoint, &com, &io);
    // SEC-6-6: default to redacted so the plaintext key never enters the
    // webview unless the user explicitly clicks the eye (reveal=true).
    if reveal.unwrap_or(false) {
        key
    } else if key.is_empty() {
        String::new()
    } else {
        redact_api_key(&key)
    }
}

#[tauri::command]
fn set_api_key(key: String, endpoint: String, state: tauri::State<AppState>) {
    // OCG has no API key — usage comes from the OpenCode console API, which
    // authenticates with the session cookie stored by `set_ocg_credentials`.
    // Never store/overwrite an API key for it.
    if endpoint == "ocg" {
        log::debug!("set_api_key: ocg endpoint uses the console session cookie, no key stored");
        return;
    }
    let key = key.trim();
    if !key.is_empty() && (!key.starts_with("sk-") || key.len() < 4 || key.len() > 256) {
        // SEC-6-7: log only booleans / counts — never a fragment of the key.
        let prefix_ok = key.starts_with("sk-");
        let len_ok = (4..=256).contains(&key.len());
        log::warn!(
            "set_api_key: rejected (endpoint={} prefix_ok={} len_ok={})",
            endpoint,
            prefix_ok,
            len_ok,
        );
        return;
    }
    let (service, user) = match endpoint_keyring(&endpoint) {
        Some(v) => v,
        None => {
            log::warn!("set_api_key: unknown endpoint '{}'", endpoint);
            return;
        }
    };
    save_key_to_keyring(service, user, &key);

    if endpoint == "io" {
        *state.api_key_io.lock().unwrap_or_else(|e| e.into_inner()) = key.to_string();
    } else {
        *state.api_key_com.lock().unwrap_or_else(|e| e.into_inner()) = key.to_string();
    }
    *state.endpoint.lock().unwrap_or_else(|e| e.into_inner()) = endpoint;
}

// OpenCode Go does not use an API key; it authenticates with a workspace ID and
// a session auth cookie scraped from the dashboard. Both are stored in the OS
// keychain (mirroring the Minimax key) and surfaced to the UI here. The cookie
// is returned redacted unless explicitly revealed.
#[tauri::command]
fn get_ocg_credentials(reveal: Option<bool>, state: tauri::State<AppState>) -> serde_json::Value {
    let ws = state
        .ocg_workspace_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let cookie = state
        .ocg_auth_cookie
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let cookie_out = if reveal.unwrap_or(false) {
        cookie.clone()
    } else if cookie.is_empty() {
        String::new()
    } else {
        redact_api_key(&cookie)
    };
    // SEC: the workspace ID is a secret (treated like the cookie/API key). Redact
    // it in the default response; only return plaintext when explicitly revealed.
    let ws_out = if reveal.unwrap_or(false) {
        ws.clone()
    } else if ws.is_empty() {
        String::new()
    } else {
        redact_api_key(&ws)
    };
    let has_credentials = !ws.is_empty() && !cookie.is_empty();
    serde_json::json!({
        "workspace_id": ws_out,
        "auth_cookie": cookie_out,
        "has_credentials": has_credentials,
    })
}

#[tauri::command]
fn set_ocg_credentials(
    workspace_id: String,
    auth_cookie: String,
    state: tauri::State<AppState>,
) {
    let ws = workspace_id.trim().to_string();
    let cookie = auth_cookie.trim().to_string();
    let has = !ws.is_empty() && !cookie.is_empty();
    let (ws_s, ws_u) = ocg_ws_keyring();
    let (c_s, c_u) = ocg_cookie_keyring();
    save_key_to_keyring(ws_s, ws_u, &ws);
    save_key_to_keyring(c_s, c_u, &cookie);
    *state
        .ocg_workspace_id
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = ws;
    *state
        .ocg_auth_cookie
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = cookie;
    log::debug!("set_ocg_credentials: stored (has={})", has);
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn set_refresh_interval(interval: u64, state: tauri::State<AppState>) {
    // Clamp to [10s, 24h] so the background loop cannot stall indefinitely.
    let interval = interval.clamp(10, 86400);
    *state
        .refresh_interval_secs
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = interval;
}

// The frontend owns the selected endpoint (persisted in localStorage) and must
// sync it to the backend so the background timer / tray title use the right
// data source. Without this, switching to "ocg" in the UI would leave the Rust
// endpoint at "com"/"io" and the menubar would keep rendering Minimax states
// (e.g. AUTH! for an invalid Minimax key).
fn normalize_endpoint(ep: &str) -> Option<String> {
    match ep.trim().to_lowercase().as_str() {
        "com" | "io" | "ocg" => Some(ep.trim().to_lowercase()),
        _ => None,
    }
}

fn persisted_endpoint_path(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("endpoint.txt")
}

fn load_persisted_endpoint(data_dir: &std::path::Path) -> String {
    fs::read_to_string(persisted_endpoint_path(data_dir))
        .ok()
        .and_then(|s| normalize_endpoint(&s))
        .unwrap_or_else(|| "com".to_string())
}

fn save_persisted_endpoint(data_dir: &std::path::Path, ep: &str) {
    let _ = fs::create_dir_all(data_dir);
    let _ = fs::write(persisted_endpoint_path(data_dir), ep);
}

#[tauri::command]
fn set_endpoint(ep: String, app: tauri::AppHandle, state: tauri::State<AppState>) {
    let Some(ep) = normalize_endpoint(&ep) else {
        log::debug!("set_endpoint: ignoring invalid endpoint");
        return;
    };
    *state.endpoint.lock().unwrap_or_else(|e| e.into_inner()) = ep.clone();
    // Persist so the backend starts on the correct endpoint next launch and
    // never flashes the wrong data source (e.g. AUTH! from a stale Minimax key).
    if let Ok(data_dir) = app.path().app_local_data_dir() {
        save_persisted_endpoint(&data_dir.join("subbar"), &ep);
    }
    log::debug!("set_endpoint: synced");
}

fn redact_api_key(key: &str) -> String {
    if key.len() <= 12 {
        "[REDACTED]".to_string()
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build shared reqwest client")
    })
}

async fn fetch_quota_from_api(url: &str, api_key: &str) -> Result<serde_json::Value, String> {
    log::debug!("fetch_quota: url={} key={}", url, redact_api_key(api_key));
    let resp = http_client()
        .get(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP error: {}", resp.status()));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(data)
}

fn render_title(app: &tauri::AppHandle, title: &str) {
    log::info!("tray title: {}", title);
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_title(Some(title.to_string()));
    }
    // The title text changes the status item's width, which moves its center.
    // Re-center the panel on the (new) item center so it never drifts out of
    // alignment as usage percentages update.
    reposition_dropdown(app);
}

// Re-center the panel under the current tray item. Used on launch, on tray
// click, and after every title change. No-op if the tray has no usable rect
// yet or the window is missing.
fn reposition_dropdown(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    match tray.rect() {
        Ok(Some(rect)) => {
            let has_size = match rect.size {
                tauri::Size::Physical(s) => s.width > 0 && s.height > 0,
                _ => false,
            };
            if has_size {
                let pos = compute_dropdown_position(&rect, &window);
                let tp = match rect.position {
                    tauri::Position::Physical(p) => p,
                    _ => tauri::PhysicalPosition::new(0, 0),
                };
                let ts = match rect.size {
                    tauri::Size::Physical(s) => s,
                    _ => tauri::PhysicalSize::new(0, 0),
                };
                let scale = window.scale_factor().unwrap_or(1.0);
                let win_w = window.outer_size().map(|s| s.width as f64).unwrap_or(0.0);
                let tray_cx = tp.x as f64 + ts.width as f64 / 2.0;
                let win_cx = pos.x + win_w / 2.0;
                log::info!(
                    "panel recentered: trayCenterX={:.1} winCenterX={:.1} offset={:.1} (scale={:.2})",
                    tray_cx,
                    win_cx,
                    tray_cx - win_cx,
                    scale
                );
                let _ = window.set_position(pos);
            }
        }
        _ => {}
    }
}

fn is_ocg_endpoint(endpoint: &str) -> bool {
    endpoint == "ocg"
}

async fn fetch_and_update(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    let endpoint = state
        .endpoint
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if is_ocg_endpoint(&endpoint) {
        return fetch_ocg_and_update(app).await;
    }
    let url = if endpoint == "io" {
        API_URL_IO
    } else {
        API_URL_COM
    };
    let key = if endpoint == "io" {
        state
            .api_key_io
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    } else {
        state
            .api_key_com
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    };
    if key.is_empty() {
        // No key: show a clear degraded marker so the menubar is not frozen.
        render_title(app, "KEY?");
        return Err("No API key set".into());
    }

    let data = match fetch_quota_from_api(url, &key).await {
        Ok(d) => d,
        Err(e) => {
            // Network / timeout failure: keep last-known values, mark with `?`.
            let (u5, uw) = {
                let s = app.state::<AppState>();
                let a = *s.last_used_5h.lock().unwrap_or_else(|e| e.into_inner());
                let b = *s.last_used_week.lock().unwrap_or_else(|e| e.into_inner());
                (a, b)
            };
            render_title(app, &format!("{}% {}%?", u5, uw));
            return Err(e);
        }
    };

    // Auth / API-level error: surface it in the menubar instead of freezing.
    if let Some(base) = data.get("base_resp") {
        let status = base
            .get("status_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if status != 0 {
            render_title(app, "AUTH!");
            return Ok(data);
        }
    }

    if let Some(remains) = data.get("model_remains").and_then(|v| v.as_array()) {
        for item in remains {
            if item.get("model_name").and_then(|v| v.as_str()) == Some("general") {
                let remaining_5h = item
                    .get("current_interval_remaining_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(100.0);
                let used_5h = (100.0 - remaining_5h).round() as u32;
                let remaining_week = item
                    .get("current_weekly_remaining_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(100.0);
                let used_week = (100.0 - remaining_week).round() as u32;

                {
                    let s = app.state::<AppState>();
                    *s.last_used_5h.lock().unwrap_or_else(|e| e.into_inner()) = used_5h;
                    *s.last_used_week.lock().unwrap_or_else(|e| e.into_inner()) = used_week;
                }
                render_title(app, &format!("{}% {}%", used_5h, used_week));
                break;
            }
        }
    } else {
        // Response shape unexpected: keep last-known, mark with `?`.
        let (u5, uw) = {
            let s = app.state::<AppState>();
            let a = *s.last_used_5h.lock().unwrap_or_else(|e| e.into_inner());
            let b = *s.last_used_week.lock().unwrap_or_else(|e| e.into_inner());
            (a, b)
        };
        render_title(app, &format!("{}% {}%?", u5, uw));
    }

    Ok(data)
}

#[tauri::command]
async fn fetch_quota(
    _state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    fetch_and_update(&app).await
}

// OCG (OpenCode Go) usage is fetched in-process by our own HTTP client from the
// OpenCode **console JSON API**, exactly like the Minimax pipeline — no
// external binary, no silent credential fallback. The console migrated from a
// server-rendered dashboard to an SPA at opencode.ai/console (changelog
// v1.18.26), so the legacy HTML scrape is gone. Credentials come from
// AppState, which `set_ocg_credentials` keeps current when the user saves
// them. If they are missing, this returns a clear error (mirroring how a
// missing Minimax key returns an error) instead of falling back to anything.
async fn run_agent_limits(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let s = app.state::<AppState>();
    let ws = s
        .ocg_workspace_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let cookie = s
        .ocg_auth_cookie
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    fetch_ocg_usage(&ws, &cookie).await
}

const OCG_CONSOLE_ORGS_URL: &str = "https://opencode.ai/console/api/orgs";
const OCG_CONSOLE_GO_STATUS_URL: &str = "https://opencode.ai/console/api/go/status";
// The Zen usage API is the preferred Go data source: one Bearer call, no
// workspace id, no browser session. Verified live 2026-10-11.
const OCG_ZEN_GO_USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
const OCG_USER_AGENT: &str = "SubBar (https://github.com/MarcoLeongDev/subbar)";
const OCG_FETCH_TIMEOUT_SECS: u64 = 10;

// The stored OCG credential is either a Go API key (`sk-…`, from the console
// API Keys page or the `opencode-go` entry in
// `~/.local/share/opencode/auth.json`) or a browser session cookie. The key
// is detected by prefix so a pasted `Bearer sk-…` header works too.
#[derive(PartialEq, Eq, Debug)]
enum OcgTokenKind {
    ApiKey,
    SessionCookie,
}

fn ocg_token_kind(raw: &str) -> OcgTokenKind {
    let token = raw.trim();
    let bare = token
        .strip_prefix("Bearer ")
        .or_else(|| token.strip_prefix("bearer "))
        .unwrap_or(token)
        .trim_start();
    if bare.starts_with("sk-") {
        OcgTokenKind::ApiKey
    } else {
        OcgTokenKind::SessionCookie
    }
}

// Strips an optional `Bearer ` scheme from the stored credential.
fn ocg_bearer_token(raw: &str) -> &str {
    let token = raw.trim();
    token
        .strip_prefix("Bearer ")
        .or_else(|| token.strip_prefix("bearer "))
        .map(|t| t.trim_start())
        .unwrap_or(token)
}

// Session cookie names the console accepts. Which one a browser holds depends
// on how the user signed in: `auth` / `__Host-auth` for the legacy app,
// `__Host-console_session` for the console SPA. A bare pasted value is sent
// under every name so any of those copies authenticates.
const OCG_SESSION_COOKIE_NAMES: [&str; 3] = ["auth", "__Host-auth", "__Host-console_session"];

// Builds the Cookie header from the stored credential. A full `name=value; …`
// header (the user pasted the whole devtools cookie value) is passed through
// as-is; a bare value is sent under every accepted session cookie name.
fn ocg_cookie_header(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains('=') {
        return trimmed.to_string();
    }
    OCG_SESSION_COOKIE_NAMES
        .iter()
        .map(|name| format!("{name}={trimmed}"))
        .collect::<Vec<_>>()
        .join("; ")
}

// Extracts the workspace id from a bare id (`wrk_…` / `org_…`) or a pasted
// dashboard URL (console `…/console/<id>/go` or the legacy `…/workspace/<id>/go`
// form, with or without scheme). Anything else is returned trimmed so the API
// can reject an explicit typo with a clear error.
fn ocg_workspace_id(raw: &str) -> String {
    const URL_PREFIXES: [&str; 8] = [
        "https://opencode.ai/console/",
        "http://opencode.ai/console/",
        "https://opencode.ai/workspace/",
        "http://opencode.ai/workspace/",
        "opencode.ai/console/",
        "opencode.ai/workspace/",
        "/console/",
        "/workspace/",
    ];
    let trimmed = raw.trim().trim_end_matches('/');
    for prefix in URL_PREFIXES {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let id = rest.split(['/', '?', '#']).next().unwrap_or("").trim();
            if !id.is_empty() {
                return id.to_string();
            }
        }
    }
    trimmed.to_string()
}

// Fetches OpenCode Go usage. A Go API key (`sk-…`) goes to the Zen usage API
// (no workspace id or browser session needed); anything else is treated as a
// browser session cookie for the console API. Returns JSON shaped like
// `{ providers: { opencodego: { limits: {...} } } }` so `parse_ocg_bars` can
// consume it unchanged.
async fn fetch_ocg_usage(
    workspace_id: &str,
    auth_cookie: &str,
) -> Result<serde_json::Value, String> {
    let credential = auth_cookie.trim();
    if credential.is_empty() {
        return Err(
            "OpenCode Go credential missing — paste your Go API key (console → API Keys, or the opencode-go entry in ~/.local/share/opencode/auth.json)"
                .into(),
        );
    }
    if ocg_token_kind(credential) == OcgTokenKind::ApiKey {
        return fetch_ocg_zen_usage(ocg_bearer_token(credential)).await;
    }
    fetch_ocg_console_usage(workspace_id, credential).await
}

// Fetches OpenCode Go usage for the given workspace from the console API,
// replicating the browser's console calls (verified against the live console).
// Used when the stored credential is a browser session cookie rather than a
// Go API key.
async fn fetch_ocg_console_usage(
    workspace_id: &str,
    cookie: &str,
) -> Result<serde_json::Value, String> {
    // The workspace id may be empty (auto-discovered from the account), a bare
    // id, or a pasted dashboard URL.
    let workspace = match ocg_workspace_id(workspace_id) {
        ws if ws.is_empty() => ocg_discover_workspace_id(cookie).await?,
        ws => ws,
    };

    let resp = http_client()
        .get(OCG_CONSOLE_GO_STATUS_URL)
        .header("User-Agent", OCG_USER_AGENT)
        .header("Cookie", ocg_cookie_header(cookie))
        .header("x-org-id", &workspace)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(OCG_FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("OpenCode Go request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        // SEC: the workspace id is a secret (treated like the cookie). Redact
        // it in every error message so it never leaks in logs or the UI.
        return match status.as_u16() {
            401 | 403 => Err(format!(
                "HTTP {status} — OpenCode Go session cookie rejected; copy the __Host-console_session cookie (DevTools → Application → Cookies → opencode.ai)"
            )),
            404 => Err(format!(
                "HTTP 404 — workspace {} not found in the OpenCode console; check the workspace id",
                redact_api_key(&workspace)
            )),
            400 => Err(format!("HTTP {status} — OpenCode Go rejected the request")),
            429 | 500..=599 => Err(format!("HTTP {status} (transient)")),
            _ => Err(format!("HTTP {status} from the OpenCode console API")),
        };
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("OpenCode Go: reading response: {e}"))?;
    let data: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("OpenCode Go: invalid API response: {e}"))?;

    let limits = ocg_limits_from_console(&data)?;
    Ok(ocg_limits_response(limits))
}

// Wraps parsed usage windows in the provider shape `parse_ocg_bars` consumes.
fn ocg_limits_response(
    limits: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "providers": {
            "opencodego": {
                "limits": serde_json::Value::Object(limits)
            }
        }
    })
}

// Fetches OpenCode Go usage from the Zen usage API with a Go API key. The key
// is workspace-bound, so no workspace id, session cookie, or org scoping is
// needed. Verified live 2026-10-11.
async fn fetch_ocg_zen_usage(api_key: &str) -> Result<serde_json::Value, String> {
    let resp = http_client()
        .get(OCG_ZEN_GO_USAGE_URL)
        .header("User-Agent", OCG_USER_AGENT)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(OCG_FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("OpenCode Go request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return match status.as_u16() {
            401 | 403 => Err(format!(
                "HTTP {status} — OpenCode Go API key rejected; copy it from the console API Keys page or your ~/.local/share/opencode/auth.json (opencode-go entry)"
            )),
            429 | 500..=599 => Err(format!("HTTP {status} (transient)")),
            _ => Err(format!("HTTP {status} from the OpenCode Go API")),
        };
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("OpenCode Go: reading response: {e}"))?;
    let data: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("OpenCode Go: invalid API response: {e}"))?;

    let limits = ocg_limits_from_zen_usage(&data)?;
    Ok(ocg_limits_response(limits))
}

// Maps the Zen `go/v1/usage` payload onto SubBar's internal limit shape.
// Each window is `{ status, percent, resetsAt }` where `percent` is the used
// percentage (0–100). All three windows are required: the UI renders three
// pills and missing data must surface as an error, never as fake zeros.
fn ocg_limits_from_zen_usage(
    v: &serde_json::Value,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let usage = v
        .get("usage")
        .and_then(|u| u.as_object())
        .ok_or_else(|| "OpenCode Go: no usage windows in the API response".to_string())?;
    let now = Utc::now();

    let mut limits = serde_json::Map::new();
    for (window_name, key) in [
        ("rolling", "five_hour"),
        ("weekly", "seven_day"),
        ("monthly", "monthly"),
    ] {
        let window = usage
            .get(window_name)
            .and_then(|w| w.as_object())
            .ok_or_else(|| format!("OpenCode Go: missing {window_name} window"))?;
        // SEC: only numbers flow into the UI; missing/invalid fields error out
        // instead of silently becoming 0.
        let used_pct = window
            .get("percent")
            .and_then(|p| p.as_f64())
            .ok_or_else(|| format!("OpenCode Go: missing {window_name}.percent"))?;
        let resets_at = window
            .get("resetsAt")
            .and_then(|r| r.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| now.to_rfc3339());
        limits.insert(
            key.to_string(),
            serde_json::json!({ "used_percent": used_pct, "resets_at": resets_at }),
        );
    }
    Ok(limits)
}

// Resolves the workspace id from the console account when the user has not
// entered one (the console only surfaces the id inside its URLs, so
// auto-discovery keeps setup down to the single cookie field).
async fn ocg_discover_workspace_id(cookie: &str) -> Result<String, String> {
    let resp = http_client()
        .get(OCG_CONSOLE_ORGS_URL)
        .header("User-Agent", OCG_USER_AGENT)
        .header("Cookie", ocg_cookie_header(cookie))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(OCG_FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("OpenCode Go request failed: {e}"))?;
    match resp.status().as_u16() {
        401 | 403 => {
            return Err(
                "HTTP 401 — OpenCode Go session cookie rejected; copy the __Host-console_session cookie (DevTools → Application → Cookies → opencode.ai)"
                    .into(),
            );
        }
        s if !(200..300).contains(&s) => {
            return Err(format!("HTTP {s} from the OpenCode console API"));
        }
        _ => {}
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("OpenCode Go: reading response: {e}"))?;
    let orgs: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("OpenCode Go: invalid API response: {e}"))?;
    orgs.as_array()
        .and_then(|list| list.iter().find_map(|o| o.get("id").and_then(|v| v.as_str())))
        .map(|id| id.to_string())
        .ok_or_else(|| "OpenCode Go: no workspace found for this account".into())
}

// Maps the console `go/status` payload onto SubBar's internal limit shape.
// Meter amounts are micro-cent strings (`"300000000"`) or numbers; the month
// meter carries no `resetsAt`, so the billing period end (`access.endsAt`)
// stands in for its reset time.
fn ocg_limits_from_console(
    v: &serde_json::Value,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let access = v
        .get("access")
        .filter(|a| a.is_object())
        .ok_or_else(|| "OpenCode Go: no active subscription on this workspace".to_string())?;
    let meters = access
        .get("meters")
        .and_then(|m| m.as_object())
        .ok_or_else(|| "OpenCode Go: no usage meters on this workspace".to_string())?;
    let period_end = access.get("endsAt").and_then(|e| e.as_str()).unwrap_or("");
    let now = Utc::now();

    let mut limits = serde_json::Map::new();
    for (meter_name, key) in [
        ("fiveHour", "five_hour"),
        ("week", "seven_day"),
        ("month", "monthly"),
    ] {
        let meter = meters
            .get(meter_name)
            .and_then(|m| m.as_object())
            .ok_or_else(|| format!("OpenCode Go: missing {meter_name} meter"))?;
        let used = ocg_micro_cents(meter.get("usedMicroCents"));
        let limit = ocg_micro_cents(meter.get("limitMicroCents"));
        let used_pct = if limit > 0.0 {
            (used / limit) * 100.0
        } else {
            0.0
        };
        let resets_at = meter
            .get("resetsAt")
            .and_then(|r| r.as_str())
            .filter(|s| !s.is_empty())
            .or(if meter_name == "month" && !period_end.is_empty() {
                Some(period_end)
            } else {
                None
            })
            .map(|s| s.to_string())
            .unwrap_or_else(|| now.to_rfc3339());
        limits.insert(
            key.to_string(),
            serde_json::json!({ "used_percent": used_pct, "resets_at": resets_at }),
        );
    }
    Ok(limits)
}

// Console amounts arrive as micro-cent strings (or numbers); anything else is 0.
fn ocg_micro_cents(v: Option<&serde_json::Value>) -> f64 {
    v.and_then(|x| {
        x.as_f64()
            .or_else(|| x.as_str().and_then(|s| s.parse::<f64>().ok()))
    })
    .unwrap_or(0.0)
}

fn parse_ocg_bars(v: &serde_json::Value) -> Result<Vec<serde_json::Value>, String> {
    let limits = v
        .get("providers")
        .and_then(|p| p.get("opencodego"))
        .and_then(|o| o.get("limits"))
        .ok_or_else(|| "OpenCode Go: missing providers.opencodego.limits".to_string())?;
    let mut bars = Vec::new();
    for (id, key) in [("5h", "five_hour"), ("week", "seven_day"), ("month", "monthly")] {
        let entry = limits
            .get(key)
            .ok_or_else(|| format!("OpenCode Go: missing limits.{}", key))?;
        let used = entry.get("used_percent").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let reset = entry
            .get("resets_at")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        bars.push(serde_json::json!({ "id": id, "used_percent": used, "reset_at": reset }));
    }
    Ok(bars)
}

#[tauri::command]
async fn fetch_ocg_quota(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    // Mirror fetch_quota: update the tray title as well as returning the bars,
    // so switching to ocg refreshes the menubar immediately. Credentials are read
    // from AppState, which `set_ocg_credentials` keeps current when the user saves.
    fetch_ocg_and_update(&app).await
}

// Open an external URL in the user's default browser. Only HTTPS URLs are
// allowed to prevent abuse via file://, ssh://, or other custom protocol handlers.
#[tauri::command]
fn open_external(url: String) {
    if !url.starts_with("https://") {
        warn!("open_external: rejected non-https URL");
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", &url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
}

async fn fetch_ocg_and_update(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let v = match run_agent_limits(app).await {
        Ok(d) => d,
        Err(first) => {
            // Transient failure (e.g. a momentary network blip): retry once
            // before surfacing the no-key / unauth status.
            time::sleep(Duration::from_millis(400)).await;
            match run_agent_limits(app).await {
                Ok(d) => d,
                Err(second) => {
                    log::warn!("ocg fetch failed twice: {first} | {second}");
                    render_title(app, OCG_NO_AUTH_TITLE);
                    return Err(second);
                }
            }
        }
    };
    let bars = match parse_ocg_bars(&v) {
        Ok(b) => b,
        Err(e) => {
            render_title(app, OCG_NO_AUTH_TITLE);
            return Err(e);
        }
    };
    let mut u5 = 0u32;
    let mut uw = 0u32;
    let mut um = 0u32;
    for bar in &bars {
        let pct = (bar
            .get("used_percent")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0))
            .round() as u32;
        match bar.get("id").and_then(|x| x.as_str()) {
            Some("5h") => u5 = pct,
            Some("week") => uw = pct,
            Some("month") => um = pct,
            _ => {}
        }
    }
    {
        let s = app.state::<AppState>();
        *s.last_used_5h.lock().unwrap_or_else(|e| e.into_inner()) = u5;
        *s.last_used_week.lock().unwrap_or_else(|e| e.into_inner()) = uw;
        *s.last_used_month.lock().unwrap_or_else(|e| e.into_inner()) = um;
    }
    // Success: show the three ocg limits (5h / week / month used percentages).
    render_title(app, &format_ocg_title(u5, uw, um));
    Ok(serde_json::json!({ "bars": bars }))
}

fn format_ocg_title(u5: u32, uw: u32, um: u32) -> String {
    format!("{}% {}% {}%", u5, uw, um)
}

// Dropdown placement math, shared by the tray-click handler and the launch
// path so the window first appears exactly where it will live (horizontally
// centered on the tray item, top just below it) instead of flashing at the
// default centered position. Tray rect values are physical pixels; `win_w` is
// the window width in physical pixels.
fn compute_dropdown_xy(
    tray_pos: tauri::PhysicalPosition<i32>,
    tray_size: tauri::PhysicalSize<u32>,
    win_w: f64,
    screen_w: f64,
    y_gap: f64,
) -> tauri::PhysicalPosition<f64> {
    let mut x = tray_pos.x as f64 + tray_size.width as f64 / 2.0 - win_w / 2.0;
    x = x.clamp(0.0, (screen_w - win_w).max(0.0));
    let y = tray_pos.y as f64 + tray_size.height as f64 + y_gap;
    tauri::PhysicalPosition::new(x, y)
}

fn compute_dropdown_position(
    tray_rect: &tauri::Rect,
    window: &tauri::WebviewWindow,
) -> tauri::PhysicalPosition<f64> {
    let pos = match tray_rect.position {
        tauri::Position::Physical(p) => p,
        _ => {
            warn!("Tray returned non-physical position");
            return window
                .outer_position()
                .map(|p| tauri::PhysicalPosition::new(p.x as f64, p.y as f64))
                .unwrap_or(tauri::PhysicalPosition::new(0.0, 0.0));
        }
    };
    let sz = match tray_rect.size {
        tauri::Size::Physical(s) => s,
        _ => {
            warn!("Tray returned non-physical size");
            return window
                .outer_position()
                .map(|p| tauri::PhysicalPosition::new(p.x as f64, p.y as f64))
                .unwrap_or(tauri::PhysicalPosition::new(0.0, 0.0));
        }
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    // Use the window's real on-screen width (physical px) so the panel is
    // truly centered on the tray item regardless of the configured inner
    // size; the constant is only a fallback if the size cannot be read.
    let win_w = window
        .outer_size()
        .map(|s| s.width as f64)
        .unwrap_or(WIDGET_OUTER_WIDTH * scale);
    let screen_w = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.size().width as f64)
        .unwrap_or(win_w);
    compute_dropdown_xy(pos, sz, win_w, screen_w, 4.0 * scale)
}

fn apply_liquid_glass(app: &tauri::AppHandle, window: &tauri::WebviewWindow) {
    let glass = app.liquid_glass();
    if !glass.is_supported() {
        warn!("Liquid Glass not supported on this platform");
        return;
    }
    let result = glass.set_effect(
        window,
        LiquidGlassConfig {
            corner_radius: 24.0,
            tint_color: Some("#ffffff15".into()),
            variant: GlassMaterialVariant::Sidebar,
            ..Default::default()
        },
    );
    match result {
        Ok(()) => info!("Liquid Glass applied successfully"),
        Err(e) => error!("Failed to apply Liquid Glass: {:?}", e),
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_liquid_glass::init())
        .manage(AppState {
            api_key_com: Mutex::new(String::new()),
            api_key_io: Mutex::new(String::new()),
            ocg_workspace_id: Mutex::new(String::new()),
            ocg_auth_cookie: Mutex::new(String::new()),
            endpoint: Mutex::new(String::new()),
                refresh_interval_secs: Mutex::new(300),
                last_used_5h: Mutex::new(0),
                last_used_week: Mutex::new(0),
                last_used_month: Mutex::new(0),
            })
    .invoke_handler(tauri::generate_handler![
        get_app_version,
        get_api_key,
        set_api_key,
        get_ocg_credentials,
        set_ocg_credentials,
        set_endpoint,
        fetch_quota,
        fetch_ocg_quota,
        open_external,
        quit_app,
        set_refresh_interval,
    ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            if let Err(e) = app.handle().set_dock_visibility(false) {
                warn!("Failed to hide dock icon: {e}");
            }

            let handle = app.handle().clone();

            // Migrate legacy config.json to keychain if present
            let app_data_dir = app
                .path()
                .app_local_data_dir()
                .map(|d| d.join("subbar"))
                .unwrap_or_else(|_| PathBuf::from("subbar"));
            migrate_config_json_if_needed(&app_data_dir);

            // Load keys from keychain
            let (s_com, u_com) = com_keyring();
            let (s_io, u_io) = io_keyring();
            let (s_ws, u_ws) = ocg_ws_keyring();
            let (s_ck, u_ck) = ocg_cookie_keyring();
            let com_key = load_key_from_keyring(s_com, u_com);
            let io_key = load_key_from_keyring(s_io, u_io);
            let ocg_ws = load_key_from_keyring(s_ws, u_ws);
            let ocg_ck = load_key_from_keyring(s_ck, u_ck);


            {
                let state = handle.state::<AppState>();
                *state.api_key_com.lock().unwrap_or_else(|e| e.into_inner()) = com_key;
                *state.api_key_io.lock().unwrap_or_else(|e| e.into_inner()) = io_key;
                *state.ocg_workspace_id.lock().unwrap_or_else(|e| e.into_inner()) = ocg_ws;
                *state.ocg_auth_cookie.lock().unwrap_or_else(|e| e.into_inner()) = ocg_ck;
                // Endpoint restored from the persisted value (last synced by the
                // frontend) so the backend starts on the right data source and
                // never flashes another endpoint's state.
                *state.endpoint.lock().unwrap_or_else(|e| e.into_inner()) =
                    load_persisted_endpoint(&app_data_dir);
                // Default refresh interval 300s (5 min)
                *state
                    .refresh_interval_secs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = 300;
            }

            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
                .expect("failed to load tray icon");

            let _tray = tauri::tray::TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .title("--")
                .build(app)?;

            let window = tauri::WebviewWindowBuilder::new(
                &handle,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("SubBar")
            .inner_size(WIDGET_INNER_WIDTH, WIDGET_INNER_HEIGHT)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .visible(false)
            .always_on_top(true)
            .build()
            .expect("failed to create window");

            #[cfg(target_os = "macos")]
            {
                use objc::msg_send;
                use objc::sel;
                use objc::sel_impl;

                if let Ok(ns_win) = window.ns_window() {
                    unsafe {
                        let obj = ns_win as *mut objc::runtime::Object;
                        let _: () = msg_send![obj, setLevel: 25_i64];
                        let _: () = msg_send![obj, setCollectionBehavior: 257_u64];
                    }
                }
            }

            apply_liquid_glass(&handle, &window);

            // Place the dropdown under the tray icon right from launch so it
            // never flashes at the default centered position. The status item
            // may not be laid out yet at startup, so if the tray has no rect
            // yet, poll briefly and only show the window once it is positioned
            // (with an always-show fallback so the app is never left hidden).
            {
                let placement_tray = handle.tray_by_id("main-tray");
                let placement_window = window.clone();
                tauri::async_runtime::spawn(async move {
                    let placed = match placement_tray
                        .as_ref()
                        .and_then(|t| t.rect().ok().flatten())
                    {
                        Some(rect) => {
                            let has_size = match rect.size {
                                tauri::Size::Physical(s) => s.width > 0 && s.height > 0,
                                _ => false,
                            };
                            has_size
                                && placement_window
                                    .set_position(compute_dropdown_position(
                                        &rect,
                                        &placement_window,
                                    ))
                                    .is_ok()
                        }
                        None => false,
                    };
                    if !placed {
                        let Some(tray) = placement_tray else {
                            let _ = placement_window.show();
                            return;
                        };
                        for _ in 0..60 {
                            time::sleep(Duration::from_millis(100)).await;
                            if let Ok(Some(rect)) = tray.rect() {
                                let has_size = match rect.size {
                                    tauri::Size::Physical(s) => s.width > 0 && s.height > 0,
                                    _ => false,
                                };
                                if has_size {
                                    let pos =
                                        compute_dropdown_position(&rect, &placement_window);
                                    if placement_window.set_position(pos).is_ok() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if let Err(e) = placement_window.show() {
                        error!("Failed to show window at launch: {e}");
                    }
                    if let Err(e) = placement_window.set_focus() {
                        warn!("Failed to focus window at launch: {e}");
                    }
                });
            }

            {
                let handle_clone = handle.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        if let Some(win) = handle_clone.get_webview_window("main") {
                            let _ = win.hide();
                        }
                    }
                });
            }

            let handle_clone = handle.clone();
            if let Some(tray) = handle.tray_by_id("main-tray") {
                tray.on_tray_icon_event(move |_tray, event| {
                    // Open the panel on any click (left or right) so the tray
                    // icon is usable for users who expect right-click to open it.
                    if let tauri::tray::TrayIconEvent::Click {
                        button:
                            tauri::tray::MouseButton::Left | tauri::tray::MouseButton::Right,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let tray_rect = _tray.rect().ok().flatten();
                        let app = handle_clone.clone();
                        tauri::async_runtime::spawn(async move {
                            // Fetch quota immediately on tray click so menubar shows fresh data
                            let _ = fetch_and_update(&app).await;

                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else if let Some(rect) = tray_rect {
                                    let pos = compute_dropdown_position(&rect, &window);
                                    if let Err(e) = window.set_position(pos) {
                                        error!("Failed to set window position: {e}");
                                    }
                                    apply_liquid_glass(&app, &window);
                                    if let Err(e) = window.show() {
                                        error!("Failed to show window: {e}");
                                    }
                                    if let Err(e) = window.set_focus() {
                                        warn!("Failed to focus window: {e}");
                                    }
                                }
                            }
                        });
                    }
                });
            }

            let handle_for_fetch = handle.clone();
            tauri::async_runtime::spawn(async move {
                match fetch_and_update(&handle_for_fetch).await {
                    Ok(_) => info!("Initial fetch succeeded"),
                    Err(e) => info!("Initial fetch (expected if no API key): {e}"),
                }
            });

            // Background timer: fetch-first so data appears immediately on startup,
            // then sleep for the configured interval.  During sleep, poll every 1s
            // for interval changes so the next fetch promptly uses the new cadence.
            let handle_for_timer = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let _ = fetch_and_update(&handle_for_timer).await;
                    let secs = {
                        let state = handle_for_timer.state::<AppState>();
                        let v = *state
                            .refresh_interval_secs
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        v
                    };
                    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
                    loop {
                        let remaining =
                            deadline.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        let check = remaining.min(Duration::from_secs(1));
                        time::sleep(check).await;
                        let current = {
                            let state = handle_for_timer.state::<AppState>();
                            let v = *state
                                .refresh_interval_secs
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            v
                        };
                        // If interval changed, stop waiting and re-loop with the new value
                        if current != secs {
                            break;
                        }
                    }
                }
            });

            info!("Setup complete");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_ocg_usage_no_cookie_is_clear_error() {
        let res = fetch_ocg_usage("wrk_TEST0000000000000000000000", "").await;
        let err = res.expect_err("empty credential must error, never fetch");
        assert!(
            err.contains("credential missing"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn fetch_ocg_usage_blank_space_cookie_is_clear_error() {
        // A whitespace-only credential must error before any network call,
        // even though an empty workspace id would otherwise trigger discovery.
        let res = fetch_ocg_usage("   ", "  ").await;
        let err = res.expect_err("whitespace credential must error");
        assert!(
            err.contains("credential missing"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn fetch_ocg_usage_wrong_creds_errors() {
        // Bogus cookie against a real-looking workspace: the console API
        // answers 401/404; in all cases the call must surface an Err, never
        // fake usage data.
        let res = fetch_ocg_usage(
            "wrk_TEST0000000000000000000000",
            "Fe26.2**definitely-invalid-session-cookie",
        )
        .await;
        assert!(res.is_err(), "wrong creds must not return usage");
    }

    #[tokio::test]
    async fn fetch_ocg_usage_valid_creds_returns_three_windows() {
        // Requires OCG_TEST_WS / OCG_TEST_CK env vars (real creds, not committed).
        let Ok(ws) = std::env::var("OCG_TEST_WS") else {
            eprintln!("skipping: OCG_TEST_WS not set");
            return;
        };
        let Ok(ck) = std::env::var("OCG_TEST_CK") else {
            eprintln!("skipping: OCG_TEST_CK not set");
            return;
        };
        let v = fetch_ocg_usage(&ws, &ck).await.expect("valid creds fetch");
        let limits = v
            .get("providers")
            .and_then(|p| p.get("opencodego"))
            .and_then(|o| o.get("limits"))
            .and_then(|l| l.as_object())
            .expect("limits object");
        for key in ["five_hour", "seven_day", "monthly"] {
            assert!(limits.contains_key(key), "missing window {key}");
            let used = limits[key].get("used_percent").and_then(|x| x.as_f64());
            assert!(used.is_some(), "{key} used_percent");
            let reset = limits[key].get("resets_at").and_then(|x| x.as_str());
            assert!(reset.is_some() && !reset.unwrap().is_empty(), "{key} resets_at");
        }
    }

    #[test]
    fn ocg_cookie_header_bare_value_sent_under_every_session_name() {
        // One pasted value must authenticate regardless of which session
        // cookie name the user's browser actually holds.
        assert_eq!(
            ocg_cookie_header("abc123"),
            "auth=abc123; __Host-auth=abc123; __Host-console_session=abc123"
        );
    }

    #[test]
    fn ocg_cookie_header_full_header_passes_through() {
        // A pasted `name=value` header is forwarded verbatim so a user who
        // copies several cookies keeps them all.
        assert_eq!(
            ocg_cookie_header("auth=abc; other=xyz"),
            "auth=abc; other=xyz"
        );
    }

    #[test]
    fn ocg_workspace_id_accepts_bare_id_and_pasted_urls() {
        let id = "wrk_01KYSGBE8P7RYN8SP1YBDCZQHY";
        assert_eq!(ocg_workspace_id(id), id);
        assert_eq!(
            ocg_workspace_id(&format!("https://opencode.ai/console/{id}/go")),
            id
        );
        assert_eq!(
            ocg_workspace_id(&format!("https://opencode.ai/workspace/{id}/go")),
            id
        );
        assert_eq!(ocg_workspace_id(&format!("opencode.ai/console/{id}/")), id);
        assert_eq!(ocg_workspace_id(&format!("  {id}  ")), id);
        // A typo is passed through untouched so the API can reject it clearly.
        assert_eq!(ocg_workspace_id("not-a-workspace"), "not-a-workspace");
    }

    #[test]
    fn ocg_token_kind_routes_api_key_to_zen_and_rest_to_console() {
        assert_eq!(ocg_token_kind("sk-abc123"), OcgTokenKind::ApiKey);
        assert_eq!(ocg_token_kind("  sk-abc123  "), OcgTokenKind::ApiKey);
        assert_eq!(ocg_token_kind("Bearer sk-abc123"), OcgTokenKind::ApiKey);
        assert_eq!(
            ocg_token_kind("Fe26.2**definitely-a-session-cookie"),
            OcgTokenKind::SessionCookie
        );
        assert_eq!(
            ocg_token_kind("auth=abc; other=xyz"),
            OcgTokenKind::SessionCookie
        );
        assert_eq!(ocg_bearer_token("Bearer sk-abc123"), "sk-abc123");
        assert_eq!(ocg_bearer_token("sk-abc123"), "sk-abc123");
    }

    #[test]
    fn ocg_limits_from_zen_usage_maps_windows_to_pills() {
        // Mirrors the live `zen/go/v1/usage` payload (verified 2026-10-11).
        let v = serde_json::json!({
            "usage": {
                "rolling": { "status": "ok", "percent": 4, "resetsAt": "2026-10-11T17:08:01.000Z" },
                "weekly": { "status": "ok", "percent": 48, "resetsAt": "2026-10-12T00:00:00.000Z" },
                "monthly": { "status": "ok", "percent": 24, "resetsAt": "2026-11-05T00:55:32.000Z" }
            }
        });
        let limits = ocg_limits_from_zen_usage(&v).expect("limits");
        let pct = |k: &str| limits[k].get("used_percent").and_then(|x| x.as_f64()).unwrap();
        assert_eq!(pct("five_hour"), 4.0);
        assert_eq!(pct("seven_day"), 48.0);
        assert_eq!(pct("monthly"), 24.0);
        assert_eq!(
            limits["monthly"].get("resets_at").and_then(|x| x.as_str()),
            Some("2026-11-05T00:55:32.000Z")
        );
        // The mapped shape must feed `parse_ocg_bars` unchanged.
        let wrapped = serde_json::json!({ "providers": { "opencodego": { "limits": limits } } });
        let bars = parse_ocg_bars(&wrapped).expect("bars");
        assert_eq!(bars.len(), 3);
        let pcts: Vec<f64> = bars
            .iter()
            .filter_map(|b| b.get("used_percent").and_then(|x| x.as_f64()))
            .collect();
        assert_eq!(pcts, vec![4.0, 48.0, 24.0]);
    }

    #[test]
    fn ocg_limits_from_zen_usage_rejects_incomplete_payloads() {
        let no_usage = serde_json::json!({});
        let err = ocg_limits_from_zen_usage(&no_usage).expect_err("must error");
        assert!(err.contains("no usage windows"), "unexpected error: {err}");

        let missing_month = serde_json::json!({
            "usage": {
                "rolling": { "status": "ok", "percent": 4, "resetsAt": "x" },
                "weekly": { "status": "ok", "percent": 48, "resetsAt": "x" }
            }
        });
        let err = ocg_limits_from_zen_usage(&missing_month).expect_err("must error");
        assert!(err.contains("missing monthly window"), "unexpected error: {err}");

        let missing_percent = serde_json::json!({
            "usage": {
                "rolling": { "status": "ok" },
                "weekly": { "status": "ok", "percent": 48, "resetsAt": "x" },
                "monthly": { "status": "ok", "percent": 24, "resetsAt": "x" }
            }
        });
        let err = ocg_limits_from_zen_usage(&missing_percent).expect_err("must error");
        assert!(err.contains("missing rolling.percent"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn fetch_ocg_zen_usage_bogus_key_errors() {
        // A syntactically valid but unknown Go API key must surface an Err
        // (401), never fake usage data.
        let res = fetch_ocg_usage("", "sk-bogus-test-key-0000000000000").await;
        assert!(res.is_err(), "bogus API key must not return usage");
    }

    #[tokio::test]
    async fn fetch_ocg_zen_usage_valid_key_returns_three_windows() {
        // Requires OCG_TEST_APIKEY env var (real Go API key, not committed).
        let Ok(key) = std::env::var("OCG_TEST_APIKEY") else {
            eprintln!("skipping: OCG_TEST_APIKEY not set");
            return;
        };
        let v = fetch_ocg_usage("", &key).await.expect("valid key fetch");
        let limits = v
            .get("providers")
            .and_then(|p| p.get("opencodego"))
            .and_then(|o| o.get("limits"))
            .and_then(|l| l.as_object())
            .expect("limits object");
        for k in ["five_hour", "seven_day", "monthly"] {
            assert!(limits.contains_key(k), "missing window {k}");
            let used = limits[k].get("used_percent").and_then(|x| x.as_f64());
            assert!(used.is_some(), "{k} used_percent");
            let reset = limits[k].get("resets_at").and_then(|x| x.as_str());
            assert!(reset.is_some() && !reset.unwrap().is_empty(), "{k} resets_at");
        }
    }

    #[test]
    fn ocg_limits_from_console_maps_meters_to_windows() {
        // Mirrors the live console `go/status` payload: micro-cent strings,
        // month meter without `resetsAt` (billing period end stands in).
        let v = serde_json::json!({
            "access": {
                "startsAt": "2026-09-19T00:00:00.000Z",
                "endsAt": "2026-10-19T00:00:00.000Z",
                "meters": {
                    "fiveHour": {
                        "startsAt": "2026-09-19T23:00:00.000Z",
                        "resetsAt": "2026-09-20T03:00:00.000Z",
                        "limitMicroCents": "1200000000",
                        "usedMicroCents": "300000000"
                    },
                    "week": {
                        "startsAt": "2026-09-14T00:00:00.000Z",
                        "resetsAt": "2026-09-21T00:00:00.000Z",
                        "limitMicroCents": "3000000000",
                        "usedMicroCents": "1200000000"
                    },
                    "month": {
                        "limitMicroCents": "6000000000",
                        "usedMicroCents": "600000000"
                    }
                }
            }
        });
        let limits = ocg_limits_from_console(&v).expect("limits");
        let pct = |k: &str| limits[k].get("used_percent").and_then(|x| x.as_f64()).unwrap();
        assert!((pct("five_hour") - 25.0).abs() < 1e-9);
        assert!((pct("seven_day") - 40.0).abs() < 1e-9);
        assert!((pct("monthly") - 10.0).abs() < 1e-9);
        assert_eq!(
            limits["monthly"].get("resets_at").and_then(|x| x.as_str()),
            Some("2026-10-19T00:00:00.000Z"),
            "month meter falls back to the billing period end"
        );
        assert_eq!(
            limits["five_hour"].get("resets_at").and_then(|x| x.as_str()),
            Some("2026-09-20T03:00:00.000Z")
        );
        // The mapped shape must feed `parse_ocg_bars` unchanged.
        let wrapped = serde_json::json!({ "providers": { "opencodego": { "limits": limits } } });
        assert_eq!(parse_ocg_bars(&wrapped).expect("bars").len(), 3);
    }

    #[test]
    fn ocg_limits_from_console_numeric_amounts_and_zero_limit() {
        let v = serde_json::json!({
            "access": {
                "endsAt": "2026-10-19T00:00:00.000Z",
                "meters": {
                    "fiveHour": { "limitMicroCents": 0, "usedMicroCents": 0 },
                    "week": { "limitMicroCents": 100, "usedMicroCents": 25 },
                    "month": { "limitMicroCents": 100, "usedMicroCents": 50 }
                }
            }
        });
        let limits = ocg_limits_from_console(&v).expect("limits");
        let pct = |k: &str| limits[k].get("used_percent").and_then(|x| x.as_f64()).unwrap();
        assert_eq!(pct("five_hour"), 0.0, "zero limit must not divide by zero");
        assert_eq!(pct("seven_day"), 25.0);
        assert_eq!(pct("monthly"), 50.0);
    }

    #[test]
    fn ocg_limits_from_console_rejects_missing_subscription() {
        let no_access = serde_json::json!({ "access": null });
        let err = ocg_limits_from_console(&no_access).expect_err("must error");
        assert!(err.contains("no active subscription"), "unexpected error: {err}");

        let no_meters = serde_json::json!({ "access": { "endsAt": "x" } });
        let err = ocg_limits_from_console(&no_meters).expect_err("must error");
        assert!(err.contains("no usage meters"), "unexpected error: {err}");

        let missing_meter = serde_json::json!({
            "access": { "endsAt": "x", "meters": { "fiveHour": {}, "week": {} } }
        });
        let err = ocg_limits_from_console(&missing_meter).expect_err("must error");
        assert!(err.contains("missing month meter"), "unexpected error: {err}");
    }

    #[test]
    fn ocg_title_formats_three_limits() {
        assert_eq!(format_ocg_title(0, 0, 0), "0% 0% 0%");
        assert_eq!(format_ocg_title(12, 34, 56), "12% 34% 56%");
        assert_eq!(format_ocg_title(100, 100, 100), "100% 100% 100%");
    }

    #[test]
    fn ocg_no_auth_title_is_clear_status() {
        // The tray must show a clear no-key / unauth status, not a raw error.
        assert_eq!(OCG_NO_AUTH_TITLE, "unauth");
    }

    #[test]
    fn endpoint_normalization_accepts_only_known_endpoints() {
        assert_eq!(normalize_endpoint("ocg"), Some("ocg".to_string()));
        assert_eq!(normalize_endpoint("com"), Some("com".to_string()));
        assert_eq!(normalize_endpoint("io"), Some("io".to_string()));
        assert_eq!(normalize_endpoint("OCG"), Some("ocg".to_string()));
        assert_eq!(normalize_endpoint("bogus"), None);
        assert_eq!(normalize_endpoint(""), None);
    }

    #[test]
    fn keyring_mapping_uses_subbar_names() {
        // Minimax keys live under subbar-named keychain items.
        assert_eq!(com_keyring(), ("subbar-minimax", "subbar"));
        assert_eq!(io_keyring(), ("subbar-minimaxi", "subbar"));
        assert_eq!(endpoint_keyring("com"), Some(("subbar-minimax", "subbar")));
        assert_eq!(endpoint_keyring("io"), Some(("subbar-minimaxi", "subbar")));
        assert_eq!(endpoint_keyring("ocg"), None);
    }

    #[test]
    fn select_api_key_routes_com_io_ocg() {
        // get_api_key must read the key for the endpoint the frontend is on, so
        // .com and .io behave identically and independently.
        assert_eq!(select_api_key("com", "comkey", "iokey"), "comkey");
        assert_eq!(select_api_key("io", "comkey", "iokey"), "iokey");
        assert_eq!(select_api_key("ocg", "comkey", "iokey"), "");
        assert_eq!(select_api_key("unknown", "comkey", "iokey"), "comkey");
    }

    #[test]
    fn ocg_keyring_items_use_subbar_names() {
        // OpenCode Go credentials live in their own keychain items so they never
        // collide with the Minimax keys.
        assert_eq!(ocg_ws_keyring(), ("subbar-ocg-ws", "subbar"));
        assert_eq!(ocg_cookie_keyring(), ("subbar-ocg-cookie", "subbar"));
    }

    #[test]
    fn endpoint_persistence_round_trips_normalized() {
        let dir = std::env::temp_dir().join(format!("mm-endpoint-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // No persisted value yet -> default to com.
        assert_eq!(load_persisted_endpoint(&dir), "com");
        save_persisted_endpoint(&dir, "ocg");
        assert_eq!(load_persisted_endpoint(&dir), "ocg");
        // Persisted values are normalized on load.
        save_persisted_endpoint(&dir, "OCG");
        assert_eq!(load_persisted_endpoint(&dir), "ocg");
        // Corrupt content falls back to the default.
        fs::write(persisted_endpoint_path(&dir), "???").expect("write corrupt");
        assert_eq!(load_persisted_endpoint(&dir), "com");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ocg_endpoint_dispatches_away_from_minimax() {
        // The tray "AUTH!" marker only exists on the Minimax path. When the
        // synced endpoint is ocg, the dispatcher MUST route to the ocg path so
        // the menubar never shows Minimax auth states for ocg.
        assert!(is_ocg_endpoint("ocg"));
        assert!(!is_ocg_endpoint("com"));
        assert!(!is_ocg_endpoint("io"));
    }

    // The console `go/status` mapping produces the same JSON shape the UI
    // parser expects. This proves the in-process fetch integrates with
    // `parse_ocg_bars`.
    #[test]
    fn parse_ocg_bars_reads_vendored_shape() {
        let v = serde_json::json!({
            "providers": {
                "opencodego": {
                    "limits": {
                        "five_hour": { "used_percent": 12.5, "resets_at": "2026-08-24T12:00:00+00:00" },
                        "seven_day": { "used_percent": 42.0, "resets_at": "2026-08-30T12:00:00+00:00" },
                        "monthly":   { "used_percent": 78.0, "resets_at": "2026-08-31T12:00:00+00:00" }
                    }
                }
            }
        });
        let bars = parse_ocg_bars(&v).expect("bars should parse");
        assert_eq!(bars.len(), 3);
        let ids: Vec<&str> = bars
            .iter()
            .filter_map(|b| b.get("id").and_then(|x| x.as_str()))
            .collect();
        assert_eq!(ids, vec!["5h", "week", "month"]);
        for b in bars {
            let used = b
                .get("used_percent")
                .and_then(|x| x.as_f64())
                .expect("used_percent should be a number");
            assert!(
                (0.0..=100.0).contains(&used),
                "used_percent {} out of range",
                used
            );
            let reset = b.get("reset_at").and_then(|x| x.as_str()).unwrap_or("");
            assert!(!reset.is_empty(), "reset_at should be present");
        }
    }

    #[test]
    fn dropdown_xy_centers_on_tray_and_sits_below_it() {
        // 2x Retina: tray at x=1800 (width 24, height 24, top y=30); window
        // 212 logical px -> 424 physical px. The window's horizontal center
        // should align with the tray's center and the top should sit just
        // below the tray.
        let pos = compute_dropdown_xy(
            tauri::PhysicalPosition::new(1800, 30),
            tauri::PhysicalSize::new(24, 24),
            424.0,
            3024.0,
            8.0,
        );
        assert_eq!(pos.x, 1800.0 + 24.0 / 2.0 - 424.0 / 2.0);
        assert_eq!(pos.y, 30.0 + 24.0 + 8.0);
        // Window center == tray center on the same vertical line.
        let win_center = pos.x + 424.0 / 2.0;
        let tray_center = 1800.0 + 24.0 / 2.0;
        assert!((win_center - tray_center).abs() < f64::EPSILON);
    }

    #[test]
    fn dropdown_xy_clamps_to_screen_edges() {
        // Tray near the left edge: the window centered on the tray would
        // overshoot the left screen edge, so x must clamp to 0.
        let pos = compute_dropdown_xy(
            tauri::PhysicalPosition::new(0, 24),
            tauri::PhysicalSize::new(24, 24),
            424.0,
            3024.0,
            8.0,
        );
        assert_eq!(pos.x, 0.0);
        // Tray near the right edge: the window must not extend past the screen.
        let pos = compute_dropdown_xy(
            tauri::PhysicalPosition::new(3024 - 24, 24),
            tauri::PhysicalSize::new(24, 24),
            424.0,
            3024.0,
            8.0,
        );
        assert_eq!(pos.x, 3024.0 - 424.0);
    }

    #[test]
    fn dropdown_xy_never_goes_negative_when_window_wider_than_screen() {
        let pos = compute_dropdown_xy(
            tauri::PhysicalPosition::new(0, 24),
            tauri::PhysicalSize::new(24, 24),
            500.0,
            400.0,
            8.0,
        );
        assert_eq!(pos.x, 0.0);
    }
}

#[test]
fn ocg_keyring_round_trip() {
    let (ws_s, ws_u) = ocg_ws_keyring();
    let (c_s, c_u) = ocg_cookie_keyring();
    save_key_to_keyring(ws_s, ws_u, "wstest123");
    save_key_to_keyring(c_s, c_u, "cookietest456");
    let loaded_ws = load_key_from_keyring(ws_s, ws_u);
    let loaded_ck = load_key_from_keyring(c_s, c_u);
    assert_eq!(loaded_ws, "wstest123", "ocg ws keychain write failed");
    assert_eq!(loaded_ck, "cookietest456", "ocg cookie keychain write failed");
    // cleanup
    save_key_to_keyring(ws_s, ws_u, "");
    save_key_to_keyring(c_s, c_u, "");
}
