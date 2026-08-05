#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
const KEYRING_SERVICE: &str = "pre-rebrand";
const KEYRING_USER_COM: &str = "api_key_com";
const KEYRING_USER_IO: &str = "api_key_io";

// OCG reads usage from the `agent-limits` CLI and has no in-app API key. When
// the CLI cannot authenticate / has no usable credentials, the tray title
// shows this clear status instead of a raw error marker.
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
    endpoint: Mutex<String>,
    refresh_interval_secs: Mutex<u64>,
    last_used_5h: Mutex<u32>,
    last_used_week: Mutex<u32>,
    last_used_month: Mutex<u32>,
}

fn keyring_entry(user: &str) -> Option<keyring::Entry> {
    match keyring::Entry::new(KEYRING_SERVICE, user) {
        Ok(entry) => Some(entry),
        Err(e) => {
            warn!("keyring entry creation failed for {}: {:?}", user, e);
            None
        }
    }
}

fn load_key_from_keyring(user: &str) -> String {
    keyring_entry(user)
        .and_then(|entry| entry.get_password().ok())
        .unwrap_or_default()
}

fn save_key_to_keyring(user: &str, key: &str) {
    if key.is_empty() {
        let _ = keyring_entry(user).and_then(|entry| entry.delete_credential().ok());
    } else {
        if let Some(entry) = keyring_entry(user) {
            if let Err(e) = entry.set_password(key) {
                error!("keyring write failed for {}: {:?}", user, e);
            }
        }
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
                save_key_to_keyring(KEYRING_USER_COM, &com);
            }
            if !io.is_empty() {
                save_key_to_keyring(KEYRING_USER_IO, &io);
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

#[tauri::command]
fn get_api_key(reveal: Option<bool>, state: tauri::State<AppState>) -> String {
    let endpoint = state
        .endpoint
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    // OCG reads from the `agent-limits` CLI and has no API key of its own.
    if endpoint == "ocg" {
        return String::new();
    }
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
    // OCG has no API key — usage comes from the `agent-limits` CLI, which
    // manages its own credentials. Never store/overwrite a key for it.
    if endpoint == "ocg" {
        log::debug!("set_api_key: ocg endpoint uses agent-limits CLI, no key stored");
        return;
    }
    let key = key.trim();
    if !key.is_empty() && (!key.starts_with("sk-") || key.len() < 4 || key.len() > 256) {
        // SEC-6-7: log only booleans / counts — never a fragment of the key.
        let prefix_ok = key.starts_with("sk-");
        let len_ok = (4..=256).contains(&key.len());
        log::warn!(
            "set_api_key: rejected (endpoint={} prefix_ok={} len_ok={} len={})",
            endpoint,
            prefix_ok,
            len_ok,
            key.len()
        );
        return;
    }
    let user = if endpoint == "io" {
        KEYRING_USER_IO
    } else {
        KEYRING_USER_COM
    };
    save_key_to_keyring(user, &key);

    if endpoint == "io" {
        *state.api_key_io.lock().unwrap_or_else(|e| e.into_inner()) = key.to_string();
    } else {
        *state.api_key_com.lock().unwrap_or_else(|e| e.into_inner()) = key.to_string();
    }
    *state.endpoint.lock().unwrap_or_else(|e| e.into_inner()) = endpoint;
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

fn redact_api_key(key: &str) -> String {
    if key.len() <= 8 {
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
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_title(Some(title.to_string()));
    }
}

async fn fetch_and_update(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    let endpoint = state
        .endpoint
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if endpoint == "ocg" {
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

// OCG (OpenCode Go) usage comes from the `agent-limits` CLI rather than a
// direct API call. GUI apps launched from /Applications inherit a minimal
// PATH, so probe common install locations before falling back to PATH lookup.
fn resolve_agent_limits_bin() -> String {
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".cargo/bin/agent-limits");
        if p.exists() {
            return p.to_string_lossy().into_owned();
        }
    }
    for p in ["/usr/local/bin/agent-limits", "/opt/homebrew/bin/agent-limits"] {
        if PathBuf::from(p).exists() {
            return p.to_string();
        }
    }
    "agent-limits".to_string()
}

async fn fetch_ocg_quota_from_cli() -> Result<serde_json::Value, String> {
    let bin = resolve_agent_limits_bin();
    log::debug!("fetch_ocg_quota: invoking {}", bin);
    let output = std::process::Command::new(&bin)
        .args(["usage", "opencodego"])
        .output()
        .map_err(|e| format!("Failed to run {}: {}", bin, e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("agent-limits exited {}: {}", output.status, stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .map_err(|e| format!("Failed to parse agent-limits output: {}", e))
}

fn parse_ocg_bars(v: &serde_json::Value) -> Result<Vec<serde_json::Value>, String> {
    let limits = v
        .get("providers")
        .and_then(|p| p.get("opencodego"))
        .and_then(|o| o.get("limits"))
        .ok_or_else(|| "agent-limits: missing providers.opencodego.limits".to_string())?;
    let mut bars = Vec::new();
    for (id, key) in [("5h", "five_hour"), ("week", "seven_day"), ("month", "monthly")] {
        let entry = limits
            .get(key)
            .ok_or_else(|| format!("agent-limits: missing limits.{}", key))?;
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
async fn fetch_ocg_quota() -> Result<serde_json::Value, String> {
    let v = fetch_ocg_quota_from_cli().await?;
    let bars = parse_ocg_bars(&v)?;
    Ok(serde_json::json!({ "bars": bars }))
}

async fn fetch_ocg_and_update(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let v = match fetch_ocg_quota_from_cli().await {
        Ok(d) => d,
        Err(e) => {
            render_title(app, OCG_NO_AUTH_TITLE);
            return Err(e);
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
        fetch_quota,
        fetch_ocg_quota,
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
            if let Ok(data_dir) = app.path().app_local_data_dir() {
                let data_dir = data_dir.join("pre-rebrand");
                migrate_config_json_if_needed(&data_dir);
            }

            // Load keys from keychain
            let com_key = load_key_from_keyring(KEYRING_USER_COM);
            let io_key = load_key_from_keyring(KEYRING_USER_IO);

            {
                let state = handle.state::<AppState>();
                *state.api_key_com.lock().unwrap_or_else(|e| e.into_inner()) = com_key;
                *state.api_key_io.lock().unwrap_or_else(|e| e.into_inner()) = io_key;
                // Endpoint defaults to "com"; restored from frontend localStorage via set_api_key
                *state.endpoint.lock().unwrap_or_else(|e| e.into_inner()) = "com".to_string();
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
            .visible(true)
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
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
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
                                    let pos = match rect.position {
                                        tauri::Position::Physical(p) => p,
                                        _ => {
                                            warn!("Tray returned non-physical position");
                                            return;
                                        }
                                    };
                                    let sz = match rect.size {
                                        tauri::Size::Physical(s) => s,
                                        _ => {
                                            warn!("Tray returned non-physical size");
                                            return;
                                        }
                                    };
                                    let scale = match window.scale_factor() {
                                        Ok(s) => s,
                                        Err(e) => {
                                            warn!(
                                                "Failed to get window scale factor: {e}, using 1.0"
                                            );
                                            1.0
                                        }
                                    };
                                    let win_w = WIDGET_OUTER_WIDTH * scale;

                                    let mut x = pos.x as f64 + sz.width as f64 - win_w;

                                    if let Some(monitor) = window.primary_monitor().ok().flatten() {
                                        let screen_w = monitor.size().width as f64;
                                        x = x.clamp(0.0, screen_w - win_w);
                                    }

                                    let y = pos.y as f64 + sz.height as f64 + 4.0 * scale;

                                    if let Err(e) =
                                        window.set_position(tauri::PhysicalPosition::new(x, y))
                                    {
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
}
