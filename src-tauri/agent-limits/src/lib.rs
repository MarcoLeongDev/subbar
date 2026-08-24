//! Vendored port of the OpenCode Go provider from
//! [f4ah6o/agent-limits](https://github.com/f4ah6o/agent-limits) (MIT).
//!
//! OpenCode Go has no public API key. Usage is scraped from the server-rendered
//! dashboard HTML at `https://opencode.ai/workspace/{workspace_id}/go`,
//! authenticated with a session cookie (`auth={auth_cookie}`). The HTML embeds
//! `rolling` / `weekly` / `monthly` usage windows; we regex-extract
//! `usagePercent` and `resetInSec` from each, mirroring upstream's parsing.
//!
//! This crate compiles into the app so no external `agent-limits` CLI is
//! required. Credentials are supplied by the caller (the host app reads them
//! from the OS keychain).

use chrono::{DateTime, Utc};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

const DASHBOARD_URL_TEMPLATE: &str = "https://opencode.ai/workspace/{}/go";
const FETCH_TIMEOUT_SECS: u64 = 10;
const USER_AGENT: &str = "SubBar/agent-limits (opencodego)";

fn re_window_block() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?:\"(rolling|weekly|monthly)\"|(rolling|weekly|monthly)Usage)\s*:\s*(?:\$R\[\d+\]\s*=\s*)?\{([^}]*)\}"#,
        )
        .unwrap()
    })
}

fn re_usage_pct() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\"?usagePercent\"?\s*:\s*(\d+(?:\.\d+)?)"#).unwrap())
}

fn re_reset_sec() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\"?resetInSec\"?\s*:\s*(\d+)"#).unwrap())
}

fn window_key(name: &str) -> &'static str {
    match name {
        "rolling" => "five_hour",
        "weekly" => "seven_day",
        "monthly" => "monthly",
        _ => "unknown",
    }
}

#[derive(Debug)]
struct Limit {
    used_percent: f64,
    resets_at: DateTime<Utc>,
}

/// Fetch OpenCode Go usage for the given workspace using the session cookie.
///
/// Returns JSON shaped exactly like `agent-limits usage opencodego`:
/// `{ "providers": { "opencodego": { "limits": { <window>: { "used_percent", "resets_at" } } } } }`
/// where `<window>` is one of `five_hour`, `seven_day`, `monthly`, and
/// `resets_at` is an RFC3339 string.
pub async fn usage(
    client: &reqwest::Client,
    workspace_id: &str,
    auth_cookie: &str,
) -> Result<serde_json::Value, String> {
    if workspace_id.trim().is_empty() || auth_cookie.trim().is_empty() {
        return Err("OpenCode Go credentials missing (workspace ID and auth cookie required)".into());
    }

    let url = DASHBOARD_URL_TEMPLATE.replacen("{}", &workspace_id.trim(), 1);

    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Cookie", format!("auth={}", auth_cookie.trim()))
        .header("Accept", "text/html,application/xhtml+xml")
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("opencodego request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return match status.as_u16() {
            401 | 403 => Err(format!(
                "HTTP {} from {url} — OpenCode Go auth cookie rejected; re-enter it in settings",
                status
            )),
            429 | 500..=599 => Err(format!("HTTP {status} from {url} (transient)")),
            _ => Err(format!("HTTP {status} from {url}")),
        };
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("opencodego: reading response: {e}"))?;

    let limits = parse_dashboard(&body)?;

    let mut limits_json = serde_json::Map::new();
    for (key, limit) in limits {
        let mut entry = serde_json::Map::new();
        entry.insert("used_percent".into(), serde_json::json!(limit.used_percent));
        entry.insert("resets_at".into(), serde_json::json!(limit.resets_at.to_rfc3339()));
        limits_json.insert(key, serde_json::Value::Object(entry));
    }

    Ok(serde_json::json!({
        "providers": {
            "opencodego": {
                "limits": serde_json::Value::Object(limits_json)
            }
        }
    }))
}

fn parse_dashboard(body: &str) -> Result<BTreeMap<String, Limit>, String> {
    let now = Utc::now();
    let mut limits: BTreeMap<String, Limit> = BTreeMap::new();

    for cap in re_window_block().captures_iter(body) {
        let window_name = cap.get(1).or_else(|| cap.get(2)).unwrap().as_str();
        let block = cap.get(3).unwrap().as_str();

        let used_pct = match re_usage_pct().captures(block) {
            Some(m) => m[1].parse::<f64>().unwrap_or(0.0),
            None => continue,
        };

        let reset_sec = match re_reset_sec().captures(block) {
            Some(m) => m[1].parse::<i64>().unwrap_or(0).max(0),
            None => continue,
        };

        let key = window_key(window_name);
        let resets_at = now + chrono::Duration::seconds(reset_sec);
        limits.insert(
            key.to_string(),
            Limit {
                used_percent: used_pct,
                resets_at,
            },
        );
    }

    if limits.is_empty() {
        return Err(
            "opencodego: no usage window data found in dashboard response".into(),
        );
    }

    Ok(limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_three_windows() {
        let html = r#"
          "rollingUsage": { "usagePercent": 12.5, "resetInSec": 18000 },
          "weeklyUsage": { "usagePercent": 42, "resetInSec": 160000 },
          "monthlyUsage": { "usagePercent": 78.0, "resetInSec": 540000 }
        "#;
        let limits = parse_dashboard(html).expect("should parse");
        assert_eq!(limits.len(), 3);
        assert_eq!(limits.get("five_hour").unwrap().used_percent, 12.5);
        assert_eq!(limits.get("seven_day").unwrap().used_percent, 42.0);
        assert_eq!(limits.get("monthly").unwrap().used_percent, 78.0);
    }

    #[test]
    fn parse_handles_quoted_keys() {
        let html = r#"
          "rolling": { "usagePercent": 0, "resetInSec": 1 },
          "weekly": { "usagePercent": 3.3, "resetInSec": 2 },
          "monthly": { "usagePercent": 9, "resetInSec": 3 }
        "#;
        let limits = parse_dashboard(html).expect("should parse");
        assert_eq!(limits.get("five_hour").unwrap().used_percent, 0.0);
        assert_eq!(limits.get("seven_day").unwrap().used_percent, 3.3);
    }

    #[test]
    fn parse_returns_error_when_no_windows() {
        assert!(parse_dashboard("no usage here").is_err());
    }
}
