# SubBar (Subscription Bar)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A macOS menu bar app that monitors your API subscription / quota usage at a
glance — across Minimax (`.com` / `.io`) and OpenCode Go.

Shows the 5h interval quota percentage directly in the menu bar tray. Click to reveal a widget panel with detailed usage bars and settings.

<img src="logo-sm.png" width="64" alt="SubBar">

> The README screenshot uses `logo-sm.png` from the repo root. The same image
> is also bundled as `src/logo-sm.png` for the in-app icon. When updating the
> logo, keep both copies identical.

## Features

- **Menu bar tray label** — displays 5h interval usage as `X%`
- **Widget panel** — at-a-glance quota bars: 5h interval, weekly interval, limits
- **Liquid Glass** — native frosted glass effect on macOS 26+ Tahoe
- **Dark / Light theme** — auto-detection with manual toggle
- **Secure API key** — stored in the OS native credential store (macOS Keychain, Windows Credential Manager, Linux Secret Service) via the `keyring` crate; no plaintext on disk
- **Auto-refresh** — polls every 5 minutes; configurable refresh interval
- **24h time format** — clean timestamp display

## Screenshot

<img src="logo-sm.png" width="192" alt="SubBar menubar widget showing 5h and weekly quota bars">

## Requirements

- macOS 11.0 or later (currently the only platform tested for releases — Liquid Glass effect requires macOS 26+ Tahoe)
- Minimax API key ([get one here](https://platform.minimaxi.com))

> **Platform scope:** the codebase is structured to support Windows and Linux (cross-platform `keyring` features in `Cargo.toml`), but release artifacts and runtime testing are currently macOS-only. Windows/Linux builds are not produced and may require additional testing before publishing.

## Install

Download the latest `.dmg` from this repository's Releases page and drag `SubBar.app` to your Applications folder.

Or build from the repository root:

```bash
cargo tauri build
# App bundle at src-tauri/target/release/bundle/macos/SubBar.app
```

## Setup

1. Launch the app — a `--` icon appears in your menu bar
2. Click the tray icon to open the panel
3. Click the gear icon to open settings
4. Paste your Minimax API key (`sk-cp-...`)
5. Close settings — the tray label updates automatically

Your API key is stored in the OS native keychain/credential store (e.g. macOS Keychain), never written as plaintext to disk, and sent only over HTTPS to the selected Minimax API endpoint.

## Tech Stack

- **[Tauri v2](https://tauri.app)** — Rust backend, webview frontend
- **[tauri-plugin-liquid-glass](https://crates.io/crates/tauri-plugin-liquid-glass)** — Native `NSGlassEffectView` / `NSVisualEffectView`
- **Vanilla HTML/CSS/JS** — no framework, no build step
- **[reqwest](https://crates.io/crates/reqwest)** — HTTP client for Minimax API

## Development

```bash
# Run in dev mode
cd src-tauri
cargo run

# Build release bundle
cargo tauri build
```

Window is created programmatically in `main.rs` — no window defined in `tauri.conf.json`.

## License

MIT — see [LICENSE](LICENSE)

## Verifying releases

Each GitHub Release is produced from a tagged commit on `main`. To verify a
release:

```bash
# 1. Confirm the released source tarball matches the tag
git checkout v1.0.0
git diff <downloaded-tarball>

# 2. In a fresh clone of this repository, reproduce the build (macOS only)
git checkout v1.0.0
cargo tauri build --bundles app
# Result: src-tauri/target/release/bundle/macos/SubBar.app
# Compare SHA-256 against the maintainer's published SHA-256SUMS file
shasum -a 256 "src-tauri/target/release/bundle/macos/SubBar.app"
```

The `.dmg` shipped via GitHub Releases is not Apple-notarized. After
installing, right-click `SubBar.app` in `/Applications` and choose
**Open** the first time to bypass Gatekeeper.

> **Note:** the build host's toolchain versions affect the binary's
> fingerprint. A byte-identical reproduction requires the same `rustc`,
> `cargo-tauri`, macOS SDK, and linker versions. The intent of the
> instructions above is source-comparability, not byte-reproducibility.