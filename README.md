# SubBar

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![Platform: macOS](https://img.shields.io/badge/platform-macOS-lightgrey)

A lightweight macOS menu bar app that shows your API subscription / quota usage
at a glance — for **Minimax** (`.com` / `.io`) and **OpenCode Go**.

The 5h interval usage percentage is shown directly in the menu bar tray. Click
the tray icon to open the widget panel with detailed usage bars and settings.

<img src="logo-sm.png" width="64" alt="SubBar">

> The README logo uses `logo-sm.png` from the repo root; the same image is
> bundled as `src/logo-sm.png` for the in-app icon. Keep both copies identical
> when updating the logo.

## Features

- **Menu bar tray label** — live 5h usage as `X%`
- **Widget panel** — at-a-glance quota bars: 5h interval, weekly, monthly (OpenCode Go)
- **Liquid Glass** — native frosted glass effect on macOS 26+ (Tahoe)
- **Dark / Light theme** — auto-detection with manual toggle
- **Secure credentials** — API keys, workspace IDs, and auth cookies stored in the OS native credential store (macOS Keychain) via the `keyring` crate; no plaintext on disk
- **Auto-refresh** — polls every 5 minutes; configurable refresh interval
- **24h time format** — clean timestamp display

## Requirements

- macOS 11.0 or later (Liquid Glass effect requires macOS 26+ Tahoe)
- A Minimax API key ([get one here](https://platform.minimaxi.com)) or OpenCode Go credentials

> **Platform scope:** the codebase supports Windows and Linux (cross-platform
> `keyring` features in `Cargo.toml`), but release artifacts and runtime testing
> are currently macOS-only.

## Install

Download the latest `.dmg` from the Releases page and drag `SubBar.app` to your
Applications folder.

Or build from source:

```bash
cargo tauri build
# App bundle at src-tauri/target/release/bundle/macos/SubBar.app
```

## Quickstart

1. **Install** — grab the latest `.dmg` from the Releases page, drag `SubBar.app`
   into your `/Applications` folder, then right-click → **Open** the first time
   (macOS Gatekeeper).
2. **Launch** — a `--` icon appears in your menu bar. Click it to open the panel,
   then click the gear icon to open settings.
3. **Pick an endpoint** — **OpenCode Go** is the default; or choose `.com` / `.io`
   for Minimax.
4. **Enter your credentials** — see *Where to find your credentials* below.
5. **Done** — close settings and the tray label updates automatically.

All credentials are stored in the OS keychain (macOS Keychain) via the `keyring`
crate — never written as plaintext to disk, and sent only over HTTPS to the
selected endpoint.

### Where to find your credentials

**OpenCode Go (default)** — needs a *workspace ID* and an *auth token*:

- **Workspace ID** — sign in to `https://opencode.ai`, open your dashboard, and
  read the address bar. The URL looks like
  `https://opencode.ai/workspace/<workspace-id>/go`; the `<workspace-id>` segment
  is your workspace ID.
- **Auth token (cookie)** — while logged into `opencode.ai`, open your browser
  developer tools (**Application / Storage → Cookies → `https://opencode.ai`**),
  find the `auth` cookie, and copy its value. The app sends it as `auth=…`
  automatically, so paste only the cookie value.

Paste the workspace ID into the `workspace id` field and the token into the
`auth cookie` field — both fields show these hints when empty.

**Minimax** — needs an API key (`sk-cp-…`):

- **API key** — create/get one from your Minimax dashboard:
  `https://platform.minimaxi.com` (or `https://platform.minimax.io` for the
  international endpoint). Paste it into the API key field after selecting
  `.com` or `.io`.

## Tech Stack

- **[Tauri v2](https://tauri.app)** — Rust backend, webview frontend
- **[tauri-plugin-liquid-glass](https://crates.io/crates/tauri-plugin-liquid-glass)** — native `NSGlassEffectView` / `NSVisualEffectView`
- **Vanilla HTML/CSS/JS** — no framework, no build step
- **[reqwest](https://crates.io/crates/reqwest)** — HTTP client for Minimax / OpenCode Go

## Development

```bash
# Run in dev mode
cd src-tauri
cargo run

# Build release bundle
cargo tauri build
```

The window is created programmatically in `main.rs` — no window is defined in
`tauri.conf.json`.

## Security

See [SECURITY.md](SECURITY.md) for the full security policy and posture
(credential storage, network, CSP, and dependency auditing).

## Verifying releases

Each GitHub Release is produced from a tagged commit on `main`:

```bash
git checkout v1.0.0
cargo tauri build --bundles app
shasum -a 256 "src-tauri/target/release/bundle/macos/SubBar.app"
```

> The `.dmg` is not Apple-notarized. After installing, right-click `SubBar.app`
> in `/Applications` and choose **Open** the first time to bypass Gatekeeper.

## License

MIT — see [LICENSE](LICENSE)