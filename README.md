# SubBar 🍺

> Your API quota, right in the menu bar — so you never have to open a dashboard again.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS-lightgrey)
![Version](https://img.shields.io/badge/version-v1.0.31-blue)

A tiny macOS menu-bar app that watches your subscription / quota usage for
**Minimax** (`.com` / `.io`) and **OpenCode Go**. One click. One glance. Done.

<img src="logo-sm.png" width="80" alt="SubBar">

---

## ✨ At a glance

| | |
|---|---|
| 🧲 **Menu-bar ticker** | live 5h usage as `X%`, always visible |
| 📊 **Usage bars** | 5h / week / month quotas at a glance |
| 🪟 **Liquid Glass** | native macOS 26 glass, very pretty |
| 🌗 **Dark / light** | auto-detected, manually toggleable |
| 🔐 **Keychain-safe** | keys, workspace IDs & cookies — never plaintext on disk |
| ⏱️ **Auto-refresh** | every 5 min (configurable) |

---

## 🚀 Quickstart

1. **Install** — grab the `.dmg` from **Releases**, drag `SubBar.app` to `/Applications`,
   then right-click → **Open** on first launch (macOS Gatekeeper, totally normal).
2. **Run** — a `--` icon appears in the menu bar. Click it → gear ⚙️.
3. **Pick a side** — **OpenCode Go** is the default; or switch to Minimax `.com` / `.io`.
4. **Paste your secrets** — see the table below.
5. **Done.** Close settings and the tray starts doing its thing.

---

## 🔑 Where to find your credentials

| Endpoint | You need | Where to get it |
|---------|---------|-----------------|
| **OpenCode Go**<br>(default) | **Workspace ID** | Your dashboard URL: `opencode.ai/workspace/<workspace-id>/go` — the `<workspace-id>` bit. |
| | **Auth token** | Browser devtools → **Cookies → `opencode.ai`** → copy the `auth` cookie value. |
| **Minimax** | **API key** (`sk-…`) | [platform.minimaxi.com](https://platform.minimaxi.com) (or `platform.minimax.io` for international). |

All credentials live in the OS keychain and travel only over HTTPS.

---

## 📦 Install & build

```bash
# Download the .dmg from the Releases page, or:
cargo tauri build          # app lands in src-tauri/target/release/bundle/macos/
```

## 🛠️ Development

```bash
cd src-tauri
cargo run                 # dev mode
cargo tauri build         # release bundle
```

## 🧰 Tech stack

**Tauri v2** · **Rust** · vanilla HTML/CSS/JS · **reqwest** · **keyring**

---

## 🔒 Security

Credentials are stored in the OS native credential store (macOS Keychain), sent
only over HTTPS, and never written as plaintext to disk. Full posture →
[SECURITY.md](SECURITY.md).

## 📄 License

MIT — see [LICENSE](LICENSE)